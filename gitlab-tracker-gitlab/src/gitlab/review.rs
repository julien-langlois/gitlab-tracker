//! Discussions read as review activity, milestone events and approvals.

use super::*;

/// A discussion thread (`/merge_requests/:iid/discussions`); system notes such
/// as "approved this merge request" come as single-note discussions.
#[derive(serde::Deserialize)]
pub(super) struct Discussion {
    #[serde(default)]
    pub(super) notes: Vec<DiscussionNote>,
}

/// `Option`s: GitLab omits (or nulls) `resolvable` / `resolved` on notes that
/// cannot be resolved.
#[derive(serde::Deserialize)]
pub(super) struct DiscussionNote {
    #[serde(default)]
    pub(super) id: u64,
    pub(super) author: Option<crate::models::GitLabUser>,
    #[serde(default)]
    pub(super) body: String,
    #[serde(default)]
    pub(super) created_at: String,
    #[serde(default)]
    pub(super) system: bool,
    pub(super) resolvable: Option<bool>,
    pub(super) resolved: Option<bool>,
}

impl DiscussionNote {
    pub(super) fn username(&self) -> &str {
        self.author.as_ref().map_or("", |a| a.username.as_str())
    }
}

/// Characters kept from a thread's first note.
pub(super) const EXCERPT_CHARS: usize = 200;

/// The resolvable threads (proper review threads, not plain comments or system
/// notes). A thread is unresolved while one of its resolvable notes is.
pub(super) fn review_threads(discussions: &[Discussion]) -> Vec<MrThread> {
    discussions
        .iter()
        .filter(|d| d.notes.iter().any(|n| n.resolvable.unwrap_or(false)))
        .filter_map(|d| {
            let first = d.notes.first()?;
            let excerpt: String = first.body.split_whitespace().collect::<Vec<_>>().join(" ");
            Some(MrThread {
                note_id: first.id,
                author: first.username().to_string(),
                excerpt: excerpt.chars().take(EXCERPT_CHARS).collect(),
                created_at: first.created_at.clone(),
                last_note_at: d
                    .notes
                    .iter()
                    .map(|n| n.created_at.as_str())
                    .max()?
                    .to_string(),
                notes: u32::try_from(d.notes.len()).unwrap_or(u32::MAX),
                resolved: !d
                    .notes
                    .iter()
                    .any(|n| n.resolvable.unwrap_or(false) && !n.resolved.unwrap_or(false)),
            })
        })
        .collect()
}

/// Whether a system note moves the MR into draft (`Some(true)`) or out of it,
/// ready for review (`Some(false)`). GitLab's wording changed over the
/// versions: "marked this merge request as **draft**" / "as **ready**" (with
/// " from `<sha>`" when a commit message did it), and earlier "marked as a
/// **Work In Progress**" / "unmarked as a **Work In Progress**". A checklist
/// item ("marked the checklist item **Draft …** as completed") is not one.
pub(super) fn draft_move(body: &str) -> Option<bool> {
    let text = body.to_lowercase().replace('*', "");
    let draft_word = |t: &str| t.contains("draft") || t.contains("work in progress");
    if let Some(rest) = text.strip_prefix("unmarked as a ") {
        return draft_word(rest).then_some(false);
    }
    let rest = text
        .strip_prefix("marked this merge request as ")
        .or_else(|| text.strip_prefix("marked as a "))
        .or_else(|| text.strip_prefix("marked as "))?;
    if rest.starts_with("ready") {
        Some(false)
    } else {
        draft_word(rest).then_some(true)
    }
}

