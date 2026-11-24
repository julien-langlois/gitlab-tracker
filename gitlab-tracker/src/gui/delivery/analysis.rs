//! Reading the tracked MRs into a `Delivery`: paths, reverts, hotfixes, deployments, the period before.

use super::*;

pub(super) fn percentile(mut values: Vec<f64>, p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * p).round() as usize;
    values.get(index).copied()
}

/// Rounds of review an MR went through: its pushes after the first review
/// (at least one when its last push followed the first review, for MRs read
/// before the count existed).
pub(super) fn rounds_of(mr: &TrackedMr) -> Option<u32> {
    let review = mr.review.as_ref()?;
    let first = board::time_of(review.first_review_at.as_deref())?;
    let late_push = board::time_of(review.last_push_at.as_deref()).is_some_and(|p| p > first);
    Some(if review.pushes_after_review == 0 && late_push {
        1
    } else {
        review.pushes_after_review
    })
}

/// The path of `mr`, with `last` the last branch of the flow.
pub fn path_of(app: &App, mr: &TrackedMr, last: &str) -> Option<Path> {
    let created = board::time_of(mr.created_at.as_deref())?;
    let review = mr.review.as_ref();
    let ready = review.and_then(|r| board::time_of(r.up_for_review_at()));
    let merged = (mr.state == GitlabMrState::Merged)
        .then(|| board::time_of(mr.merged_at.as_deref()))
        .flatten();
    let delivered = merged.and_then(|m| {
        if last.is_empty() || mr.target_branch == last {
            return Some(m);
        }
        let on_last = app
            .branch_presence(mr, last)
            .is_some_and(|p| !p.is_missing());
        on_last
            .then(|| super::super::milestones::arrived(app, mr, last))
            .flatten()
            .map(|at| at.max(m))
    });
    Some(Path {
        id: mr.id.clone(),
        title: mr.title.clone(),
        author: mr.author.clone(),
        created,
        ready,
        never_draft: review.is_some_and(|r| !r.was_draft),
        draft_comments: review.map_or(0, |r| r.draft_comments),
        reviewed: review.and_then(|r| board::time_of(r.first_review_at.as_deref())),
        approved: review.and_then(|r| board::time_of(r.approved_at.as_deref())),
        merged,
        delivered,
        first_commit: mr
            .commits
            .iter()
            .map(|c| c.fingerprint.authored_at)
            .min()
            .and_then(|s| DateTime::from_timestamp(s, 0))
            .filter(|at| *at <= created),
        rounds: rounds_of(mr),
    })
}

/// Whether an MR undoes another: GitLab's `Revert "…"`.
pub(super) fn is_revert(title: &str) -> bool {
    title.trim_start().to_lowercase().starts_with("revert")
}

/// Whether an MR is a fix rushed to production: a hotfix label, or a source
/// branch among the project's hotfix branches (`hotfix/*` unless set).
pub(super) fn is_hotfix(app: &App, mr: &TrackedMr) -> bool {
    mr.labels
        .iter()
        .any(|l| l.to_lowercase().contains("hotfix"))
        || app.config.is_hotfix_branch(&mr.source_branch)
}

// ── Reverts and hotfixes ─────────────────────────────────────────────────────

/// A revert or a hotfix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoKind {
    Revert,
    Hotfix,
}

/// How the MR a revert or hotfix undoes was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// The description names it: "This reverts merge request !12" (GitLab's
    /// Revert button).
    Description,
    /// The description names one of its commits: "This reverts commit 1a2b3c".
    Commit,
    /// Its title, quoted: `Revert "feat: …"`.
    Title,
    /// A guess: the latest MR merged before it that changed the same files.
    SameFiles,
}

