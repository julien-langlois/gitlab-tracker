use crate::app::App;
use crate::models::{GitlabMrState, MergeabilityStatus, PipelineState};
use crate::utils::matches_gitlab_username;
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use std::collections::BTreeMap;

const MERGED_LAST_7_DAYS_WINDOW: i64 = 7;
const MERGED_LAST_30_DAYS_WINDOW: i64 = 30;
const STALE_DAYS_THRESHOLD: i64 = 7;
const DUE_SOON_DAYS_THRESHOLD: i64 = 7;
const COMPLEX_SCORE_THRESHOLD: f64 = 0.66;
const MANY_COMMITS_THRESHOLD: u32 = 10;
const MANY_FILES_THRESHOLD: u32 = 20;
const HOT_THREADS_THRESHOLD: u32 = 10;
const RELEASE_URGENT_DAYS_THRESHOLD: i64 = 3;
const RELEASE_URGENT_REMAINING_THRESHOLD: usize = 2;
const RELEASE_SOON_DAYS_THRESHOLD: i64 = 7;
const RELEASE_SOON_REMAINING_THRESHOLD: usize = 5;
const MAX_RELEASE_SUMMARIES: usize = 3;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DashboardSummary {
    pub(super) open: usize,
    pub(super) merged: usize,
    pub(super) closed: usize,
    pub(super) mergeable: usize,
    pub(super) draft: usize,
    pub(super) merged_today: usize,
    pub(super) merged_this_week: usize,
    pub(super) merged_this_month: usize,
    pub(super) merged_last_7_days: usize,
    pub(super) merged_last_30_days: usize,
    pub(super) conflicts: usize,
    pub(super) needs_rebase: usize,
    pub(super) ci_failing: usize,
    pub(super) ci_running: usize,
    pub(super) discussions: usize,
    pub(super) requested_changes: usize,
    pub(super) needs_review: usize,
    pub(super) assigned_to_me: usize,
    pub(super) review_by_me: usize,
    pub(super) no_reviewer: usize,
    pub(super) no_assignee: usize,
    pub(super) flagged: usize,
    pub(super) stale_7_days: usize,
    pub(super) oldest_open_days: Option<i64>,
    pub(super) no_milestone: usize,
    pub(super) due_this_week: usize,
    pub(super) overdue: usize,
    pub(super) behind_target: usize,
    pub(super) hot_threads: usize,
    pub(super) with_ticket: usize,
    pub(super) no_ticket: usize,
    pub(super) over_estimate: usize,
    pub(super) complex: usize,
    pub(super) large_diff: usize,
    pub(super) many_commits: usize,
    pub(super) many_files: usize,
    pub(super) no_diff_stats: usize,
    diff_stats_count: usize,
    total_diff_lines: u64,
    avg_diff_lines_from_stats: Option<u64>,
    pub(super) pipeline_unknown: usize,
    pub(super) ci_skipped: usize,
    pub(super) recently_updated: usize,
}

#[derive(Debug, Default)]
pub(super) struct ReleaseSummary {
    pub(super) title: String,
    pub(super) due_date: Option<NaiveDate>,
    pub(super) merged: usize,
    pub(super) in_progress: usize,
    pub(super) blocked: usize,
    pub(super) waiting_review: usize,
}

impl ReleaseSummary {
    fn remaining(&self) -> usize {
        self.in_progress + self.blocked + self.waiting_review
    }

    pub(super) fn is_at_risk(&self, today: NaiveDate) -> bool {
        let Some(due_date) = self.due_date else {
            return false;
        };

        let days_left = (due_date - today).num_days();
        self.remaining() > 0
            && (days_left < 0
                || (days_left <= RELEASE_URGENT_DAYS_THRESHOLD
                    && self.remaining() >= RELEASE_URGENT_REMAINING_THRESHOLD)
                || (days_left <= RELEASE_SOON_DAYS_THRESHOLD
                    && self.remaining() >= RELEASE_SOON_REMAINING_THRESHOLD))
    }
}

impl DashboardSummary {
    #[cfg(feature = "stats")]
    pub(super) fn from_app(app: &App) -> Self {
        let mut summary = Self::from_visible_mrs(app);
        summary.enrich_with_stats_report(app);
        summary
    }

    #[cfg(not(feature = "stats"))]
    pub(super) fn from_app(app: &App) -> Self {
        Self::from_visible_mrs(app)
    }

