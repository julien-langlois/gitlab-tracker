//! The delivery page: how fast changes reach production and where the time
//! goes on the way. The four DORA measures (deployment frequency, lead time
//! for changes, change failure rate, time to restore), each merged MR's path
//! cut into its steps (in development, waiting for a review, in review, waiting
//! for the merge, waiting for production), the rounds a review sends back to
//! the author, and the open MRs placed by step and age against the usual time
//! to merge. Everything is read from what the tracked MRs already carry, plus
//! the project's deployments (the environments page's fetch). The weekly
//! report copies it all as Markdown.

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::board::{self, Figure, GAP};
use super::jump::{Entry, Jumps};
use super::rail_badges::{self, Badge};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{button, muted, primary_button, split_person};
use super::GuiApp;
use crate::app::App;
use crate::gitlab_detail::{DeploymentRecord, ProjectOverview};
use crate::i18n::{tf, tl, tn};
use crate::models::{GitlabMrState, TrackedMr};
use gitlab_tracker_core::MergeabilityStatus;

/// The periods offered, in days.
const PERIODS: [(i64, &str); 3] = [(7, "7 d"), (30, "30 d"), (90, "90 d")];

/// The delivery page's choices.
#[derive(Debug, Clone)]
pub struct State {
    /// Index in [`PERIODS`].
    pub period: usize,
    /// The MR picked, by id.
    pub selected: Option<String>,
    /// The shortcuts to the page's sections.
    pub jumps: Jumps,
    /// Each figure against the period before, of the same length.
    pub compare: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            period: 1,
            selected: None,
            jumps: Jumps::default(),
            compare: true,
        }
    }
}

// ── The steps of an MR's path ────────────────────────────────────────────────

/// A step of an MR's way to production, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Opened, not up for review yet: still being written. A draft until it
    /// is marked ready; an MR never in draft, until its first review request.
    Development,
    /// Review asked, nobody reviewed yet.
    WaitingReview,
    /// Reviewed, not approved yet: the back and forth.
    InReview,
    /// Approved (or reviewed), not merged yet.
    ToMerge,
    /// Merged, not on production yet.
    ToProduction,
}

impl Stage {
    pub const ALL: [Stage; 5] = [
        Stage::Development,
        Stage::WaitingReview,
        Stage::InReview,
        Stage::ToMerge,
        Stage::ToProduction,
    ];

    pub fn label(self) -> &'static str {
        tl(match self {
            Stage::Development => "In development",
            Stage::WaitingReview => "Waiting for a review",
            Stage::InReview => "In review",
            Stage::ToMerge => "Waiting for the merge",
            Stage::ToProduction => "Waiting for production",
        })
    }

    pub fn hint(self) -> &'static str {
        tl(match self {
            Stage::Development => "From the opening to the move out of draft (marked ready). An MR never in draft: to its first review request. Comments while a draft are the review along the way and stay in this step.",
            Stage::WaitingReview => "From ready for review to the first review by someone else (bots left out).",
            Stage::InReview => "From the first review to the approval: the back and forth.",
            Stage::ToMerge => "From the approval to the merge.",
            Stage::ToProduction => "From the merge to the arrival on the last branch.",
        })
    }

    /// Its colour, by who has the MR: the team at work in cool hues (teal
    /// writing, blue reviewing), the waits in warm ones (amber for a review,
    /// orange for the merge), and purple after the merge, the release's time.
    /// Cool and warm alternate along the path, so no two neighbours look
    /// alike.
    pub fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Stage::Development => t.teal,
            Stage::WaitingReview => t.amber,
            Stage::InReview => t.blue,
            Stage::ToMerge => t.orange,
            Stage::ToProduction => t.purple,
        }
    }

    /// A wait: nobody works on the MR, the time to cut first.
    pub fn is_wait(self) -> bool {
        matches!(
            self,
            Stage::WaitingReview | Stage::ToMerge | Stage::ToProduction
        )
    }
}

/// When an MR went through each step.
#[derive(Debug, Clone)]
pub struct Path {
    pub id: String,
    pub title: String,
    pub author: String,
    pub created: DateTime<Utc>,
    /// Put up for review: out of draft, else the first review request.
    pub ready: Option<DateTime<Utc>>,
    /// Never a draft: its development cannot be told from its wait for a
    /// review when its reviewers were asked at the opening.
    pub never_draft: bool,
    /// Comments by others while a draft: the review along the way.
    pub draft_comments: u32,
    pub reviewed: Option<DateTime<Utc>>,
    pub approved: Option<DateTime<Utc>>,
    pub merged: Option<DateTime<Utc>>,
    /// On the last branch of the flow.
    pub delivered: Option<DateTime<Utc>>,
    /// Its oldest commit: where DORA starts the lead time.
    pub first_commit: Option<DateTime<Utc>>,
    /// Pushes after the first review; `None` without review data.
    pub rounds: Option<u32>,
}

impl Path {
    /// Days spent in each step; `None` for a step it skipped (its time goes to
    /// the next one it went through) or has not reached. An MR not merged yet
    /// counts its current step up to `now`.
    pub fn stages(&self, now: DateTime<Utc>) -> [Option<f64>; 5] {
        let ends = [
            self.ready,
            self.reviewed,
            self.approved,
            self.merged,
            self.delivered,
        ];
        let mut out = [None; 5];
        let mut last = self.created;
        for (i, end) in ends.iter().enumerate() {
            if let Some(end) = end {
                out[i] = Some(board::days_between(last, *end).max(0.0));
                last = last.max(*end);
            }
        }
        // The step it is in now, open-ended.
        let reached = ends.iter().rposition(Option::is_some);
        let current = match (self.merged, self.delivered) {
            (Some(_), None) => Some(4),
            (None, _) => Some(reached.map_or(0, |r| r + 1).min(3)),
            _ => None,
        };
        if let Some(i) = current {
            if out[i].is_none() {
                out[i] = Some(board::days_between(last, now).max(0.0));
            }
        }
        out
    }

    /// From the opening to production (else to the merge), in days.
    pub fn total(&self) -> Option<f64> {
        let end = self.delivered.or(self.merged)?;
        Some(board::days_between(self.created, end).max(0.0))
    }
}

/// The step an open MR is in, as the aging chart places it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Draft,
    Waiting,
    InReview,
    Approved,
}

impl Column {
    pub const ALL: [Column; 4] = [
        Column::Draft,
        Column::Waiting,
        Column::InReview,
        Column::Approved,
    ];

    fn label(self) -> &'static str {
        tl(match self {
            Column::Draft => "Draft",
            Column::Waiting => "Waiting for a review",
            Column::InReview => "In review",
            Column::Approved => "Approved",
        })
    }
}

/// An open MR on the aging chart.
#[derive(Debug, Clone)]
pub struct Aging {
    pub id: String,
    pub title: String,
    pub author: String,
    pub column: Column,
    /// Days since it opened.
    pub age: f64,
    pub rounds: Option<u32>,
}

/// How a DORA measure compares with the industry's bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Elite,
    High,
    Medium,
    Low,
}

impl Level {
    pub fn label(self) -> &'static str {
        tl(match self {
            Level::Elite => "Elite",
            Level::High => "High",
            Level::Medium => "Medium",
            Level::Low => "Low",
        })
    }

    pub fn tone(self, theme: &Theme) -> Tone {
        let t = &theme.tones;
        match self {
            Level::Elite => t.green,
            Level::High => t.teal,
            Level::Medium => t.amber,
            Level::Low => t.red,
        }
    }

    /// Deployments a week: daily or more, weekly, monthly, less.
    pub fn of_frequency(per_week: f64) -> Self {
        if per_week >= 7.0 {
            Level::Elite
        } else if per_week >= 1.0 {
            Level::High
        } else if per_week >= 0.25 {
            Level::Medium
        } else {
            Level::Low
        }
    }

    /// Days from commit to production: under a day, a week, a month.
    pub fn of_lead(days: f64) -> Self {
        if days < 1.0 {
            Level::Elite
        } else if days < 7.0 {
            Level::High
        } else if days < 30.0 {
            Level::Medium
        } else {
            Level::Low
        }
    }

    /// Share of the changes that failed: 5, 10, 15 %.
    pub fn of_failure_rate(rate: f64) -> Self {
        if rate <= 0.05 {
            Level::Elite
        } else if rate <= 0.10 {
            Level::High
        } else if rate <= 0.15 {
            Level::Medium
        } else {
            Level::Low
        }
    }

    /// Days to restore: under an hour, a day, a week.
    pub fn of_restore(days: f64) -> Self {
        if days < 1.0 / 24.0 {
            Level::Elite
        } else if days < 1.0 {
            Level::High
        } else if days < 7.0 {
            Level::Medium
        } else {
            Level::Low
        }
    }
}

/// The four DORA measures over the period.
#[derive(Debug, Clone, Default)]
pub struct Dora {
    /// The project's deployments are known (read, and some to production).
    pub has_deployments: bool,
    /// Successful deployments to production, in the period and the one before.
    pub deploys: usize,
    pub deploys_before: usize,
    pub per_week: f64,
    /// What reached production in the period: the deployments, else the
    /// MRs that got there (the frequency's sample).
    pub shipped: usize,
    /// Median days from the first commit to production, over `leads` MRs.
    pub lead: Option<f64>,
    pub leads: usize,
    pub lead_before: Option<f64>,
    /// Failures: failed deployments, reverts, hotfixes; over the changes.
    pub failed_deploys: usize,
    pub reverts: Vec<String>,
    pub hotfixes: Vec<String>,
    pub changes: usize,
    pub failure_rate: Option<f64>,
    /// Median days from a failure to its fix: the next good deployment, or
    /// the revert's merge after the reverted MR's.
    pub restore: Option<f64>,
    pub restores: usize,
}