/// Review timeline of an MR authored by `author` (username), from its notes in
/// chronological order. System notes are matched on GitLab's English wording
/// ("approved this merge request", "requested review from @x", "added 2 commits",
/// "marked this merge request as ready"): GitLab stores them in English whatever
/// the user's locale. `draft_now`: the MR is a draft today, which says how it
/// was opened when no note moved it in or out of draft.
///
/// An MR opened when the work starts is reviewed along the way while a draft:
/// those notes are counted apart ([`ReviewActivity::draft_comments`]), and the
/// review proper (first review, rounds) starts once it is ready. Bots' notes
/// are never a review.
pub(super) fn review_activity(
    discussions: &[Discussion],
    author: &str,
    draft_now: bool,
) -> ReviewActivity {
    let mut notes: Vec<&DiscussionNote> = discussions.iter().flat_map(|d| &d.notes).collect();
    notes.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut activity = ReviewActivity {
        version: crate::models::REVIEW_ACTIVITY_VERSION,
        ..ReviewActivity::default()
    };
    // Opened as a draft: its first move is out of draft (or, with none, it is
    // a draft today).
    let mut draft = notes
        .iter()
        .filter(|n| n.system)
        .find_map(|n| draft_move(&n.body))
        .map_or(draft_now, |became_draft| !became_draft);
    activity.was_draft = draft;
    for note in notes {
        let user = note.username();
        let at = &note.created_at;
        let body = note.body.trim();
        let reviewed = if note.system {
            if let Some(became_draft) = draft_move(body) {
                draft = became_draft;
                if became_draft {
                    activity.was_draft = true;
                } else if activity.first_review_at.is_none() {
                    activity.ready_at = Some(at.clone());
                }
                false
            } else if body == "approved this merge request" {
                activity.approved_at = Some(at.clone());
                if !user.is_empty() {
                    activity.approvals.insert(user.to_string(), at.clone());
                }
                true
            } else if body == "unapproved this merge request" {
                activity.approved_at = None;
                activity.approvals.remove(user);
                false
            } else if let Some(who) = body.strip_prefix("requested review from ") {
                if activity.first_review_requested_at.is_none() {
                    activity.first_review_requested_at = Some(at.clone());
                }
                for reviewer in mentions(who) {
                    activity
                        .review_requested_at
                        .insert(reviewer.to_string(), at.clone());
                }
                false
            } else if let Some(who) = body.strip_prefix("removed review request for ") {
                for reviewer in mentions(who) {
                    activity.review_requested_at.remove(reviewer);
                }
                false
            } else {
                if body.starts_with("added ") && body.contains(" commit") {
                    activity.last_push_at = Some(at.clone());
                    if !user.is_empty() {
                        activity.last_push_by.insert(user.to_string(), at.clone());
                    }
                    if activity.first_review_at.is_some() {
                        activity.pushes_after_review += 1;
                    }
                }
                false
            }
        } else {
            true
        };
        if reviewed && !user.is_empty() {
            activity
                .last_activity_by
                .insert(user.to_string(), at.clone());
            if user != author && !is_bot(user) {
                if draft {
                    activity.draft_comments += 1;
                } else if activity.first_review_at.is_none() {
                    activity.first_review_at = Some(at.clone());
                }
            }
        }
    }
    activity
}

/// Usernames mentioned as `@name` in a system note ("@alice, @bob and @carol").
pub(super) fn mentions(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .map(|name| name.trim_end_matches([',', '.', ';']))
        .filter(|name| !name.is_empty())
}

/// All discussions of a MR (`None` when a page failed: the cache is kept).
pub(super) async fn fetch_discussions(ctx: &FetchContext, mr_id: &str) -> Option<Vec<Discussion>> {
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/discussions?",
        ctx.base_url, ctx.project_id, mr_id
    );
    get_all_pages(ctx, &url).await
}

/// One entry of `/resource_milestone_events`.
#[derive(serde::Deserialize)]
pub(super) struct GitLabMilestoneEvent {
    pub(super) created_at: String,
    pub(super) action: String,
    pub(super) milestone: Option<GitLabMilestone>,
    pub(super) user: Option<crate::models::GitLabUser>,
}

/// When the MR's milestone was set, changed or removed, oldest first (`None` on
/// any error).
pub(super) async fn fetch_milestone_events(
    ctx: &FetchContext,
    mr_id: &str,
) -> Option<Vec<crate::models::MilestoneEvent>> {
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/resource_milestone_events?",
        ctx.base_url, ctx.project_id, mr_id
    );
    let mut events: Vec<crate::models::MilestoneEvent> =
        get_all_pages::<GitLabMilestoneEvent>(ctx, &url)
            .await?
            .into_iter()
            .map(|e| crate::models::MilestoneEvent {
                at: e.created_at,
                milestone: e.milestone.map(|m| m.title),
                added: e.action != "remove",
                by: e.user.map(|u| format!("{} (@{})", u.name, u.username)),
            })
            .collect();
    events.sort_by(|a, b| a.at.cmp(&b.at));
    Some(events)
}

/// Whether the milestone events must be read: never read yet, a forced refresh,
/// or the milestone is no longer the one they were read with. A change undone
/// between two refreshes (A → B → A) is only seen by a forced refresh.
pub(super) fn milestone_events_due(cached: &CachedMrData, milestone: Option<&str>) -> bool {
    cached.cache_policy.notes_stale()
        || cached.milestone_events.is_none()
        || cached.milestone.as_deref() != milestone
}

/// `/approvals` response: only the current approvers matter.
#[derive(serde::Deserialize)]
pub(super) struct Approvals {
    #[serde(default)]
    pub(super) approved_by: Vec<Approver>,
}

#[derive(serde::Deserialize)]
pub(super) struct Approver {
    pub(super) user: crate::models::GitLabUser,
}

/// The MR's current approvers (`None` on any error).
pub(super) async fn fetch_approved_by(
    ctx: &FetchContext,
    mr_id: &str,
) -> Option<Vec<crate::models::GitLabUser>> {
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/approvals",
        ctx.base_url, ctx.project_id, mr_id
    );
    let res = http_client()
        .get(url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
        .ok()
        .filter(|r| r.status().is_success())?;
    let approvals: Approvals = res.json().await.ok()?;
    Some(approvals.approved_by.into_iter().map(|a| a.user).collect())
}