    fn from_visible_mrs(app: &App) -> Self {
        let mut summary = Self::default();
        let now = Utc::now();
        let today = now.date_naive();
        let current_iso_week = now.iso_week();
        let current_year = now.year();
        let current_month = now.month();
        let username = app.config.gitlab_username.as_deref();
        let tracker_enabled = app.tracker.is_some();

        for mr in app.visible_mrs() {
            match mr.state {
                GitlabMrState::Opened => summary.open += 1,
                GitlabMrState::Merged => {
                    summary.merged += 1;
                    if let Some(merged_at) = parse_gitlab_datetime(mr.merged_at.as_deref()) {
                        if merged_at.date_naive() == today {
                            summary.merged_today += 1;
                        }
                        if merged_at.iso_week() == current_iso_week {
                            summary.merged_this_week += 1;
                        }
                        if merged_at.year() == current_year && merged_at.month() == current_month {
                            summary.merged_this_month += 1;
                        }
                        if merged_at >= now - Duration::days(MERGED_LAST_7_DAYS_WINDOW) {
                            summary.merged_last_7_days += 1;
                        }
                        if merged_at >= now - Duration::days(MERGED_LAST_30_DAYS_WINDOW) {
                            summary.merged_last_30_days += 1;
                        }
                    }
                }
                GitlabMrState::Closed => summary.closed += 1,
            }

            if mr.flagged {
                summary.flagged += 1;
            }
            if mr.recently_updated {
                summary.recently_updated += 1;
            }

            if tracker_enabled {
                if let Some(ticket) = &mr.linked_ticket {
                    summary.with_ticket += 1;
                    if is_over_estimate(ticket.time_estimate, ticket.time_spent) {
                        summary.over_estimate += 1;
                    }
                } else {
                    summary.no_ticket += 1;
                }
            }

            if mr.state != GitlabMrState::Opened {
                continue;
            }

            match mr.mergeability {
                MergeabilityStatus::Mergeable => summary.mergeable += 1,
                MergeabilityStatus::Draft => summary.draft += 1,
                MergeabilityStatus::Conflict => summary.conflicts += 1,
                MergeabilityStatus::NeedsRebase => summary.needs_rebase += 1,
                MergeabilityStatus::DiscussionsNotResolved => summary.discussions += 1,
                MergeabilityStatus::RequestedChanges => summary.requested_changes += 1,
                MergeabilityStatus::CiStillRunning => summary.ci_running += 1,
                MergeabilityStatus::CiMustPass => {}
                MergeabilityStatus::NotApproved => summary.needs_review += 1,
                _ => {}
            }

            match mr.pipelines.first().map(|pipeline| &pipeline.status) {
                Some(PipelineState::Failed) => summary.ci_failing += 1,
                Some(PipelineState::Skipped) => summary.ci_skipped += 1,
                None => summary.pipeline_unknown += 1,
                _ => {}
            }

            if mr.reviewers.is_empty() {
                summary.no_reviewer += 1;
            }
            if mr.assignee == "None" || mr.assignee.trim().is_empty() {
                summary.no_assignee += 1;
            }

            if let Some(username) = username {
                if matches_gitlab_username(&mr.assignee, username) {
                    summary.assigned_to_me += 1;
                }
                if mr
                    .reviewers
                    .iter()
                    .any(|reviewer| matches_gitlab_username(reviewer, username))
                {
                    summary.review_by_me += 1;
                }
            }

            if parse_gitlab_datetime(mr.updated_at.as_deref())
                .is_some_and(|updated_at| updated_at < now - Duration::days(STALE_DAYS_THRESHOLD))
            {
                summary.stale_7_days += 1;
            }

            if let Some(created_at) = parse_gitlab_datetime(mr.created_at.as_deref()) {
                let age_days = (now - created_at).num_days().max(0);
                summary.oldest_open_days = Some(
                    summary
                        .oldest_open_days
                        .map_or(age_days, |oldest| oldest.max(age_days)),
                );
            }

            if let Some(stats) = &mr.diff_stats {
                let diff_lines = u64::from(stats.additions + stats.deletions);
                summary.diff_stats_count += 1;
                summary.total_diff_lines += diff_lines;

                if stats.difficulty(&app.config.complexity_profile) >= COMPLEX_SCORE_THRESHOLD {
                    summary.complex += 1;
                }
                if diff_lines >= u64::from(app.config.complexity_profile.hard_threshold) {
                    summary.large_diff += 1;
                }
                if stats.commits_count >= MANY_COMMITS_THRESHOLD {
                    summary.many_commits += 1;
                }
                if stats.files_changed >= MANY_FILES_THRESHOLD {
                    summary.many_files += 1;
                }
                if stats.commits_behind.is_some_and(|behind| behind > 0) {
                    summary.behind_target += 1;
                }
            } else {
                summary.no_diff_stats += 1;
            }

            if mr.milestone.trim().is_empty() || mr.milestone == "None" {
                summary.no_milestone += 1;
            }
            if let Some(due_date) = parse_gitlab_date(mr.milestone_due_date.as_deref()) {
                if due_date < today {
                    summary.overdue += 1;
                } else if due_date <= today + Duration::days(DUE_SOON_DAYS_THRESHOLD) {
                    summary.due_this_week += 1;
                }
            }

            if mr.user_notes_count >= HOT_THREADS_THRESHOLD {
                summary.hot_threads += 1;
            }
        }

        summary
    }