/// Review rounds of the MRs merged in the period.
#[derive(Debug, Clone, Default)]
pub struct Rounds {
    /// Merged MRs with a review.
    pub reviewed: usize,
    /// Of them, approved with no push after the first review.
    pub first_time: usize,
    /// MRs with 0, 1, 2 and 3 or more rounds.
    pub histogram: [usize; 4],
    pub median: Option<f64>,
    /// Open MRs with two rounds or more, the most first: (id, title, rounds).
    pub ping_pong: Vec<(String, String, u32)>,
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Delivery {
    /// The last branch of the flow.
    pub production: String,
    /// MRs merged in the period, newest first.
    pub merged: Vec<Path>,
    /// Median days of each step over them.
    pub medians: [Option<f64>; 5],
    pub open: Vec<Aging>,
    /// Paths of the open MRs, for the inspector.
    pub open_paths: Vec<Path>,
    /// Days to merge of the merged MRs (all tracked): half and 85 % of them
    /// merged within.
    pub p50: Option<f64>,
    pub p85: Option<f64>,
    pub dora: Dora,
    pub rounds: Rounds,
    /// The reverts and hotfixes merged in the period, newest first.
    pub undos: Vec<Undo>,
    /// Of the merged MRs: opened as (or made) a draft, and of them commented
    /// on while a draft (reviewed along the way).
    pub drafted: usize,
    pub reviewed_in_draft: usize,
    /// Of the merged MRs, with a review timeline: never a draft. Their
    /// development ends at the first review request, at the opening when the
    /// reviewers were asked at once.
    pub never_draft: usize,
}

fn percentile(mut values: Vec<f64>, p: f64) -> Option<f64> {
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
fn rounds_of(mr: &TrackedMr) -> Option<u32> {
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
            .then(|| super::milestones::arrived(app, mr, last))
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
fn is_revert(title: &str) -> bool {
    title.trim_start().to_lowercase().starts_with("revert")
}

/// Whether an MR is a fix rushed to production: a hotfix label, or a source
/// branch among the project's hotfix branches (`hotfix/*` unless set).
fn is_hotfix(app: &App, mr: &TrackedMr) -> bool {
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
fn mr_number(text: &str) -> Option<&str> {
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
const HOTFIX_LOOKBACK_DAYS: i64 = 14;

/// A guess at what `hotfix` patches: of the MRs merged in the two weeks before
/// it (no revert or hotfix), the one sharing the most files with it, the
/// latest on a tie.
fn patched<'a>(app: &'a App, hotfix: &TrackedMr, merged: DateTime<Utc>) -> Option<&'a TrackedMr> {
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
fn furthest(app: &App, mr: &TrackedMr, ordered: &[String]) -> Option<String> {
    ordered
        .iter()
        .rev()
        .find(|b| app.branch_presence(mr, b).is_some_and(|p| !p.is_missing()))
        .cloned()
}

/// The reverts and hotfixes merged since `start`, newest first.
fn undos(app: &App, ordered: &[String], start: DateTime<Utc>, counted: &[String]) -> Vec<Undo> {
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
fn production_deployments<'a>(
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
    let (last, ordered) = super::milestones::flow_of(app);
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

pub fn screen(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let now = Utc::now();
    let days = PERIODS[g.delivery.period.min(PERIODS.len() - 1)].0;
    let ctx = ui.ctx().clone();
    let overview = g.details.project(&g.app, &ctx).cloned();
    let known = overview.is_some();
    let d = g.memo.delivery.get(&g.app.project_id, (days, known), || {
        analyze(&g.app, overview.as_ref(), now, days)
    });
    let before = g.delivery.compare.then(|| {
        g.memo
            .delivery_before
            .get(&g.app.project_id, (days, known), || {
                analyze(&g.app, overview.as_ref(), now - Duration::days(days), days)
            })
    });
    let before = before.as_deref();
    let period = Period { days, now };
    let entries = vec![
        Entry::new("undos", tl("Reverts and hotfixes")).count(d.undos.len()),
        Entry::new("steps", tl("Where the time goes")),
        Entry::new("rounds", tl("Review rounds")),
        Entry::new("aging", tl("Aging work in progress")),
        Entry::new("paths", tl("Merged in the period")).count(d.merged.len()),
    ];
    board::screen_with_jumps(
        g,
        ui,
        sizes,
        "delivery",
        Some((|g| &mut g.delivery.jumps, entries)),
        |g, ui| nav(g, ui, &d, before, period),
        |g, ui| inspector(g, ui, &d, now),
        |g, ui, jumps| main(g, ui, jumps, &d, before, period, overview.as_ref()),
    );
}

/// The period shown: its length, and its end.
#[derive(Debug, Clone, Copy)]
struct Period {
    days: i64,
    now: DateTime<Utc>,
}

impl Period {
    /// "the 30 d before".
    fn before_label(self) -> String {
        tf(
            "the {period} before",
            &[("period", &tf("{n} d", &[("n", &self.days.to_string())]))],
        )
    }
}

/// "▲ from 1.2 / week · better", in green, orange, or grey when too few or
/// the same.
fn versus_note(theme: &Theme, v: Versus, before: &str, period: Period) -> (String, Color32) {
    let arrow = |up: bool| if up { "▲" } else { "▼" };
    let from = tf("from {before}", &[("before", before)]);
    match v {
        Versus::Same => (
            tf("= as {period}", &[("period", &period.before_label())]),
            theme.palette.text_muted,
        ),
        Versus::Moved { up, better } => (
            format!(
                "{} {from} · {}",
                arrow(up),
                if better { tl("better") } else { tl("worse") }
            ),
            if better {
                theme.tones.green.fg
            } else {
                theme.tones.orange.fg
            },
        ),
        Versus::TooFew { up } => (
            format!("{} {from} · {}", arrow(up), tl("too few to tell")),
            theme.palette.text_muted,
        ),
    }
}

fn nav(g: &mut GuiApp, ui: &mut Ui, d: &Delivery, before: Option<&Delivery>, period: Period) {
    let theme = g.theme;
    board::nav_header(
        ui,
        &theme,
        tl("Delivery"),
        &tn(
            d.merged.len() as u64,
            "{n} MR merged in the period",
            "{n} MRs merged in the period",
        ),
    );
    board::nav_group(ui, &theme, tl("Period"), |ui| {
        let labels: Vec<&str> = PERIODS.iter().map(|p| tl(p.1)).collect();
        if let Some(i) = board::segmented(ui, &theme, "delivery_period", &labels, g.delivery.period)
        {
            g.delivery.period = i;
        }
    });
    board::nav_group(ui, &theme, tl("Compare with"), |ui| {
        let labels = [period.before_label(), tl("Off").to_string()];
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        if let Some(i) = board::segmented(
            ui,
            &theme,
            "delivery_compare",
            &labels,
            usize::from(!g.delivery.compare),
        ) {
            g.delivery.compare = i == 0;
        }
        if let Some(before) = before {
            let day =
                |t: DateTime<Utc>| t.with_timezone(&chrono::Local).format("%d/%m").to_string();
            let start = period.now - Duration::days(period.days);
            let earlier = start - Duration::days(period.days);
            ui.add_space(6.0);
            ui.label(
                muted(
                    &theme,
                    tf(
                        "{from} – {to} against {from_before} – {to_before}",
                        &[
                            ("from", &day(start)),
                            ("to", &day(period.now)),
                            ("from_before", &day(earlier)),
                            ("to_before", &day(start)),
                        ],
                    ),
                )
                .size(theme.text_size - 1.5),
            );
            ui.label(
                muted(
                    &theme,
                    tn(
                        before.merged.len() as u64,
                        "{n} MR merged then",
                        "{n} MRs merged then",
                    ),
                )
                .size(theme.text_size - 1.5),
            );
            ui.add_space(6.0);
            let size = theme.text_size - 1.5;
            ui.label(muted(&theme, tl("▲ ▼ the way the figure went")).size(size));
            ui.label(
                RichText::new(tl("green: better"))
                    .size(size)
                    .color(theme.tones.green.fg),
            );
            ui.label(
                RichText::new(tl("orange: worse"))
                    .size(size)
                    .color(theme.tones.orange.fg),
            );
            ui.label(
                muted(
                    &theme,
                    tf(
                        "grey: too few to tell (under {n} on either side)",
                        &[("n", &MIN_SAMPLE.to_string())],
                    ),
                )
                .size(size),
            );
        }
    });
    board::nav_group(ui, &theme, tl("Steps"), |ui| {
        for stage in Stage::ALL {
            let value = d.medians[stage as usize]
                .map(board::duration_text)
                .unwrap_or_else(|| board::dash().to_string());
            board::nav_item(
                ui,
                &theme,
                stage.label(),
                &value,
                Some(stage.tone(&theme).fg),
                false,
            )
            .rich(Tip::new().title(stage.label()).text(stage.hint()));
        }
    });
    board::nav_note(
        ui,
        &theme,
        &tf(
            "Production is {branch}, the last tracked branch. The steps come from the review timeline the discussions give; the deployments from the project's deployments to it.",
            &[("branch", &d.production)],
        ),
    );
}

fn main(
    g: &mut GuiApp,
    ui: &mut Ui,
    jumps: &mut Jumps,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
    overview: Option<&ProjectOverview>,
) {
    let now = period.now;
    let theme = g.theme;
    let mut copy = false;
    board::heading(
        ui,
        &theme,
        tl("Delivery"),
        tl("How fast changes reach production, and where the time goes"),
        |ui| {
            copy = primary_button(ui, &theme, tl("Copy the weekly report")).clicked();
        },
    );
    if copy {
        let ci = super::ci::analyze(&g.app, now, 7);
        ui.ctx()
            .copy_text(board::Links::of(&g.app).apply(&weekly_report(&g.app, overview, &ci, now)));
        g.app.flash(
            tl("Weekly report copied: paste it in a channel or a wiki page"),
            false,
        );
    }
    ui.add_space(14.0);
    dora_figures(ui, &theme, d, before, period);

    let mut picked = None;
    let selected = g.delivery.selected.as_deref();
    ui.add_space(GAP);
    jumps.section(ui, "undos", |ui| {
        undo_card(
            ui,
            &theme,
            d,
            &g.app.config.hotfix_branches,
            selected,
            &mut picked,
        );
    });
    ui.add_space(GAP);
    // Side by side, or one under the other: each its own section.
    let cell = std::cell::RefCell::new(&mut *jumps);
    board::split(
        ui,
        0.6,
        360.0,
        260.0,
        |ui| {
            cell.borrow_mut()
                .section(ui, "steps", |ui| steps_card(ui, &theme, d, before, period));
        },
        |ui| {
            cell.borrow_mut().section(ui, "rounds", |ui| {
                rounds_card(ui, &theme, d, before, period, &mut picked)
            });
        },
    );
    ui.add_space(GAP);
    jumps.section(ui, "aging", |ui| {
        aging_card(ui, &theme, d, selected, &mut picked);
    });
    ui.add_space(GAP);
    jumps.section(ui, "paths", |ui| {
        paths_card(ui, &theme, d, selected, now, &mut picked);
    });
    if let Some(id) = picked {
        g.delivery.selected = Some(id);
    }
}

/// The reverts and hotfixes merged in the period: what each undoes or patches,
/// how long after, and how far the faulty MR had gone.
fn undo_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    hotfix_branches: &str,
    selected: Option<&str>,
    picked: &mut Option<String>,
) {
    let counted = d.undos.iter().filter(|u| u.counted).count();
    let note = if d.undos.is_empty() {
        tl("what had to be undone or patched").to_string()
    } else {
        let note = tn(
            counted as u64,
            "{n} counted in the change failure rate",
            "{n} counted in the change failure rate",
        );
        match d.dora.failed_deploys {
            0 => note,
            n => format!(
                "{note} · {}",
                tn(
                    n as u64,
                    "with {n} failed deployment",
                    "with {n} failed deployments"
                )
            ),
        }
    };
    board::card(ui, theme, tl("Reverts and hotfixes"), &note, 0.0, |ui| {
        if d.undos.is_empty() {
            board::empty(ui, theme, tl("No revert or hotfix merged in the period."));
        }
        let font = FontId::proportional(theme.text_size - 1.0);
        let small = FontId::proportional(theme.text_size - 2.0);
        // The columns, right to left; the same in the heading and the rows.
        let (reached_w, after_w, undoes_w) = (120.0, 70.0, 170.0);
        if !d.undos.is_empty() {
            let (head, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
            let right = head.right() - 8.0;
            let y = head.center().y;
            let caption = |ui: &Ui, x: f32, align: Align2, text: &str, width: f32| {
                board::paint_text(
                    ui,
                    egui::pos2(x, y),
                    align,
                    &text.to_uppercase(),
                    FontId::proportional(theme.text_size - 3.0),
                    theme.palette.text_muted,
                    width,
                );
            };
            caption(ui, right, Align2::RIGHT_CENTER, tl("How far"), reached_w);
            caption(
                ui,
                right - reached_w - 8.0,
                Align2::RIGHT_CENTER,
                tl("After"),
                after_w,
            );
            caption(
                ui,
                right - reached_w - after_w - 16.0 - undoes_w,
                Align2::LEFT_CENTER,
                tl("Undoes"),
                undoes_w,
            );
        }
        for undo in d.undos.iter().take(12) {
            let (rect, response) = board::row(ui, theme, 34.0, selected == Some(undo.id.as_str()));
            let y = rect.center().y;
            let (kind, tone) = match undo.kind {
                UndoKind::Revert => (tl("Revert"), theme.tones.red),
                UndoKind::Hotfix => (tl("Hotfix"), theme.tones.amber),
            };
            // The kind, in a column of its own so the ids line up.
            board::paint_tag(
                ui,
                theme,
                egui::pos2(rect.left() + 6.0, y),
                &kind.to_uppercase(),
                tone,
                false,
            );
            let mut x = rect.left() + 84.0;
            x += board::paint_tag(
                ui,
                theme,
                egui::pos2(x, y),
                &format!("!{}", undo.id),
                board::id_tone(theme),
                true,
            ) + 10.0;
            // Right to left: where the faulty MR got, after how long, what it was.
            let right = rect.right() - 8.0;
            // On production, users saw it: in red.
            let (reached, reached_color) = match &undo.reached {
                Some(branch) if *branch == d.production => (branch.clone(), theme.tones.red.fg),
                Some(branch) => (branch.clone(), theme.palette.text_secondary),
                None => (board::dash().to_string(), theme.palette.text_muted),
            };
            board::paint_text(
                ui,
                egui::pos2(right, y),
                Align2::RIGHT_CENTER,
                &reached,
                small.clone(),
                reached_color,
                reached_w,
            );
            let after = undo
                .after
                .map_or_else(|| board::dash().to_string(), board::duration_text);
            board::paint_text(
                ui,
                egui::pos2(right - reached_w - 8.0, y),
                Align2::RIGHT_CENTER,
                &after,
                font.clone(),
                theme.palette.text_secondary,
                after_w,
            );
            let undoes_left = right - reached_w - after_w - 16.0 - undoes_w;
            let mut undone_click = None;
            match &undo.undoes {
                Some((id, title, found)) => {
                    let w = board::paint_tag(
                        ui,
                        theme,
                        egui::pos2(undoes_left, y),
                        &format!("!{id}"),
                        board::id_tone(theme),
                        true,
                    );
                    let tag =
                        Rect::from_min_size(egui::pos2(undoes_left, y - 10.0), Vec2::new(w, 20.0));
                    let hint = match found {
                        Found::Description => tl("named in its description"),
                        Found::Commit => tl("one of its commits is reverted"),
                        Found::Title => tl("its title is quoted"),
                        Found::SameFiles => tl(
                            "a guess: the latest MR merged before it that changed the same files",
                        ),
                    };
                    let r = ui
                        .interact(
                            tag,
                            ui.make_persistent_id(("undoes", &undo.id)),
                            Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .rich(Tip::new().mr(id, title).text(hint));
                    if r.clicked() {
                        undone_click = Some(id.clone());
                    }
                    if *found == Found::SameFiles {
                        board::paint_text(
                            ui,
                            egui::pos2(undoes_left + w + 6.0, y),
                            Align2::LEFT_CENTER,
                            tl("guess"),
                            small.clone(),
                            theme.palette.text_muted,
                            undoes_w - w - 6.0,
                        );
                    }
                }
                None => {
                    board::paint_text(
                        ui,
                        egui::pos2(undoes_left, y),
                        Align2::LEFT_CENTER,
                        match undo.kind {
                            UndoKind::Revert => tl("not found"),
                            UndoKind::Hotfix => tl("no guess"),
                        },
                        small.clone(),
                        theme.palette.text_muted,
                        undoes_w,
                    );
                }
            }
            board::paint_text(
                ui,
                egui::pos2(x, y),
                Align2::LEFT_CENTER,
                &undo.title,
                font.clone(),
                theme.palette.text,
                (undoes_left - x - 12.0).max(0.0),
            );
            if undone_click.is_some() {
                *picked = undone_click;
            } else if response.clicked() {
                *picked = Some(undo.id.clone());
            }
        }
        ui.add_space(6.0);
        let patterns = if hotfix_branches.trim().is_empty() {
            tl("Hotfixes: the MRs labelled hotfix (no hotfix branch set).").to_string()
        } else {
            tf(
                "Hotfixes: the MRs labelled hotfix or from {branches}. The project settings change the branches.",
                &[("branches", hotfix_branches.trim())],
            )
        };
        ui.label(muted(theme, patterns).size(theme.text_size - 1.5));
    });
}

/// "+2 on the period before" in green, or worse in red, `better_up` telling
/// which way is better.
fn trend(
    now: f64,
    before: f64,
    better_up: bool,
    text: String,
    theme: &Theme,
) -> (String, Option<Color32>) {
    if (now - before).abs() < f64::EPSILON {
        return (tl("as the period before").to_string(), None);
    }
    let better = (now > before) == better_up;
    (
        text,
        Some(if better {
            theme.tones.green.fg
        } else {
            theme.tones.red.fg
        }),
    )
}

fn dora_figures(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
) {
    let dora = &d.dora;
    // Compared: the period before says it all, under each figure.
    let comparing = before.is_some();
    let level_caption = |level: Option<Level>, detail: String| -> (String, Option<Color32>) {
        match level {
            Some(l) => (
                if detail.is_empty() {
                    l.label().to_string()
                } else {
                    format!("{} · {detail}", l.label())
                },
                Some(l.tone(theme).fg),
            ),
            None => (detail, None),
        }
    };
    let freq = if dora.per_week >= 1.0 || dora.per_week == 0.0 {
        tf("{n} / week", &[("n", &format!("{:.1}", dora.per_week))])
    } else {
        tf(
            "{n} / month",
            &[("n", &format!("{:.1}", dora.per_week * 30.0 / 7.0))],
        )
    };
    let shipped_before = dora.deploys_before;
    let (freq_caption, freq_color) = if dora.has_deployments {
        let delta = dora.deploys as i64 - shipped_before as i64;
        let detail = tn(dora.deploys as u64, "{n} deployment", "{n} deployments");
        if delta == 0 || comparing {
            level_caption(Some(Level::of_frequency(dora.per_week)), detail)
        } else {
            level_caption(
                Some(Level::of_frequency(dora.per_week)),
                format!("{detail} ({delta:+})"),
            )
        }
    } else {
        level_caption(
            (dora.per_week > 0.0).then(|| Level::of_frequency(dora.per_week)),
            tl("MRs reaching production").to_string(),
        )
    };
    let (lead_caption, lead_color) = match (dora.lead, dora.lead_before) {
        (Some(a), Some(b)) if (a - b).abs() >= 0.05 && !comparing => {
            let (text, color) = trend(
                a,
                b,
                false,
                tf("{d} before", &[("d", &board::duration_text(b))]),
                theme,
            );
            (
                format!("{} · {text}", Level::of_lead(a).label()),
                color.or(Some(Level::of_lead(a).tone(theme).fg)),
            )
        }
        (Some(a), _) => level_caption(Some(Level::of_lead(a)), tl("from the first commit").into()),
        _ => (tl("nothing on production in the period").to_string(), None),
    };
    let failures = dora.failed_deploys + dora.reverts.len() + dora.hotfixes.len();
    let (cfr_caption, cfr_color) = match dora.failure_rate {
        Some(rate) => level_caption(
            Some(Level::of_failure_rate(rate)),
            tf(
                "{n} of {total}",
                &[
                    ("n", &failures.to_string()),
                    ("total", &dora.changes.to_string()),
                ],
            ),
        ),
        None => (tl("no change in the period").to_string(), None),
    };
    let (restore_caption, restore_color) = match dora.restore {
        Some(days) => level_caption(
            Some(Level::of_restore(days)),
            tn(dora.restores as u64, "{n} incident", "{n} incidents"),
        ),
        None if failures == 0 => (tl("no failure to recover from").to_string(), None),
        None => (tl("not restored yet").to_string(), Some(theme.tones.red.fg)),
    };
    let figures = [
        Figure {
            label: tl("Deployment frequency").into(),
            value: freq,
            value_color: None,
            caption: freq_caption,
            caption_color: freq_color,
        },
        Figure {
            label: tl("Lead time for changes").into(),
            value: dora
                .lead
                .map_or_else(|| board::dash().to_string(), board::duration_text),
            value_color: None,
            caption: lead_caption,
            caption_color: lead_color,
        },
        Figure {
            label: tl("Change failure rate").into(),
            value: dora
                .failure_rate
                .map_or_else(|| board::dash().to_string(), board::percent),
            value_color: None,
            caption: cfr_caption,
            caption_color: cfr_color,
        },
        Figure {
            label: tl("Time to restore").into(),
            value: dora
                .restore
                .map_or_else(|| board::dash().to_string(), board::duration_text),
            value_color: None,
            caption: restore_caption,
            caption_color: restore_color,
        },
    ];
    let Some(b) = before.map(|b| &b.dora) else {
        board::figures(ui, theme, &figures);
        return;
    };
    let freq_text = |per_week: f64| {
        if per_week >= 1.0 || per_week == 0.0 {
            tf("{n} / week", &[("n", &format!("{per_week:.1}"))])
        } else {
            tf(
                "{n} / month",
                &[("n", &format!("{:.1}", per_week * 30.0 / 7.0))],
            )
        }
    };
    let notes = [
        Some(versus_note(
            theme,
            versus(
                dora.per_week,
                b.per_week,
                (dora.shipped, b.shipped),
                true,
                0.05,
            ),
            &freq_text(b.per_week),
            period,
        )),
        dora.lead.zip(b.lead).map(|(now, then)| {
            versus_note(
                theme,
                versus(now, then, (dora.leads, b.leads), false, 0.05),
                &board::duration_text(then),
                period,
            )
        }),
        dora.failure_rate.zip(b.failure_rate).map(|(now, then)| {
            versus_note(
                theme,
                versus(now, then, (dora.changes, b.changes), false, 0.005),
                &board::percent(then),
                period,
            )
        }),
        dora.restore.zip(b.restore).map(|(now, then)| {
            versus_note(
                theme,
                versus(now, then, (dora.restores, b.restores), false, 0.05),
                &board::duration_text(then),
                period,
            )
        }),
    ];
    // A figure with nothing to compare says so.
    let notes: Vec<Option<(String, Color32)>> = notes
        .into_iter()
        .map(|n| {
            n.or_else(|| {
                Some((
                    tl("nothing to compare before").to_string(),
                    theme.palette.text_muted,
                ))
            })
        })
        .collect();
    board::figures_with(ui, theme, &figures, &notes);
}

/// The median path cut into its steps: a bar, then one line per step.
fn steps_card(ui: &mut Ui, theme: &Theme, d: &Delivery, before: Option<&Delivery>, period: Period) {
    let mut note = tn(
        d.merged.len() as u64,
        "median of {n} merged MR",
        "median of {n} merged MRs",
    );
    if before.is_some() {
        note.push_str(" · ");
        note.push_str(&tf(
            "thin bar: {period}",
            &[("period", &period.before_label())],
        ));
    }
    board::card(ui, theme, tl("Where the time goes"), &note, 268.0, |ui| {
        let total: f64 = d.medians.iter().flatten().sum();
        if total <= 0.0 {
            board::empty(ui, theme, tl("No MR merged in the period."));
            return;
        }
        if let Some((stage, share)) = slowest(&d.medians) {
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
            board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &tf(
                    "Most of the time: {stage}, {share} of the way",
                    &[
                        ("stage", &stage.label().to_lowercase()),
                        ("share", &board::percent(share)),
                    ],
                ),
                fonts::semibold(theme.text_size - 0.5),
                // A wait in its colour, the time to cut; work in the text's.
                if stage.is_wait() {
                    stage.tone(theme).fg
                } else {
                    theme.palette.text
                },
                line.width(),
            );
            ui.add_space(6.0);
        }
        // A step under 1 % of the way would be a sliver: it stays in the
        // lines below.
        let parts: Vec<(f32, Color32)> = Stage::ALL
            .iter()
            .map(|s| {
                let v = d.medians[*s as usize].unwrap_or(0.0);
                let v = if v / total < 0.01 { 0.0 } else { v };
                (v as f32, s.tone(theme).fg)
            })
            .collect();
        board::stacked_bar(ui, theme, &parts, 16.0, 2.0);
        ui.add_space(10.0);
        let then = before.map(|b| b.medians);
        let max = d
            .medians
            .iter()
            .chain(then.iter().flatten())
            .flatten()
            .fold(0.0_f64, |a, b| a.max(*b));
        let samples = (d.merged.len(), before.map_or(0, |b| b.merged.len()));
        for stage in Stage::ALL {
            let value = d.medians[stage as usize];
            let (line, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            let tone = stage.tone(theme);
            ui.painter().circle_filled(
                egui::pos2(line.left() + 4.0, line.center().y),
                4.0,
                tone.fg,
            );
            let label_w = (line.width() * 0.42).min(200.0);
            board::paint_text(
                ui,
                egui::pos2(line.left() + 14.0, line.center().y),
                Align2::LEFT_CENTER,
                stage.label(),
                FontId::proportional(theme.text_size - 0.5),
                theme.palette.text_secondary,
                label_w - 14.0,
            );
            let text = value.map_or_else(|| board::dash().to_string(), board::duration_text);
            let share = value
                .filter(|_| total > 0.0)
                .map(|v| board::percent(v / total))
                .unwrap_or_default();
            let value_w = 92.0;
            board::paint_text(
                ui,
                egui::pos2(line.right(), line.center().y),
                Align2::RIGHT_CENTER,
                &format!("{text}  {share}"),
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text,
                value_w,
            );
            let track = Rect::from_min_max(
                egui::pos2(line.left() + label_w + 6.0, line.center().y - 3.0),
                egui::pos2(line.right() - value_w - 10.0, line.center().y + 3.0),
            );
            if track.width() > 20.0 {
                ui.painter()
                    .rect_filled(track, CornerRadius::same(3), theme.palette.border_soft);
                if let Some(v) = value.filter(|_| max > 0.0) {
                    let mut fill = track;
                    fill.set_width((track.width() * (v / max) as f32).max(3.0));
                    ui.painter()
                        .rect_filled(fill, CornerRadius::same(3), tone.fg);
                }
                // The period before: a thin bar under it.
                if let Some(v) = then.and_then(|m| m[stage as usize]).filter(|_| max > 0.0) {
                    let ghost = Rect::from_min_size(
                        egui::pos2(track.left(), track.bottom() + 3.0),
                        Vec2::new((track.width() * (v / max) as f32).max(2.0), 2.0),
                    );
                    ui.painter().rect_filled(
                        ghost,
                        CornerRadius::same(1),
                        theme.palette.text_muted.gamma_multiply(0.6),
                    );
                }
            }
            let mut tip = Tip::new().title(stage.label()).text(stage.hint());
            if let (Some(now), Some(was)) = (value, then.and_then(|m| m[stage as usize])) {
                // Every step is better shorter.
                let (text, color) = versus_note(
                    theme,
                    versus(now, was, samples, false, now.max(was) * 0.05),
                    &board::duration_text(was),
                    period,
                );
                // The arrow by the value, in its colour.
                if !text.starts_with('=') {
                    board::paint_text(
                        ui,
                        egui::pos2(line.right() - value_w - 2.0, line.center().y),
                        Align2::RIGHT_CENTER,
                        &text[..text.char_indices().nth(1).map_or(text.len(), |(i, _)| i)],
                        FontId::proportional(theme.text_size - 2.0),
                        color,
                        12.0,
                    );
                }
                tip = tip.dot(color, text);
            }
            response.rich(tip);
        }
        drafts_caption(ui, theme, d);
    });
}

/// "3 of 18 opened as drafts, 2 reviewed along the way".
fn drafts_text(d: &Delivery) -> Option<String> {
    (d.drafted > 0).then(|| {
        tn(
            d.drafted as u64,
            "{n} of {total} opened as a draft, {along} reviewed along the way",
            "{n} of {total} opened as drafts, {along} reviewed along the way",
        )
        .replace("{total}", &(d.drafted + d.never_draft).to_string())
        .replace("{along}", &d.reviewed_in_draft.to_string())
    })
}

/// "15 never drafts: development counted up to the review request".
fn never_draft_text(d: &Delivery) -> Option<String> {
    (d.never_draft > 0).then(|| {
        tn(
            d.never_draft as u64,
            "{n} never a draft: development counted up to the review request",
            "{n} never drafts: development counted up to the review request",
        )
    })
}

/// Under the steps: how many MRs were drafts reviewed along the way, and how
/// many never were, whose development cannot be told from their wait.
fn drafts_caption(ui: &mut Ui, theme: &Theme, d: &Delivery) {
    let known = d.drafted + d.never_draft;
    if known == 0 {
        return;
    }
    ui.add_space(6.0);
    if let Some(text) = drafts_text(d) {
        ui.add(egui::Label::new(muted(theme, &text).size(theme.text_size - 1.5)).wrap())
            .rich(Tip::new().title(tl("Drafts")).text(tl(
                "An MR opened as a draft when the work starts is in development until it is marked ready. Comments while a draft are the review along the way: they count here, not as the first review nor as rounds.",
            )));
    }
    if let Some(text) = never_draft_text(d) {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .color(theme.tones.amber.fg)
                    .size(theme.text_size - 1.5),
            )
            .wrap(),
        )
        .rich(Tip::new().title(tl("Never a draft")).text(tl(
            "Nothing in GitLab tells when the work on these MRs ended. Their development ends at the first review request: when the reviewers were asked at the opening, the writing counts as waiting for a review. Open the MRs as drafts and mark them ready to split them.",
        )));
    }
}

fn rounds_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    before: Option<&Delivery>,
    period: Period,
    picked: &mut Option<String>,
) {
    let r = &d.rounds;
    board::card(
        ui,
        theme,
        tl("Review rounds"),
        tl("pushes after the first review"),
        268.0,
        |ui| {
            if r.reviewed == 0 {
                board::empty(ui, theme, tl("No reviewed MR merged in the period."));
                return;
            }
            let share = r.first_time as f64 / r.reviewed as f64;
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
            let tone = if share >= 0.6 {
                theme.tones.green
            } else if share >= 0.35 {
                theme.tones.amber
            } else {
                theme.tones.red
            };
            let w = board::paint_text(
                ui,
                egui::pos2(line.left(), line.center().y),
                Align2::LEFT_CENTER,
                &board::percent(share),
                fonts::semibold(26.0),
                tone.fg,
                line.width() * 0.4,
            );
            board::paint_text(
                ui,
                egui::pos2(line.left() + w + 10.0, line.center().y),
                Align2::LEFT_CENTER,
                &tf(
                    "approved at the first round · median {n}",
                    &[(
                        "n",
                        &r.median
                            .map(|m| format!("{m:.1}").replace(".0", ""))
                            .unwrap_or_default(),
                    )],
                ),
                FontId::proportional(theme.text_size - 1.0),
                theme.palette.text_secondary,
                line.width() - w - 10.0,
            );
            if let Some(b) = before.map(|b| &b.rounds).filter(|b| b.reviewed > 0) {
                let then = b.first_time as f64 / b.reviewed as f64;
                let (text, color) = versus_note(
                    theme,
                    versus(share, then, (r.reviewed, b.reviewed), true, 0.005),
                    &board::percent(then),
                    period,
                );
                ui.label(RichText::new(text).size(theme.text_size - 1.5).color(color));
            }
            ui.add_space(8.0);
            // One column per round count.
            let labels = [tl("0"), tl("1"), tl("2"), tl("3+")];
            let most = *r.histogram.iter().max().unwrap_or(&1).max(&1);
            let (area, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 86.0), Sense::hover());
            let col_w = area.width() / 4.0;
            let t = &theme.tones;
            let colors = [t.green.fg, t.teal.fg, t.amber.fg, t.red.fg];
            for (i, n) in r.histogram.iter().enumerate() {
                let x = area.left() + i as f32 * col_w;
                let bar_h = 56.0 * *n as f32 / most as f32;
                let bar = Rect::from_min_max(
                    egui::pos2(x + col_w * 0.22, area.top() + 14.0 + 56.0 - bar_h.max(2.0)),
                    egui::pos2(x + col_w * 0.78, area.top() + 70.0),
                );
                ui.painter().rect_filled(
                    bar,
                    CornerRadius::same(3),
                    colors[i].gamma_multiply(0.85),
                );
                board::paint_text(
                    ui,
                    egui::pos2(bar.center().x, bar.top() - 7.0),
                    Align2::CENTER_CENTER,
                    &n.to_string(),
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text,
                    col_w,
                );
                board::paint_text(
                    ui,
                    egui::pos2(bar.center().x, area.bottom() - 7.0),
                    Align2::CENTER_CENTER,
                    labels[i],
                    FontId::proportional(theme.text_size - 2.0),
                    theme.palette.text_muted,
                    col_w,
                );
            }
            ui.add_space(6.0);
            if r.ping_pong.is_empty() {
                board::caption(ui, theme, tl("No open MR goes back and forth."));
                return;
            }
            board::caption(ui, theme, tl("Open MRs going back and forth:"));
            ui.add_space(2.0);
            for (id, title, n) in r.ping_pong.iter().take(3) {
                let (rect, response) = board::row(ui, theme, 26.0, false);
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 4.0, rect.center().y),
                    &format!("!{id}"),
                    board::id_tone(theme),
                    true,
                );
                let rounds = tn(u64::from(*n), "{n} round", "{n} rounds");
                let rw = board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 6.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &rounds,
                    FontId::proportional(theme.text_size - 1.5),
                    theme.tones.amber.fg,
                    90.0,
                );
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + w + 12.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    title,
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text,
                    rect.width() - w - rw - 26.0,
                );
                if response.clicked() {
                    *picked = Some(id.clone());
                }
            }
        },
    );
}

