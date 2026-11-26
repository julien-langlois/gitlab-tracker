//! What the page computes: steps, paths, aging levels, DORA figures, review rounds.

use super::*;

/// A step of an MR's way to production, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Opened, not up for review yet: still being written. Until it is marked
    /// ready or a review is asked, whichever comes first.
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
            Stage::Development => "From the opening to the move out of draft (marked ready) or the first review request, whichever comes first. Comments on a draft nobody was asked to review yet are the review along the way and stay in this step.",
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

    pub(super) fn label(self) -> &'static str {
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