/// A revert or hotfix merged in the period.
#[derive(Debug, Clone)]
pub struct Undo {
    pub kind: UndoKind,
    pub id: String,
    pub title: String,
    pub merged: DateTime<Utc>,
    /// The MR it undoes or patches: (id, title, how it was found).
    pub undoes: Option<(String, String, Found)>,
    /// Days from that MR's merge to this one's.
    pub after: Option<f64>,
    /// The furthest tracked branch that MR reached (a hotfix without one: its own).
    pub reached: Option<String>,
    /// Counted in the change failure rate: on production in the period.
    pub counted: bool,
}

/// The digits of `!123` at the start of `text` (after the `!`).
pub(super) fn mr_number(text: &str) -> Option<&str> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    (end > 0).then(|| &text[..end])
}

/// The merged MR `revert` undoes, and how it was found: the description's
/// `!id` or commit, else the title (its own or a commit's) in quotes.
pub fn reverted<'a>(app: &'a App, revert: &TrackedMr) -> Option<(&'a TrackedMr, Found)> {
    if !is_revert(&revert.title) {
        return None;
    }
    let merged = |m: &&TrackedMr| m.id != revert.id && m.state == GitlabMrState::Merged;
    let text = revert.description.to_lowercase();
    // "This reverts merge request !12".
    for (at, _) in text.match_indices("reverts merge request !") {
        let rest = &text[at + "reverts merge request !".len()..];
        if let Some(found) =
            mr_number(rest).and_then(|id| app.mrs.iter().filter(merged).find(|m| m.id == id))
        {
            return Some((found, Found::Description));
        }
    }
    // "This reverts commit 1a2b3c4…".
    for (at, _) in text.match_indices("reverts commit ") {
        let rest = &text[at + "reverts commit ".len()..];
        let sha: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
        if sha.len() < 7 {
            continue;
        }
        let found = app.mrs.iter().filter(merged).find(|m| {
            m.commits
                .iter()
                .any(|c| c.sha.starts_with(&sha) || sha.starts_with(&c.sha))
        });
        if let Some(found) = found {
            return Some((found, Found::Commit));
        }
    }
    // `Revert "title"`, in the MR's title or one of its commits'.
    let quoted = |title: &str| {
        title
            .split('"')
            .nth(1)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    let titles = std::iter::once(revert.title.as_str())
        .chain(revert.commits.iter().map(|c| c.fingerprint.title.as_str()));
    for undone in titles.filter(|t| is_revert(t)).filter_map(quoted) {
        let found =
            app.mrs.iter().filter(merged).find(|m| {
                m.title == undone || m.commits.iter().any(|c| c.fingerprint.title == undone)
            });
        if let Some(found) = found {
            return Some((found, Found::Title));
        }
    }
    None
}

/// How far back a hotfix's cause is looked for.
pub(super) const HOTFIX_LOOKBACK_DAYS: i64 = 14;

/// A guess at what `hotfix` patches: of the MRs merged in the two weeks before
/// it (no revert or hotfix), the one sharing the most files with it, the
/// latest on a tie.
pub(super) fn patched<'a>(
    app: &'a App,
    hotfix: &TrackedMr,
    merged: DateTime<Utc>,
) -> Option<&'a TrackedMr> {
    let files: Vec<&str> = hotfix
        .diff_stats
        .as_ref()?
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    if files.is_empty() {
        return None;
    }
    let since = merged - Duration::days(HOTFIX_LOOKBACK_DAYS);
    app.mrs
        .iter()
        .filter(|m| m.id != hotfix.id && m.state == GitlabMrState::Merged)
        .filter(|m| !is_revert(&m.title) && !is_hotfix(app, m))
        .filter_map(|m| {
            let at = m.merged_at.as_deref().and_then(board::parse_time)?;
            if at > merged || at < since {
                return None;
            }
            let shared = m
                .diff_stats
                .as_ref()?
                .files
                .iter()
                .filter(|f| files.contains(&f.path.as_str()))
                .count();
            (shared > 0).then_some((m, shared, at))
        })
        .max_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)))
        .map(|(m, _, _)| m)
}