/// The tone of an open MR's age against the usual time to merge.
fn age_tone(theme: &Theme, age: f64, p50: Option<f64>, p85: Option<f64>) -> Tone {
    match (p50, p85) {
        (_, Some(p85)) if age >= p85 => theme.tones.red,
        (Some(p50), _) if age >= p50 => theme.tones.amber,
        (Some(_), _) | (_, Some(_)) => theme.tones.green,
        _ => theme.tones.gray,
    }
}

/// The open MRs by step (columns) and age (height), with the P50 and P85 of
/// the time to merge across: a dot above P85 is late before being overdue.
fn aging_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    selected: Option<&str>,
    picked: &mut Option<String>,
) {
    let late = d
        .open
        .iter()
        .filter(|a| d.p85.is_some_and(|p| a.age >= p))
        .count();
    let note = match d.p85 {
        Some(_) => tn(
            late as u64,
            "{n} past 85 % of the merged MRs",
            "{n} past 85 % of the merged MRs",
        ),
        None => tl("no merged MR to compare with yet").to_string(),
    };
    board::card(ui, theme, tl("Aging work in progress"), &note, 0.0, |ui| {
        if d.open.is_empty() {
            board::empty(ui, theme, tl("No open MR."));
            return;
        }
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 292.0), Sense::hover());
        // The column headers on top, then the plot, a margin under it so that
        // the youngest MRs do not sit on the edge.
        let header_h = 30.0;
        let area = Rect::from_min_max(
            egui::pos2(rect.left() + 44.0, rect.top()),
            egui::pos2(rect.right() - 70.0, rect.bottom()),
        );
        let plot = Rect::from_min_max(
            egui::pos2(area.left(), area.top() + header_h + 12.0),
            egui::pos2(area.right(), area.bottom() - 10.0),
        );
        // Up to four times the P85 (a week at least): an MR forgotten for
        // months sits on the top edge instead of flattening the others.
        let oldest = d.open.iter().map(|a| a.age).fold(1.0_f64, f64::max);
        let cap = d.p85.map_or(oldest, |p| (p * 4.0).max(7.0));
        let top_age = oldest.min(cap).max(d.p85.unwrap_or(0.0)).max(1.0) * 1.12;
        let y_of = |age: f64| {
            plot.bottom() - 8.0 - (age.min(top_age) / top_age) as f32 * (plot.height() - 8.0)
        };
        // The columns: a header each (name, count), bands every other one shaded.
        let col_w = plot.width() / Column::ALL.len() as f32;
        for (i, column) in Column::ALL.iter().enumerate() {
            let left = plot.left() + i as f32 * col_w;
            let band = Rect::from_min_max(
                egui::pos2(left, plot.top()),
                egui::pos2(left + col_w, plot.bottom()),
            );
            if i % 2 == 0 {
                ui.painter().rect_filled(
                    band,
                    CornerRadius::same(theme.radii.small),
                    theme.palette.raised.gamma_multiply(0.6),
                );
            }
            let header = Rect::from_min_size(
                egui::pos2(left + 3.0, area.top()),
                Vec2::new(col_w - 6.0, header_h),
            );
            ui.painter().rect(
                header,
                CornerRadius::same(theme.radii.medium),
                theme.palette.raised,
                Stroke::new(1.0, theme.palette.border_soft),
                egui::StrokeKind::Inside,
            );
            let n = d.open.iter().filter(|a| a.column == *column).count();
            let count = n.to_string();
            let count_w = board::one_line(
                ui,
                &count,
                fonts::medium(theme.text_size - 2.5),
                theme.palette.text,
                60.0,
            )
            .size()
            .x + 12.0;
            let name = board::one_line(
                ui,
                column.label(),
                fonts::semibold(theme.text_size - 0.5),
                theme.palette.text,
                header.width() - count_w - 20.0,
            );
            let total_w = name.size().x + 8.0 + count_w;
            let x = header.center().x - total_w / 2.0;
            let name_h = name.size().y;
            ui.painter().galley(
                egui::pos2(x, header.center().y - name_h / 2.0),
                name,
                theme.palette.text,
            );
            board::paint_tag(
                ui,
                theme,
                egui::pos2(x + total_w - count_w, header.center().y),
                &count,
                board::id_tone(theme),
                false,
            );
        }
        // The age scale on the left, on round steps.
        let step = [
            1.0 / 24.0,
            0.25,
            0.5,
            1.0,
            2.0,
            3.0,
            7.0,
            14.0,
            21.0,
            30.0,
            60.0,
            90.0,
            180.0,
            365.0,
        ]
        .into_iter()
        .find(|s| top_age / s <= 4.5)
        .unwrap_or(365.0);
        let mut age = 0.0;
        while age <= top_age {
            let y = y_of(age);
            ui.painter().hline(
                plot.x_range(),
                y,
                Stroke::new(1.0, theme.palette.border_soft.gamma_multiply(0.7)),
            );
            board::paint_text(
                ui,
                egui::pos2(plot.left() - 8.0, y),
                Align2::RIGHT_CENTER,
                &if age == 0.0 {
                    "0".to_string()
                } else {
                    board::duration_text(age)
                },
                FontId::proportional(theme.text_size - 2.5),
                theme.palette.text_muted,
                40.0,
            );
            age += step;
        }
        // The usual time to merge, across.
        let close = match (d.p50, d.p85) {
            (Some(a), Some(b)) => (y_of(a) - y_of(b)).abs() < 14.0,
            _ => false,
        };
        for (p, label, tone) in [
            (d.p50, "P50", theme.tones.amber),
            (d.p85, "P85", theme.tones.red),
        ] {
            let Some(p) = p else {
                continue;
            };
            let y = y_of(p);
            let label_y = if close && label == "P50" {
                y + 8.0
            } else if close {
                y - 8.0
            } else {
                y
            };
            super::milestones::dashed(
                ui,
                egui::pos2(plot.left(), y),
                egui::pos2(plot.right(), y),
                tone.fg.gamma_multiply(0.8),
            );
            board::paint_text(
                ui,
                egui::pos2(plot.right() + 6.0, label_y),
                Align2::LEFT_CENTER,
                &format!("{label} · {}", board::duration_text(p)),
                FontId::proportional(theme.text_size - 2.5),
                tone.fg,
                64.0,
            );
        }
        // The MRs, spread across their column, the oldest first.
        for (i, column) in Column::ALL.iter().enumerate() {
            let list: Vec<&Aging> = d.open.iter().filter(|a| a.column == *column).collect();
            let left = plot.left() + i as f32 * col_w;
            for (k, a) in list.iter().enumerate() {
                let x = left + col_w * (k as f32 + 1.0) / (list.len() as f32 + 1.0);
                let center = egui::pos2(x, y_of(a.age));
                // Off the scale: a mark on the top edge.
                if a.age > top_age {
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            center + Vec2::new(0.0, -14.0),
                            center + Vec2::new(-4.0, -9.0),
                            center + Vec2::new(4.0, -9.0),
                        ],
                        theme.tones.red.fg,
                        Stroke::NONE,
                    ));
                }
                let tone = age_tone(theme, a.age, d.p50, d.p85);
                let is_selected = selected == Some(a.id.as_str());
                let hit = Rect::from_center_size(center, Vec2::splat(16.0));
                let response =
                    ui.interact(hit, ui.scope_id().with(("aging", &a.id)), Sense::click());
                let radius = if response.hovered() || is_selected {
                    7.5
                } else {
                    6.0
                };
                ui.painter().circle(
                    center,
                    radius,
                    tone.fg,
                    Stroke::new(
                        if is_selected { 2.5 } else { 1.5 },
                        if is_selected {
                            theme.palette.text
                        } else {
                            theme.palette.panel
                        },
                    ),
                );
                let rounds = a
                    .rounds
                    .filter(|n| *n > 0)
                    .map(|n| format!(" · {}", tn(u64::from(n), "{n} round", "{n} rounds")))
                    .unwrap_or_default();
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .rich(Tip::new().mr(&a.id, &a.title).text(format!(
                        "{} · {}{rounds}",
                        split_person(&a.author).0,
                        tf("open for {time}", &[("time", &board::duration_text(a.age))]),
                    )));
                if response.clicked() {
                    *picked = Some(a.id.clone());
                }
            }
        }
    });
}

