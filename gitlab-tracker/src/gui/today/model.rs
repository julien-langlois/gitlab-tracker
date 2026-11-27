//! Where each MR of every loaded tab stands for me: what it needs, or none of my business.

use super::*;

/// Where an MR stands for me; each MR shows under the first that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Someone asked for my review, and I have not approved yet.
    Review,
    /// Mine, and something needs me: CI failed, conflicts, threads, changes asked.
    Fix,
    /// Mine, approved and mergeable.
    Ready,
    /// Assigned to me, someone else's.
    Assigned,
    /// Mine, waiting on the others.
    Waiting,
    /// Merged by me today, whoever wrote it.
    Merged,
}

impl Kind {
    pub(super) const ALL: [Kind; 6] = [
        Kind::Review,
        Kind::Fix,
        Kind::Ready,
        Kind::Assigned,
        Kind::Waiting,
        Kind::Merged,
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Kind::Review => tl("Waiting for my review"),
            Kind::Fix => tl("My MRs that need me"),
            Kind::Ready => tl("Ready to merge"),
            Kind::Assigned => tl("Assigned to me"),
            Kind::Waiting => tl("Waiting on the others"),
            Kind::Merged => tl("Merged today"),
        }
    }

    pub(super) fn empty(self) -> &'static str {
        match self {
            Kind::Review => tl("No review waits for you."),
            Kind::Fix => tl("None of your MRs is failing, in conflict or awaiting an answer."),
            Kind::Ready => tl("None of your MRs is approved and mergeable yet."),
            Kind::Assigned => tl("Nothing else is assigned to you."),
            Kind::Waiting => tl("None of your MRs waits on a reviewer."),
            Kind::Merged => tl("You have not merged anything today yet."),
        }
    }

    pub(super) fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Kind::Review => t.amber,
            Kind::Fix => t.red,
            Kind::Ready => t.green,
            Kind::Assigned => t.blue,
            Kind::Waiting => t.teal,
            Kind::Merged => t.purple,
        }
    }
}

/// An MR on the page.
#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    /// The tab it is in.
    pub tab: usize,
    pub project: String,
    pub id: String,
    pub title: String,
    pub author: String,
    /// Why it is here, shortest first: "CI failed", "2 threads"…
    pub reasons: Vec<String>,
    pub state: GitlabMrState,
    pub mergeability: MergeabilityStatus,
    /// Since when it waits, and the usual wait when known: "6 d · usually 1 d".
    pub since: Option<String>,
    /// Waits too long for that step: the since in amber.
    pub late: bool,
    /// A message to remind whoever it waits on, for the MRs waiting on the others.
    pub reminder: Option<String>,
    /// The tab of the MR's page that helps with it, and the button's name.
    pub action: Option<(super::super::mr_page::Tab, &'static str)>,
    /// Snoozed: until when ("until Mon 09:00"); it then shows apart, at the end.
    pub snoozed: Option<String>,
    /// A review: how many MRs it lets go (stacked on it, or to merge after it
    /// on the same lines), and in how many days its milestone is due.
    pub unblocks: usize,
    pub due_in: Option<i64>,
    /// Days it has waited for its step, for the oldest-first order.
    pub waited: Option<f64>,
}

/// Days within which a milestone's date makes a review come first.
const DUE_SOON_DAYS: i64 = 14;