/// The furthest of `ordered` (feature to production) that `mr` is on.
pub(super) fn furthest(app: &App, mr: &TrackedMr, ordered: &[String]) -> Option<String> {
    ordered
        .iter()
        .rev()
        .find(|b| app.branch_presence(mr, b).is_some_and(|p| !p.is_missing()))
        .cloned()
}

/// The reverts and hotfixes merged since `start`, newest first.
pub(super) fn undos(
    app: &App,
    ordered: &[String],
    start: DateTime<Utc>,
    counted: &[String],
) -> Vec<Undo> {
    let mut list: Vec<Undo> = app
        .mrs
        .iter()
        .filter(|m| m.state == GitlabMrState::Merged && !m.title.is_empty())
        .filter_map(|mr| {
            let merged = mr.merged_at.as_deref().and_then(board::parse_time)?;
            if merged < start {
                return None;
            }
            let (kind, undone) = if is_revert(&mr.title) {
                (UndoKind::Revert, reverted(app, mr))
            } else if is_hotfix(app, mr) {
                (
                    UndoKind::Hotfix,
                    patched(app, mr, merged).map(|m| (m, Found::SameFiles)),
                )
            } else {
                return None;
            };
            let after = undone.and_then(|(m, _)| {
                let at = m.merged_at.as_deref().and_then(board::parse_time)?;
                Some(board::days_between(at, merged).max(0.0))
            });
            let reached = match (kind, undone) {
                (_, Some((m, _))) => furthest(app, m, ordered),
                (UndoKind::Hotfix, None) => furthest(app, mr, ordered),
                (UndoKind::Revert, None) => None,
            };
            Some(Undo {
                kind,
                id: mr.id.clone(),
                title: mr.title.clone(),
                merged,
                undoes: undone.map(|(m, found)| (m.id.clone(), m.title.clone(), found)),
                after,
                reached,
                counted: counted.contains(&mr.id),
            })
        })
        .collect();
    list.sort_by(|a, b| b.merged.cmp(&a.merged).then(a.id.cmp(&b.id)));
    list
}

/// The deployments to production: those of the last branch, else of an
/// environment named like production. Oldest first.
pub(super) fn production_deployments<'a>(
    overview: Option<&'a ProjectOverview>,
    last: &str,
) -> Vec<&'a DeploymentRecord> {
    let Some(overview) = overview else {
        return Vec::new();
    };
    let mut list: Vec<&DeploymentRecord> = overview
        .deployments
        .iter()
        .filter(|d| !last.is_empty() && d.reference == last)
        .collect();
    if list.is_empty() {
        list = overview
            .deployments
            .iter()
            .filter(|d| d.environment.to_lowercase().contains("prod"))
            .collect();
    }
    list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    list
}