/// The MRs merged in the period, each as its steps end to end, the slowest
/// first.
fn paths_card(
    ui: &mut Ui,
    theme: &Theme,
    d: &Delivery,
    selected: Option<&str>,
    now: DateTime<Utc>,
    picked: &mut Option<String>,
) {
    board::card(
        ui,
        theme,
        tl("Merged in the period"),
        tl("the slowest first"),
        0.0,
        |ui| {
            if d.merged.is_empty() {
                board::empty(ui, theme, tl("No MR merged in the period."));
                return;
            }
            let mut list: Vec<(&Path, [Option<f64>; 5], f64)> = d
                .merged
                .iter()
                .map(|p| {
                    let stages = p.stages(now);
                    let sum = stages.iter().flatten().sum();
                    (p, stages, sum)
                })
                .collect();
            list.sort_by(|a, b| b.2.total_cmp(&a.2));
            let longest = list.first().map_or(1.0, |l| l.2).max(0.01);
            for (path, stages, sum) in list.iter().take(12) {
                let (rect, response) =
                    board::row(ui, theme, 32.0, selected == Some(path.id.as_str()));
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(rect.left() + 6.0, rect.center().y),
                    &format!("!{}", path.id),
                    board::id_tone(theme),
                    true,
                );
                let title_w = (rect.width() * 0.34).min(320.0);
                board::paint_text(
                    ui,
                    egui::pos2(rect.left() + w + 14.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &path.title,
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text,
                    title_w - w - 14.0,
                );
                let total_w = 64.0;
                board::paint_text(
                    ui,
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &board::duration_text(*sum),
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text_secondary,
                    total_w,
                );
                let room = rect.width() - title_w - total_w - 24.0;
                if room > 30.0 {
                    let bar = Rect::from_min_size(
                        egui::pos2(rect.left() + title_w + 8.0, rect.center().y - 5.0),
                        Vec2::new((room * (*sum / longest) as f32).max(4.0), 10.0),
                    );
                    let parts: Vec<(f32, Color32)> = Stage::ALL
                        .iter()
                        .map(|s| (stages[*s as usize].unwrap_or(0.0) as f32, s.tone(theme).fg))
                        .collect();
                    board::paint_stacked_bar(ui, theme, bar, &parts, 1.0);
                }
                if response.clicked() {
                    *picked = Some(path.id.clone());
                }
            }
            ui.add_space(6.0);
            let legend: Vec<(Color32, String)> = Stage::ALL
                .iter()
                .map(|s| (s.tone(theme).fg, s.label().to_string()))
                .collect();
            board::legend(ui, theme, &legend);
        },
    );
}

