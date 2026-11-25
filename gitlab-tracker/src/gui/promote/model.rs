//! What can go from one branch to the next: the MRs between them, and the cherry-pick commands that bring them.

use super::*;

/// Where an MR stands between the two branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// On `from`, not (or partly) on `to`.
    Ready,
    /// Not on `from` yet.
    Upstream,
    /// Already on `to`.
    Done,
}

/// An MR on the page.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub author: String,
    pub stage: Stage,
    /// How it is on each branch (`None`: not merged yet, or loading).
    pub on_from: Option<BranchPresence>,
    pub on_to: Option<BranchPresence>,
    /// What holds it back, for the MRs not on `from` yet.
    pub blockers: Vec<String>,
    pub state: GitlabMrState,
    pub mergeability: MergeabilityStatus,
    /// When it was merged (RFC 3339), for the merge order.
    pub merged_at: Option<String>,
    /// Since when it is on `from`: its arrival there, else its merge when
    /// `from` is its target.
    pub on_from_since: Option<chrono::DateTime<chrono::Utc>>,
    /// Since when it is on `to`, when known.
    pub on_to_since: Option<chrono::DateTime<chrono::Utc>>,
    /// Its ticket: id and status.
    pub ticket: Option<(String, String)>,
    pub effort: Option<crate::models::Effort>,
    /// The commit that landed it (merge or squash), else none.
    pub landed: Option<String>,
    /// Its own commits, oldest first.
    pub commits: Vec<String>,
}

/// The MRs between `from` and `to`; closed MRs and those still loading are left out.
pub fn rows(app: &App, from: &str, to: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for mr in &app.mrs {
        if mr.title.is_empty() || mr.state == GitlabMrState::Closed {
            continue;
        }
        let on_from = app.branch_presence(mr, from);
        let on_to = app.branch_presence(mr, to);
        let arrived = |p: Option<BranchPresence>| p.is_some_and(|p| !p.is_missing());
        let stage = if arrived(on_to) {
            Stage::Done
        } else if arrived(on_from) {
            Stage::Ready
        } else {
            Stage::Upstream
        };
        let mut blockers = Vec::new();
        if stage == Stage::Upstream {
            if mr.state == GitlabMrState::Opened {
                if mr
                    .pipelines
                    .first()
                    .is_some_and(|p| p.status == PipelineState::Failed)
                {
                    blockers.push(tl("CI failed").to_string());
                }
                if mr.user_notes_count > 0 {
                    blockers.push(tn(
                        u64::from(mr.user_notes_count),
                        "{n} open thread",
                        "{n} open threads",
                    ));
                }
                if mr.approved_by.is_empty() && mr.mergeability != MergeabilityStatus::Draft {
                    blockers.push(tl("Not approved").to_string());
                }
            } else if on_from == Some(BranchPresence::Partial) {
                blockers.push(tl("Partly copied").to_string());
            }
        }
        let parse = |at: &str| {
            chrono::DateTime::parse_from_rfc3339(at)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc))
        };
        let on_from_since = app
            .branch_arrivals
            .get(&(mr.id.clone(), from.to_string()))
            .copied()
            .or_else(|| {
                (mr.target_branch == from)
                    .then(|| mr.merged_at.as_deref().and_then(parse))
                    .flatten()
            });
        let on_to_since = app
            .branch_arrivals
            .get(&(mr.id.clone(), to.to_string()))
            .copied()
            .or_else(|| {
                (mr.target_branch == to)
                    .then(|| mr.merged_at.as_deref().and_then(parse))
                    .flatten()
            });
        rows.push(Row {
            id: mr.id.clone(),
            title: mr.title.clone(),
            author: mr.author.clone(),
            stage,
            on_from,
            on_to,
            blockers,
            state: mr.state,
            mergeability: mr.mergeability,
            merged_at: mr.merged_at.clone(),
            on_from_since,
            on_to_since,
            ticket: mr
                .linked_ticket
                .as_ref()
                .map(|t| (t.id.clone(), t.status.clone())),
            effort: mr.diff_stats.as_ref().map(|d| {
                crate::models::Effort::from_score(d.difficulty(&app.config.complexity_profile))
            }),
            landed: mr.sha.clone().filter(|_| mr.state == GitlabMrState::Merged),
            // GitLab lists them newest first.
            commits: mr.commits.iter().rev().map(|c| c.sha.clone()).collect(),
        });
    }
    // In merge order: the order `from` got them, and the order to bring them.
    rows.sort_by(|a, b| match (&a.merged_at, &b.merged_at) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    rows
}

/// The commands that bring `rows` (in their order) from `from` to `to`, one
/// `git cherry-pick -x` per MR: its merge or squash commit with `-m 1` (git
/// 2.20+ takes it for a plain commit too), else its own commits.
pub fn commands(rows: &[&Row], from: &str, to: &str) -> String {
    let mut out = format!(
        "# {}\ngit fetch origin\ngit switch {to}\ngit pull --ff-only\n",
        tf(
            &tn(
                rows.len() as u64,
                "Bring {n} MR from {from} to {to}",
                "Bring {n} MRs from {from} to {to}",
            ),
            &[("from", from), ("to", to)],
        ),
    );
    for r in rows {
        out.push_str(&format!("\n# !{} {}\n", r.id, r.title));
        match (&r.landed, r.commits.is_empty()) {
            (Some(sha), _) => out.push_str(&format!("git cherry-pick -x -m 1 {sha}\n")),
            (None, false) => {
                out.push_str(&format!(
                    "git fetch origin refs/merge-requests/{}/head\ngit cherry-pick -x {}\n",
                    r.id,
                    r.commits.join(" ")
                ));
            }
            (None, true) => {
                out.push_str(&format!("# {}\n", tl("No commit known yet: refresh first")))
            }
        }
    }
    out.push_str(&format!("\ngit push origin {to}\n"));
    out
}

/// The pair of branches shown: the one picked when still tracked, else the
/// last two tracked branches (the last hop of the release train).
pub(super) fn pair(g: &GuiApp) -> Option<(String, String)> {
    let branches = &g.app.branches;
    if branches.len() < 2 {
        return None;
    }
    if let Some((from, to)) = &g.promote {
        if from != to && branches.contains(from) && branches.contains(to) {
            return Some((from.clone(), to.clone()));
        }
    }
    let n = branches.len();
    Some((branches[n - 2].clone(), branches[n - 1].clone()))
}