/// What a review lets go: the reasons to show, how many MRs, and in how many
/// days its milestone is due (when soon, or past).
fn unblocking(
    mr: &crate::models::TrackedMr,
    read: &(
        super::super::overlaps::Overlaps,
        Vec<super::super::stacks::Stack>,
    ),
    today: chrono::NaiveDate,
) -> (Vec<String>, usize, Option<i64>) {
    let ids = |list: &[String]| {
        list.iter()
            .map(|id| format!("!{id}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut reasons = Vec::new();
    let stacked = super::super::stacks::stacked_on(&read.1, &mr.id);
    if !stacked.is_empty() {
        reasons.push(tf(
            "{n} stacked on it: {mrs}",
            &[
                ("n", &tn(stacked.len() as u64, "{n} MR", "{n} MRs")),
                ("mrs", &ids(&stacked)),
            ],
        ));
    }
    let after: Vec<String> = super::super::overlaps::merge_after(&read.0, &mr.id)
        .into_iter()
        .filter(|id| !stacked.contains(id))
        .collect();
    if !after.is_empty() {
        reasons.push(tf(
            "{mrs} to merge after it (same lines)",
            &[("mrs", &ids(&after))],
        ));
    }
    let due_in = mr
        .milestone_due_date
        .as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|due| (due - today).num_days())
        .filter(|days| *days <= DUE_SOON_DAYS);
    if let (Some(days), Some(milestone)) = (due_in, mr.milestone.as_deref()) {
        reasons.push(if days >= 0 {
            tf(
                "milestone {name} due in {days}",
                &[
                    ("name", milestone),
                    ("days", &tn(days as u64, "{n} day", "{n} days")),
                ],
            )
        } else {
            tf(
                "milestone {name} {days} late",
                &[
                    ("name", milestone),
                    ("days", &tn((-days) as u64, "{n} day", "{n} days")),
                ],
            )
        });
    }
    (reasons, stacked.len() + after.len(), due_in)
}

/// Where each MR of `app` stands for `username`; the MRs that are none of my
/// business are left out.
pub fn classify(app: &App, username: &str, tab: usize, today: chrono::NaiveDate) -> Vec<Item> {
    use crate::next_step::{days_text, first_name, Actor};
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let usual_review = crate::next_step::usual_first_review_days(app);
    let now = chrono::Utc::now();
    let local_now = chrono::Local::now();
    // Built the first time a reminder needs it.
    let mut links: Option<super::super::board::Links> = None;
    // The overlaps and stacks, read the first time a review needs them.
    let mut unblock_read = None;
    let mut items = Vec::new();
    for mr in &app.mrs {
        if mr.title.is_empty() {
            continue;
        }
        let open = mr.state == GitlabMrState::Opened;
        let mine = matches_gitlab_username(&mr.author, username) || mr.author == username;
        let assigned = mr
            .assignee
            .as_deref()
            .is_some_and(|a| matches_gitlab_username(a, username) || a == username);
        let mut reasons = Vec::new();
        let kind = if awaiting_review_by(
            mr.state,
            mr.mergeability,
            &mr.reviewers,
            &mr.approved_by,
            username,
        ) && !mine
        {
            Some(Kind::Review)
        } else if mine && open {
            if mr
                .pipelines
                .first()
                .is_some_and(|p| p.status == PipelineState::Failed)
            {
                reasons.push(tl("CI failed").to_string());
            }
            // Said by the status badge already: no chip of its own.
            let blocked = matches!(
                mr.mergeability,
                MergeabilityStatus::Conflict
                    | MergeabilityStatus::NeedsRebase
                    | MergeabilityStatus::RequestedChanges
            );
            if mr.user_notes_count > 0 {
                reasons.push(tn(
                    u64::from(mr.user_notes_count),
                    "{n} thread to answer",
                    "{n} threads to answer",
                ));
            }
            // Not in conflict yet, but it will be at the next rebase or merge
            // (a merge train would stop on it): the author's to handle.
            if let Some(first) = super::super::overlaps::ahead(app, mr).first() {
                reasons.push(tf(
                    "A conflict is coming: !{id}, merged into {target}, changed the same lines",
                    &[("id", &first.merged), ("target", &mr.target_branch)],
                ));
            }
            if blocked || !reasons.is_empty() {
                Some(Kind::Fix)
            } else if mr.auto_merge || app.merge_train.contains_key(&mr.id) {
                // On the train, or set to auto-merge: GitLab merges it, nobody
                // has to.
                reasons.push(tl("merging by itself").to_string());
                Some(Kind::Waiting)
            } else if mr.mergeability == MergeabilityStatus::Mergeable && !mr.approved_by.is_empty()
            {
                reasons.push(tn(
                    mr.approved_by.len() as u64,
                    "{n} approval",
                    "{n} approvals",
                ));
                Some(Kind::Ready)
            } else {
                if mr.mergeability == MergeabilityStatus::Draft {
                    reasons.push(tl("Still a draft").to_string());
                } else if mr.reviewers.is_empty() {
                    reasons.push(tl("No reviewer").to_string());
                }
                Some(Kind::Waiting)
            }
        } else if assigned && open {
            Some(Kind::Assigned)
        } else if mr.state == GitlabMrState::Merged
            // Merged by me, whoever wrote it: what I merged is the day's count.
            && mr
                .merged_by
                .as_deref()
                .is_some_and(|by| matches_gitlab_username(by, username) || by == username)
            && mr
                .merged_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| at.with_timezone(&chrono::Local).date_naive() == today)
        {
            Some(Kind::Merged)
        } else {
            None
        };
        // Who it waits on, since when, and (waiting on the others) a reminder.
        let step = crate::next_step::of(app, mr, Some(username));
        let waited = step.waited_days(now);
        let usual = (step.actor == Actor::Reviewers)
            .then_some(usual_review)
            .flatten();
        let late = match (waited, usual) {
            (Some(w), Some(u)) => w > u * 1.5 && w - u > 0.5,
            (Some(w), None) => w > 7.0,
            _ => false,
        };
        let since = waited.map(|w| match usual {
            Some(u) => tf(
                "{time} · usually {usual}",
                &[("time", &days_text(w)), ("usual", &days_text(u))],
            ),
            None => days_text(w),
        });
        let (mut unblocks, mut due_in) = (0, None);
        if kind == Some(Kind::Review) {
            let read = unblock_read.get_or_insert_with(|| {
                (
                    super::super::overlaps::analyze(app, false, true),
                    super::super::stacks::stacks(app),
                )
            });
            let (why, n, due) = unblocking(mr, read, today);
            reasons.extend(why);
            (unblocks, due_in) = (n, due);
        }
        let mut reminder = None;
        if kind == Some(Kind::Waiting) && !step.mine && !step.people.is_empty() {
            let names: Vec<&str> = step.people.iter().map(|p| first_name(p)).collect();
            let what = step
                .todos
                .first()
                .map(|t| t.text().to_lowercase())
                .unwrap_or_default();
            reasons.push(tf(
                "Waiting on {who}: {what}",
                &[("who", &names.join(", ")), ("what", &what)],
            ));
            let links = links.get_or_insert_with(|| super::super::board::Links::of(app));
            reminder = Some(links.apply(&tf(
                "Hi {who}, could you have a look at !{id} \"{title}\" when you can? (waiting for {time})",
                &[
                    ("who", &names.join(", ")),
                    ("id", &mr.id),
                    ("title", &mr.title),
                    ("time", &waited.map(days_text).unwrap_or_default()),
                ],
            )));
        }
        use crate::next_step::Todo;
        let action = match kind {
            Some(Kind::Review) => Some(if step.todos.contains(&Todo::Review { again: true }) {
                (
                    super::super::mr_page::Tab::Commits,
                    tl("See the new commits"),
                )
            } else {
                (super::super::mr_page::Tab::Changes, tl("Review it"))
            }),
            Some(Kind::Fix)
                if step
                    .todos
                    .iter()
                    .any(|t| matches!(t, Todo::AnswerThreads { .. })) =>
            {
                Some((
                    super::super::mr_page::Tab::Discussions,
                    tl("Open the threads"),
                ))
            }
            Some(Kind::Fix) if step.todos.contains(&Todo::FixPipeline) => Some((
                super::super::mr_page::Tab::Pipelines,
                tl("See the pipeline"),
            )),
            _ => None,
        };
        let shows_since = matches!(
            kind,
            Some(Kind::Review | Kind::Fix | Kind::Waiting | Kind::Ready | Kind::Assigned)
        );
        if let Some(kind) = kind {
            items.push(Item {
                kind,
                tab,
                project: project.clone(),
                id: mr.id.clone(),
                title: mr.title.clone(),
                author: mr.author.clone(),
                reasons,
                state: mr.state,
                mergeability: mr.mergeability,
                since: since.filter(|_| shows_since),
                late: late && shows_since,
                reminder,
                action,
                snoozed: mr
                    .snooze
                    .as_ref()
                    .filter(|_| open)
                    .map(|s| s.text(&local_now)),
                unblocks,
                due_in,
                waited,
            });
        }
    }
    items
}

/// The reviews in the order picked: what each lets go first (MRs stacked on
/// it or to merge after it, then a milestone near its date), then the longest
/// wait; or the longest wait alone.
pub(super) fn sort_reviews(list: &mut [&Item], oldest_first: bool) {
    let waited = |i: &Item| std::cmp::Reverse(wait_minutes(i.waited));
    if oldest_first {
        list.sort_by_key(|i| waited(i));
    } else {
        list.sort_by_key(|i| {
            (
                std::cmp::Reverse(i.unblocks),
                i.due_in.unwrap_or(i64::MAX),
                waited(i),
            )
        });
    }
}

/// A wait in minutes, for sorting (none: no wait).
fn wait_minutes(days: Option<f64>) -> i64 {
    days.map_or(-1, |d| (d * 1440.0) as i64)
}

/// What waits for me now (reviews and fixes), for the rail's badge.
pub fn waiting_count(g: &GuiApp) -> usize {
    collect(g)
        .0
        .iter()
        .filter(|i| i.snoozed.is_none() && matches!(i.kind, Kind::Review | Kind::Fix))
        .count()
}

/// The MRs of every loaded tab, and the usernames they were read as; kept
/// between frames (see `memo.rs`).
pub(in crate::gui) fn collect(g: &GuiApp) -> std::rc::Rc<(Vec<Item>, Vec<String>)> {
    let open = g
        .tabs
        .iter()
        .filter(|t| matches!(t.state, SlotState::Active | SlotState::Parked(_)))
        .count();
    g.memo.today.get("", open, || read_all(g))
}

fn read_all(g: &GuiApp) -> (Vec<Item>, Vec<String>) {
    let today = chrono::Local::now().date_naive();
    let mut items = Vec::new();
    let mut users = Vec::new();
    for (index, slot) in g.tabs.iter().enumerate() {
        let app = match &slot.state {
            SlotState::Active => &g.app,
            SlotState::Parked(tab) => &tab.app,
            _ => continue,
        };
        let Some(username) = app.config.gitlab_username.as_deref() else {
            continue;
        };
        if !users.iter().any(|u| u == username) {
            users.push(username.to_string());
        }
        items.extend(classify(app, username, index, today));
    }
    (items, users)
}