/// The delivery of `app`'s MRs over the `days` before `now` (an earlier
/// `now`: the period before, to compare with; nothing after it counts).
pub fn analyze(
    app: &App,
    overview: Option<&ProjectOverview>,
    now: DateTime<Utc>,
    days: i64,
) -> Delivery {
    let (last, ordered) = super::super::milestones::flow_of(app);
    let single = ordered.len() < 2;
    let last = if single { String::new() } else { last };
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let mut d = Delivery {
        production: if single {
            ordered.first().cloned().unwrap_or_default()
        } else {
            last.clone()
        },
        ..Delivery::default()
    };
    let mut to_merge = Vec::new();
    let mut leads = Vec::new();
    let mut leads_before = Vec::new();
    let mut delivered_in_period = 0;
    let mut rounds = Vec::new();
    for mr in app.mrs.iter().filter(|mr| !mr.title.is_empty()) {
        let Some(path) = path_of(app, mr, &last) else {
            continue;
        };
        // Opened after the period: not in it.
        if path.created > now {
            continue;
        }
        if let Some(merged) = path.merged.filter(|m| *m <= now) {
            to_merge.push(board::days_between(path.created, merged).max(0.0));
        }
        if mr.state == GitlabMrState::Opened {
            let column = if mr.mergeability == MergeabilityStatus::Draft {
                Column::Draft
            } else if !mr.approved_by.is_empty() || path.approved.is_some() {
                Column::Approved
            } else if path.reviewed.is_some() {
                Column::InReview
            } else {
                Column::Waiting
            };
            if let Some(n) = path.rounds.filter(|n| *n >= 2) {
                d.rounds
                    .ping_pong
                    .push((mr.id.clone(), mr.title.clone(), n));
            }
            d.open.push(Aging {
                id: mr.id.clone(),
                title: mr.title.clone(),
                author: mr.author.clone(),
                column,
                age: board::days_between(path.created, now).max(0.0),
                rounds: path.rounds,
            });
            d.open_paths.push(path);
            continue;
        }
        // DORA's lead time: from the first commit to production.
        if let Some(delivered) = path.delivered.filter(|t| *t <= now) {
            let lead = board::days_between(path.first_commit.unwrap_or(path.created), delivered);
            if delivered >= start {
                leads.push(lead.max(0.0));
                delivered_in_period += 1;
                if is_revert(&mr.title) {
                    d.dora.reverts.push(mr.id.clone());
                } else if is_hotfix(app, mr) {
                    d.dora.hotfixes.push(mr.id.clone());
                }
            } else if delivered >= before {
                leads_before.push(lead.max(0.0));
            }
        }
        if path.merged.is_some_and(|m| m >= start && m <= now) {
            if let (Some(n), true) = (path.rounds, path.reviewed.is_some()) {
                rounds.push(n);
            }
            if mr.review.is_some() {
                if path.never_draft {
                    d.never_draft += 1;
                } else {
                    d.drafted += 1;
                    d.reviewed_in_draft += usize::from(path.draft_comments > 0);
                }
            }
            d.merged.push(path);
        }
    }
    d.merged
        .sort_by(|a, b| b.merged.cmp(&a.merged).then(a.id.cmp(&b.id)));
    d.open.sort_by(|a, b| b.age.total_cmp(&a.age));
    d.rounds
        .ping_pong
        .sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    d.p50 = percentile(to_merge.clone(), 0.5);
    d.p85 = percentile(to_merge, 0.85);
    for (i, stage) in d.medians.iter_mut().enumerate().take(4) {
        *stage =
            crate::next_step::median(d.merged.iter().filter_map(|p| p.stages(now)[i]).collect());
    }
    // A merged MR not on production yet counts in no step-5 median: its
    // wait is not over.
    d.medians[4] = crate::next_step::median(
        d.merged
            .iter()
            .filter(|p| p.delivered.is_some())
            .filter_map(|p| p.stages(now)[4])
            .collect(),
    );

    // Review rounds.
    d.rounds.reviewed = rounds.len();
    d.rounds.first_time = rounds.iter().filter(|n| **n == 0).count();
    for n in &rounds {
        d.rounds.histogram[(*n as usize).min(3)] += 1;
    }
    d.rounds.median = crate::next_step::median(rounds.iter().map(|n| f64::from(*n)).collect());

    // DORA.
    let deployments = production_deployments(overview, &last);
    let dora = &mut d.dora;
    dora.has_deployments = !deployments.is_empty();
    let at = |dep: &DeploymentRecord| board::parse_time(&dep.created_at);
    let in_period = |dep: &&DeploymentRecord| at(dep).is_some_and(|t| t >= start && t <= now);
    let in_before = |dep: &&DeploymentRecord| at(dep).is_some_and(|t| t >= before && t < start);
    dora.deploys = deployments
        .iter()
        .filter(|d| in_period(d))
        .filter(|d| d.status == "success")
        .count();
    dora.deploys_before = deployments
        .iter()
        .filter(|d| in_before(d))
        .filter(|d| d.status == "success")
        .count();
    dora.failed_deploys = deployments
        .iter()
        .filter(|d| in_period(d))
        .filter(|d| d.status == "failed")
        .count();
    // Without the deployments, an MR on production is one deployment.
    let shipped = if dora.has_deployments {
        dora.deploys
    } else {
        delivered_in_period
    };
    dora.per_week = shipped as f64 * 7.0 / days as f64;
    dora.shipped = shipped;
    dora.leads = leads.len();
    dora.lead = crate::next_step::median(leads);
    dora.lead_before = crate::next_step::median(leads_before);
    dora.changes = if dora.has_deployments {
        dora.deploys + dora.failed_deploys
    } else {
        delivered_in_period
    };
    let failures = dora.failed_deploys + dora.reverts.len() + dora.hotfixes.len();
    dora.failure_rate =
        (dora.changes > 0).then(|| (failures as f64 / dora.changes as f64).min(1.0));
    // Time to restore: a failed deployment until the next good one of the
    // same environment; a reverted MR's arrival until its revert's.
    let mut restores = Vec::new();
    for (i, dep) in deployments.iter().enumerate() {
        if dep.status != "failed" || !in_period(dep) {
            continue;
        }
        let Some(failed_at) = at(dep) else {
            continue;
        };
        let fixed = deployments[i + 1..]
            .iter()
            .find(|next| next.environment == dep.environment && next.status == "success")
            .and_then(|next| at(next));
        if let Some(fixed) = fixed {
            restores.push(board::days_between(failed_at, fixed).max(0.0));
        }
    }
    for id in &dora.reverts {
        let Some(revert) = app.mrs.iter().find(|m| &m.id == id) else {
            continue;
        };
        let original = reverted(app, revert).map(|(m, _)| m);
        let (Some(original), Some(fixed)) = (
            original.and_then(|m| path_of(app, m, &last)),
            path_of(app, revert, &last).and_then(|p| p.delivered.or(p.merged)),
        ) else {
            continue;
        };
        if let Some(broke) = original.delivered.or(original.merged) {
            restores.push(board::days_between(broke, fixed).max(0.0));
        }
    }
    dora.restores = restores.len();
    dora.restore = crate::next_step::median(restores);
    let counted: Vec<String> = dora.reverts.iter().chain(&dora.hotfixes).cloned().collect();
    d.undos = undos(app, &ordered, start, &counted);
    d
}