// ── Inspector ────────────────────────────────────────────────────────────────

fn inspector(g: &mut GuiApp, ui: &mut Ui, d: &Delivery, now: DateTime<Utc>) {
    let theme = g.theme;
    let path = g.delivery.selected.as_deref().and_then(|id| {
        d.merged
            .iter()
            .chain(d.open_paths.iter())
            .find(|p| p.id == id)
    });
    let Some(path) = path else {
        about_dora(ui, &theme);
        return;
    };
    if board::inspector_title(ui, &theme, &format!("!{}", path.id)) {
        g.delivery.selected = None;
        return;
    }
    let mut open = false;
    board::section(ui, &theme, "", "", |ui| {
        ui.add(egui::Label::new(super::widgets::semibold(&path.title, theme.text_size)).wrap());
        ui.add_space(4.0);
        ui.label(muted(&theme, split_person(&path.author).0));
        ui.add_space(8.0);
        open = button(ui, &theme, None, tl("Open the MR"), false).clicked();
    });
    let stages = path.stages(now);
    board::section(ui, &theme, tl("Its way"), "", |ui| {
        let parts: Vec<(f32, Color32)> = Stage::ALL
            .iter()
            .map(|s| (stages[*s as usize].unwrap_or(0.0) as f32, s.tone(&theme).fg))
            .collect();
        board::stacked_bar(ui, &theme, &parts, 10.0, 1.0);
        ui.add_space(8.0);
        let rows: Vec<(&str, String)> = Stage::ALL
            .iter()
            .map(|s| {
                (
                    s.label(),
                    stages[*s as usize]
                        .map_or_else(|| board::dash().to_string(), board::duration_text),
                )
            })
            .collect();
        board::facts(ui, &theme, &rows);
    });
    board::section(ui, &theme, tl("Figures"), "", |ui| {
        let mut rows = vec![(tl("Opened"), board::ago(path.created, now))];
        if let Some(m) = path.merged {
            rows.push((tl("Merged"), board::ago(m, now)));
        }
        if let Some(t) = path.total() {
            rows.push((tl("Opening to production"), board::duration_text(t)));
        }
        rows.push((
            tl("Review rounds"),
            path.rounds
                .map_or_else(|| board::dash().to_string(), |n| n.to_string()),
        ));
        if path.merged.is_none() {
            let age = board::days_between(path.created, now);
            let against = match d.p85 {
                Some(p) if age >= p => tl("older than 85 % of the merged MRs").to_string(),
                Some(_) => match d.p50 {
                    Some(p) if age >= p => tl("older than half the merged MRs").to_string(),
                    _ => tl("within the usual time").to_string(),
                },
                None => board::dash().to_string(),
            };
            rows.push((tl("Against the others"), against));
        }
        board::facts(ui, &theme, &rows);
    });
    if open {
        let id = path.id.clone();
        g.open_mr(&id);
    }
}