    #[cfg(feature = "stats")]
    fn enrich_with_stats_report(&mut self, app: &App) {
        let Some(report) = app.stats_view.report.as_ref() else {
            return;
        };

        let stats = &report.aggregated;

        // Persisted stats are the source of truth for lifecycle counters because
        // visible_mrs() reflects only the current cockpit list. Removing an MR from
        // the visible list must not rewrite historical flow metrics when stats are
        // available.
        self.open = stats.open_mr_ages_days.len();
        self.merged = stats.merged_count;
        self.closed = stats.closed_count;
        self.merged_today = stats.merged_today;
        self.merged_this_week = stats.merged_this_week;
        self.merged_this_month = stats.merged_this_month;
        self.merged_last_7_days = stats.merged_last_7_days;
        self.merged_last_30_days = stats.merged_last_30_days;

        if let Some(oldest_open_age) = stats.open_mr_ages_days.last() {
            self.oldest_open_days = Some(oldest_open_age.floor() as i64);
        }

        if stats.avg_diff_size > 0.0 {
            self.avg_diff_lines_from_stats = Some(stats.avg_diff_size.round() as u64);
        }
    }

    pub(super) fn total_blocked(self) -> usize {
        self.conflicts
            + self.needs_rebase
            + self.ci_failing
            + self.discussions
            + self.requested_changes
    }

    pub(super) fn avg_diff_lines(self) -> Option<u64> {
        if self.avg_diff_lines_from_stats.is_some() {
            return self.avg_diff_lines_from_stats;
        }

        if self.diff_stats_count == 0 {
            None
        } else {
            Some(self.total_diff_lines / self.diff_stats_count as u64)
        }
    }
}

pub(super) fn release_summaries(app: &App) -> Vec<ReleaseSummary> {
    let today = Utc::now().date_naive();
    let mut releases = BTreeMap::<String, ReleaseSummary>::new();

    // Current dashboard data is the baseline source for release health. Stats, when
    // compiled and loaded, only enrich historical completion counters below.
    for mr in app.visible_mrs() {
        let milestone = mr.milestone.trim();
        if milestone.is_empty() || milestone == "None" {
            continue;
        }

        let entry = releases
            .entry(milestone.to_string())
            .or_insert_with(|| ReleaseSummary {
                title: milestone.to_string(),
                due_date: parse_gitlab_date(mr.milestone_due_date.as_deref()),
                ..ReleaseSummary::default()
            });

        if entry.due_date.is_none() {
            entry.due_date = parse_gitlab_date(mr.milestone_due_date.as_deref());
        }

        match mr.state {
            GitlabMrState::Merged => entry.merged += 1,
            GitlabMrState::Opened => match mr.mergeability {
                MergeabilityStatus::Conflict | MergeabilityStatus::NeedsRebase => {
                    entry.blocked += 1;
                }
                MergeabilityStatus::NotApproved | MergeabilityStatus::RequestedChanges => {
                    entry.waiting_review += 1;
                }
                _ => entry.in_progress += 1,
            },
            GitlabMrState::Closed => {}
        }
    }

    enrich_release_summaries_with_stats(app, &mut releases);

    let mut releases: Vec<ReleaseSummary> = releases.into_values().collect();
    releases.sort_by_key(|release| {
        release
            .due_date
            .map(|due_date| ((due_date - today).num_days().abs(), due_date))
            .unwrap_or((i64::MAX, NaiveDate::MAX))
    });
    releases.truncate(MAX_RELEASE_SUMMARIES);
    releases
}