/// The step that takes the most of the median path, and its share.
pub fn slowest(medians: &[Option<f64>; 5]) -> Option<(Stage, f64)> {
    let total: f64 = medians.iter().flatten().sum();
    if total <= 0.0 {
        return None;
    }
    medians
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.map(|m| (Stage::ALL[i], m)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, m)| (s, m / total))
}

// ── Against the period before ────────────────────────────────────────────────

/// Below this many on either side, a change says nothing.
pub const MIN_SAMPLE: usize = 5;

/// A figure against the period before.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Versus {
    /// The same, to the precision shown.
    Same,
    /// Up or down (`up`), and whether that is better.
    Moved { up: bool, better: bool },
    /// Moved, on too few to tell.
    TooFew { up: bool },
}

/// `now` against `before`, each from `n` and `n_before` cases; `better_up`:
/// whether a rise is good news. Within `same` of each other they are the same.
pub fn versus(
    now: f64,
    before: f64,
    (n, n_before): (usize, usize),
    better_up: bool,
    same: f64,
) -> Versus {
    if (now - before).abs() <= same {
        return Versus::Same;
    }
    let up = now > before;
    if n < MIN_SAMPLE || n_before < MIN_SAMPLE {
        Versus::TooFew { up }
    } else {
        Versus::Moved {
            up,
            better: up == better_up,
        }
    }
}

// ── The page ─────────────────────────────────────────────────────────────────