fn about_dora(ui: &mut Ui, theme: &Theme) {
    board::section(ui, theme, tl("The DORA measures"), "", |ui| {
        ui.add(
            egui::Label::new(muted(
                theme,
                tl("Four measures of a team's delivery, from the DORA research. Each is placed in one of four bands, from Elite to Low."),
            ))
            .wrap(),
        );
    });
    let rows: [(&str, [&str; 4]); 4] = [
        (
            "Deployment frequency",
            ["daily or more", "weekly", "monthly", "less"],
        ),
        (
            "Lead time for changes",
            ["< 1 day", "< 1 week", "< 1 month", "more"],
        ),
        ("Change failure rate", ["≤ 5 %", "≤ 10 %", "≤ 15 %", "more"]),
        ("Time to restore", ["< 1 h", "< 1 day", "< 1 week", "more"]),
    ];
    for (title, bands) in rows {
        board::section(ui, theme, tl(title), "", |ui| {
            let levels = [Level::Elite, Level::High, Level::Medium, Level::Low];
            for (level, band) in levels.iter().zip(bands) {
                let (line, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                let w = board::paint_tag(
                    ui,
                    theme,
                    egui::pos2(line.left(), line.center().y),
                    level.label(),
                    level.tone(theme),
                    false,
                );
                board::paint_text(
                    ui,
                    egui::pos2(line.left() + w.max(64.0) + 10.0, line.center().y),
                    Align2::LEFT_CENTER,
                    tl(band),
                    FontId::proportional(theme.text_size - 1.0),
                    theme.palette.text_secondary,
                    line.width() - 80.0,
                );
            }
        });
    }
    board::section(ui, theme, tl("How they are measured"), "", |ui| {
        for text in [
            tl("Deployments: the successful deployments of the last tracked branch, else the MRs that reached it."),
            tl("Lead time: from an MR's first commit to its arrival on the last branch."),
            tl("Failures: failed deployments, reverts and hotfixes, over the deployments."),
            tl("Restore: from a failed deployment to the next good one, or from a reverted MR to its revert."),
        ] {
            ui.add(egui::Label::new(muted(theme, text).size(theme.text_size - 1.0)).wrap());
            ui.add_space(4.0);
        }
    });
}

// ── The weekly report ────────────────────────────────────────────────────────

/// The week as Markdown, to paste in a channel or a wiki: DORA, where the
/// time went, review rounds, CI, the MRs running late and the milestones.
pub fn weekly_report(
    app: &App,
    overview: Option<&ProjectOverview>,
    ci: &super::ci::Ci,
    now: DateTime<Utc>,
) -> String {
    use std::fmt::Write;
    let d = analyze(app, overview, now, 7);
    let project = app
        .project_name
        .clone()
        .unwrap_or_else(|| app.project_id.clone());
    let dash = board::dash();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## {}",
        tf(
            "Weekly report · {project} · week of {date}",
            &[
                ("project", &project),
                (
                    "date",
                    &(now - Duration::days(6)).format("%Y-%m-%d").to_string()
                ),
            ],
        )
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "### {}", tl("Delivery"));
    let dora = &d.dora;
    let band = |level: Option<Level>| {
        level
            .map(|l| format!(" ({})", l.label()))
            .unwrap_or_default()
    };
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Deployment frequency"),
        tf("{n} / week", &[("n", &format!("{:.1}", dora.per_week))]),
        band((dora.per_week > 0.0).then(|| Level::of_frequency(dora.per_week)))
    );
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Lead time for changes"),
        dora.lead
            .map_or_else(|| dash.to_string(), board::duration_text),
        band(dora.lead.map(Level::of_lead))
    );
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Change failure rate"),
        dora.failure_rate
            .map_or_else(|| dash.to_string(), board::percent),
        band(dora.failure_rate.map(Level::of_failure_rate))
    );
    let _ = writeln!(
        out,
        "- {}: {}{}",
        tl("Time to restore"),
        dora.restore
            .map_or_else(|| dash.to_string(), board::duration_text),
        band(dora.restore.map(Level::of_restore))
    );
    let _ = writeln!(
        out,
        "- {}",
        tn(d.merged.len() as u64, "{n} MR merged", "{n} MRs merged")
    );
    if let Some((stage, share)) = slowest(&d.medians) {
        let _ = writeln!(
            out,
            "- {}",
            tf(
                "Most of the time: {stage}, {share} of the way",
                &[
                    ("stage", &stage.label().to_lowercase()),
                    ("share", &board::percent(share)),
                ],
            )
        );
    }
    for line in [drafts_text(&d), never_draft_text(&d)]
        .into_iter()
        .flatten()
    {
        let _ = writeln!(out, "- {line}");
    }
    if d.rounds.reviewed > 0 {
        let _ = writeln!(
            out,
            "- {}",
            tf(
                "Approved at the first round: {share} of the reviewed MRs",
                &[(
                    "share",
                    &board::percent(d.rounds.first_time as f64 / d.rounds.reviewed as f64)
                )],
            )
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "### {}", tl("CI"));
    let _ = writeln!(
        out,
        "- {}: {}",
        tl("Pipelines passing"),
        ci.success.map_or_else(|| dash.to_string(), board::percent)
    );
    let _ = writeln!(
        out,
        "- {}: {}",
        tl("Median duration"),
        ci.median
            .map_or_else(|| dash.to_string(), board::seconds_text)
    );
    let _ = writeln!(
        out,
        "- {}: {}",
        tl("Runner time"),
        super::ci::hours_text(ci.budget.runner_secs)
    );
    let _ = writeln!(out);
    let late: Vec<&Aging> = d
        .open
        .iter()
        .filter(|a| d.p85.is_some_and(|p| a.age >= p))
        .collect();
    let _ = writeln!(out, "### {}", tl("Running late"));
    if late.is_empty() {
        let _ = writeln!(out, "- {}", tl("No open MR is older than usual."));
    }
    for a in late {
        let _ = writeln!(
            out,
            "- !{} {} — {} · {}",
            a.id,
            a.title,
            a.column.label(),
            tf("open for {time}", &[("time", &board::duration_text(a.age))]),
        );
    }
    let _ = writeln!(out);
    let weekly = super::milestones::weekly_merges(app, overview, now);
    let milestones: Vec<_> = super::milestones::milestones(app)
        .into_iter()
        .filter(|m| !m.title.is_empty() && !m.is_done())
        .collect();
    if !milestones.is_empty() {
        let _ = writeln!(out, "### {}", tl("Milestones"));
        for m in milestones {
            let f = super::milestones::forecast(&m, &weekly, now);
            let due = m
                .due
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| dash.to_string());
            let odds = f
                .chance
                .map(|c| {
                    format!(
                        " · {}",
                        tf("{p} chance to make it", &[("p", &board::percent(c))])
                    )
                })
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "- {} — {} {due} · {}{odds}",
                m.title,
                tl("due"),
                f.p85.map_or_else(
                    || tl("no forecast").to_string(),
                    |p| tf(
                        "85 % sure by {date}",
                        &[("date", &p.format("%Y-%m-%d").to_string())]
                    )
                ),
            );
        }
    }
    out
}