#[cfg(feature = "stats")]
fn enrich_release_summaries_with_stats(app: &App, releases: &mut BTreeMap<String, ReleaseSummary>) {
    let Some(report) = app.stats_view.report.as_ref() else {
        return;
    };

    // Historical completion data belongs to the stats crate: it is persisted and
    // deduplicated by gitlab_tracker_stats::aggregator::aggregate instead of being
    // inferred from the currently visible MR list.
    for (milestone, merged) in &report.aggregated.throughput_by_milestone {
        let milestone = milestone.trim();
        if milestone.is_empty() || milestone == "None" {
            continue;
        }

        let entry = releases
            .entry(milestone.to_string())
            .or_insert_with(|| ReleaseSummary {
                title: milestone.to_string(),
                ..ReleaseSummary::default()
            });
        entry.merged = *merged as usize;
    }
}

#[cfg(not(feature = "stats"))]
fn enrich_release_summaries_with_stats(
    _app: &App,
    _releases: &mut BTreeMap<String, ReleaseSummary>,
) {
}

fn is_over_estimate(estimate: Option<u32>, spent: Option<u32>) -> bool {
    matches!((estimate, spent), (Some(estimate), Some(spent)) if estimate > 0 && spent > estimate)
}

fn parse_gitlab_datetime(value: Option<&str>) -> Option<DateTime<Utc>> {
    value?.parse::<DateTime<Utc>>().ok()
}

fn parse_gitlab_date(value: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value?, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_summary_is_not_at_risk_without_due_date() {
        let release = ReleaseSummary {
            due_date: None,
            in_progress: 5,
            blocked: 2,
            waiting_review: 1,
            ..ReleaseSummary::default()
        };

        let today = NaiveDate::from_ymd_opt(2024, 6, 1).expect("valid test date");

        assert!(!release.is_at_risk(today));
    }

    #[test]
    fn release_summary_is_at_risk_when_overdue_with_remaining_work() {
        let release = ReleaseSummary {
            due_date: NaiveDate::from_ymd_opt(2024, 5, 31),
            in_progress: 1,
            ..ReleaseSummary::default()
        };

        let today = NaiveDate::from_ymd_opt(2024, 6, 1).expect("valid test date");

        assert!(release.is_at_risk(today));
    }

    #[test]
    fn release_summary_is_not_at_risk_when_overdue_but_complete() {
        let release = ReleaseSummary {
            due_date: NaiveDate::from_ymd_opt(2024, 5, 31),
            merged: 3,
            ..ReleaseSummary::default()
        };

        let today = NaiveDate::from_ymd_opt(2024, 6, 1).expect("valid test date");

        assert!(!release.is_at_risk(today));
    }

    #[test]
    fn release_summary_is_at_risk_when_urgent_threshold_is_met() {
        let release = ReleaseSummary {
            due_date: NaiveDate::from_ymd_opt(2024, 6, 4),
            in_progress: RELEASE_URGENT_REMAINING_THRESHOLD,
            ..ReleaseSummary::default()
        };

        let today = NaiveDate::from_ymd_opt(2024, 6, 1).expect("valid test date");

        assert!(release.is_at_risk(today));
    }

    #[test]
    fn dashboard_summary_total_blocked_counts_all_blocking_categories() {
        let summary = DashboardSummary {
            conflicts: 1,
            needs_rebase: 2,
            ci_failing: 3,
            discussions: 4,
            requested_changes: 5,
            ..DashboardSummary::default()
        };

        assert_eq!(summary.total_blocked(), 15);
    }

    #[test]
    fn dashboard_summary_avg_diff_lines_uses_runtime_diff_stats() {
        let summary = DashboardSummary {
            diff_stats_count: 2,
            total_diff_lines: 101,
            ..DashboardSummary::default()
        };

        assert_eq!(summary.avg_diff_lines(), Some(50));
    }

    #[test]
    fn dashboard_summary_avg_diff_lines_prefers_persisted_stats_report_value() {
        let summary = DashboardSummary {
            diff_stats_count: 2,
            total_diff_lines: 100,
            avg_diff_lines_from_stats: Some(42),
            ..DashboardSummary::default()
        };

        assert_eq!(summary.avg_diff_lines(), Some(42));
    }

    #[test]
    fn over_estimate_requires_positive_estimate_and_spent_above_estimate() {
        assert!(is_over_estimate(Some(3600), Some(7200)));
        assert!(!is_over_estimate(Some(3600), Some(3600)));
        assert!(!is_over_estimate(Some(0), Some(3600)));
        assert!(!is_over_estimate(None, Some(3600)));
        assert!(!is_over_estimate(Some(3600), None));
    }
}