// ── The rail's badge ─────────────────────────────────────────────────────────

/// The open MRs older than 85 % of the merged ones; red when one is twice as old.
pub fn badge(g: &GuiApp, now: DateTime<Utc>) -> Option<Badge> {
    let d = g.memo.delivery.get(&g.app.project_id, (30, false), || {
        analyze(&g.app, None, now, 30)
    });
    let p85 = d.p85?;
    let late: Vec<&Aging> = d.open.iter().filter(|a| a.age >= p85).collect();
    if late.is_empty() {
        return None;
    }
    let tone = if late.iter().any(|a| a.age >= 2.0 * p85) {
        g.theme.tones.red
    } else {
        g.theme.tones.amber
    };
    let ids: Vec<String> = late.iter().map(|a| format!("!{}", a.id)).collect();
    Some(Badge {
        n: late.len(),
        tone,
        hint: rail_badges::listed(
            tn(
                late.len() as u64,
                "{n} open MR older than 85 % of the merged ones",
                "{n} open MRs older than 85 % of the merged ones",
            ),
            rail_badges::names(&ids, 4),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MrData, ReviewActivity};

    fn at(s: &str) -> DateTime<Utc> {
        board::parse_time(s).unwrap()
    }

    #[test]
    fn a_path_cuts_its_time_into_the_steps_it_went_through() {
        let path = Path {
            id: "1".into(),
            title: String::new(),
            author: String::new(),
            created: at("2026-11-01T00:00:00Z"),
            ready: Some(at("2026-11-02T00:00:00Z")),
            never_draft: true,
            draft_comments: 0,
            reviewed: Some(at("2026-11-04T00:00:00Z")),
            approved: None,
            merged: Some(at("2026-11-05T00:00:00Z")),
            delivered: Some(at("2026-11-07T00:00:00Z")),
            first_commit: None,
            rounds: Some(0),
        };
        let stages = path.stages(at("2026-11-10T00:00:00Z"));
        // No approval: its time goes to the merge.
        assert_eq!(stages, [Some(1.0), Some(2.0), None, Some(1.0), Some(2.0)]);
        assert_eq!(path.total(), Some(6.0));
    }

    #[test]
    fn an_open_mr_counts_its_current_step_up_to_now() {
        let path = Path {
            id: "1".into(),
            title: String::new(),
            author: String::new(),
            created: at("2026-11-01T00:00:00Z"),
            ready: Some(at("2026-11-02T00:00:00Z")),
            never_draft: true,
            draft_comments: 0,
            reviewed: None,
            approved: None,
            merged: None,
            delivered: None,
            first_commit: None,
            rounds: None,
        };
        let stages = path.stages(at("2026-11-05T00:00:00Z"));
        assert_eq!(stages, [Some(1.0), Some(3.0), None, None, None]);
    }

    #[test]
    fn the_dora_bands() {
        assert_eq!(Level::of_frequency(10.0), Level::Elite);
        assert_eq!(Level::of_frequency(2.0), Level::High);
        assert_eq!(Level::of_frequency(0.5), Level::Medium);
        assert_eq!(Level::of_frequency(0.1), Level::Low);
        assert_eq!(Level::of_lead(0.5), Level::Elite);
        assert_eq!(Level::of_lead(40.0), Level::Low);
        assert_eq!(Level::of_failure_rate(0.05), Level::Elite);
        assert_eq!(Level::of_failure_rate(0.3), Level::Low);
        assert_eq!(Level::of_restore(0.5), Level::High);
    }

    #[test]
    fn a_late_push_without_a_count_is_one_round() {
        let mut mr = TrackedMr::placeholder("1".into(), "A".into(), None);
        mr.data = MrData {
            review: Some(ReviewActivity {
                first_review_at: Some("2026-11-01T10:00:00Z".into()),
                last_push_at: Some("2026-11-02T10:00:00Z".into()),
                ..ReviewActivity::default()
            }),
            ..MrData::default()
        };
        assert_eq!(rounds_of(&mr), Some(1));
        mr.data.review.as_mut().unwrap().pushes_after_review = 3;
        assert_eq!(rounds_of(&mr), Some(3));
    }

    #[test]
    fn a_change_says_which_way_and_whether_it_is_better() {
        // A shorter lead time is better.
        assert_eq!(
            versus(7.2, 8.9, (18, 15), false, 0.05),
            Versus::Moved {
                up: false,
                better: true
            }
        );
        // More failures is worse.
        assert_eq!(
            versus(0.38, 0.25, (8, 9), false, 0.005),
            Versus::Moved {
                up: true,
                better: false
            }
        );
        // Two incidents against one: too few to tell.
        assert_eq!(
            versus(3.6, 2.1, (2, 1), false, 0.05),
            Versus::TooFew { up: true }
        );
        assert_eq!(versus(1.0, 1.02, (9, 9), true, 0.05), Versus::Same);
    }

    #[tokio::test]
    async fn the_period_before_counts_nothing_of_the_current_one() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let overview = crate::demo::demo_project_overview(&app);
        let now = Utc::now();
        let current = analyze(&app, Some(&overview), now, 30);
        let before = analyze(&app, Some(&overview), now - Duration::days(30), 30);
        let start = now - Duration::days(30);
        assert!(before
            .merged
            .iter()
            .all(|p| p.merged.is_some_and(|m| m <= start)));
        assert!(current
            .merged
            .iter()
            .all(|p| p.merged.is_some_and(|m| m >= start)));
        let ids = |d: &Delivery| d.merged.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
        assert!(ids(&before).iter().all(|id| !ids(&current).contains(id)));
        assert!(
            !before.merged.is_empty(),
            "the demo has merges in the period before"
        );
    }

    #[tokio::test]
    async fn the_demo_has_a_delivery_to_show() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let now = Utc::now();
        let overview = crate::demo::demo_project_overview(&app);
        let d = analyze(&app, Some(&overview), now, 30);
        assert!(d.merged.len() >= 5, "{}", d.merged.len());
        assert!(d.dora.has_deployments);
        assert!(d.dora.lead.is_some());
        assert!(!d.dora.reverts.is_empty());
        assert!(d.dora.restore.is_some());
        assert!(d.rounds.reviewed > 0);
        assert!(slowest(&d.medians).is_some());
        let ci = super::super::ci::analyze(&app, now, 7);
        let report = weekly_report(&app, Some(&overview), &ci, now);
        assert!(report.contains("Deployment frequency"), "{report}");
        assert!(report.contains("v2.4.0"), "{report}");
    }

    fn merged(id: &str, title: &str) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), title.into(), None);
        mr.data.author = "Alex <@alex>".into();
        mr.data.state = GitlabMrState::Merged;
        mr
    }

    fn commit(sha: &str, title: &str) -> crate::models::MrCommit {
        crate::models::MrCommit {
            sha: sha.into(),
            fingerprint: crate::models::CommitFingerprint {
                author_email: "alex@example.com".into(),
                authored_at: 0,
                title: title.into(),
            },
            author_name: "Alex".into(),
        }
    }

    async fn app_with(mrs: Vec<TrackedMr>) -> App {
        let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        app.mrs = mrs;
        app
    }

    #[tokio::test]
    async fn a_revert_finds_what_it_undoes_by_description_commit_or_title() {
        let mut original = merged("12", "feat: quote cache");
        original.data.commits = vec![commit("1a2b3c4d5e6f", "feat: quote cache")];
        let other = merged("13", "feat: something else");

        // GitLab's Revert button: the description names the MR.
        let mut by_button = merged("20", "Revert \"feat: something else\"");
        by_button.data.description = "This reverts merge request !12".into();
        // `git revert`: the description (the commit's body) names the commit.
        let mut by_commit = merged("21", "Revert the cache");
        by_commit.data.description = "This reverts commit 1a2b3c4d5e6f0011.".into();
        // Only the title.
        let by_title = merged("22", "Revert \"feat: quote cache\"");
        // A commit's title, under an MR named otherwise.
        let mut by_commit_title = merged("23", "revert: back to the old cache");
        by_commit_title.data.commits = vec![commit("ffff", "Revert \"feat: quote cache\"")];
        // Not a revert.
        let mut fix = merged("24", "fix: cache size");
        fix.data.description = "This reverts merge request !12".into();

        let app = app_with(vec![
            original,
            other,
            by_button.clone(),
            by_commit.clone(),
            by_title.clone(),
            by_commit_title.clone(),
            fix.clone(),
        ])
        .await;
        let found = |mr: &TrackedMr| reverted(&app, mr).map(|(m, how)| (m.id.clone(), how));
        // The description wins over the quoted title.
        assert_eq!(found(&by_button), Some(("12".into(), Found::Description)));
        assert_eq!(found(&by_commit), Some(("12".into(), Found::Commit)));
        assert_eq!(found(&by_title), Some(("12".into(), Found::Title)));
        assert_eq!(found(&by_commit_title), Some(("12".into(), Found::Title)));
        assert_eq!(found(&fix), None);
    }

    #[tokio::test]
    async fn hotfixes_follow_the_project_branches_and_the_label() {
        let mut app = app_with(vec![]).await;
        let mut mr = merged("30", "fix: login");
        mr.data.source_branch = "hotfix/login".into();
        assert!(is_hotfix(&app, &mr));
        app.config.hotfix_branches = "fix/prod-*, urgent-*".into();
        assert!(!is_hotfix(&app, &mr));
        mr.data.source_branch = "fix/prod-login".into();
        assert!(is_hotfix(&app, &mr));
        app.config.hotfix_branches = String::new();
        assert!(!is_hotfix(&app, &mr));
        mr.data.labels = vec!["Hotfix".into()];
        assert!(is_hotfix(&app, &mr));
    }

    #[tokio::test]
    async fn the_demo_lists_its_revert_and_hotfixes() {
        let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
        let overview = crate::demo::demo_project_overview(&app);
        let d = analyze(&app, Some(&overview), Utc::now(), 90);
        let revert = d
            .undos
            .iter()
            .find(|u| u.kind == UndoKind::Revert)
            .expect("the demo has a revert");
        let (_, title, _) = revert.undoes.as_ref().expect("it finds what it undoes");
        assert!(title.contains("LRU eviction"), "{title}");
        assert!(revert.after.is_some());
        assert!(d.undos.iter().any(|u| u.kind == UndoKind::Hotfix));
        // What the change failure rate counts is in the list.
        for id in d.dora.reverts.iter().chain(&d.dora.hotfixes) {
            assert!(d.undos.iter().any(|u| &u.id == id && u.counted), "!{id}");
        }
    }
}
