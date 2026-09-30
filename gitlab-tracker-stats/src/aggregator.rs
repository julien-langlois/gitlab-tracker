use std::collections::HashMap;

use chrono::{DateTime, Datelike, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::db::{SnapshotQuery, StatsDb, StatsError, StoredSnapshot};
use crate::metrics::PerMrMetrics;
use crate::snapshot::SnapshotTrigger;

/// Defines the temporal scope of an aggregation query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeWindow {
    /// Rolling window: include all snapshots recorded in the last N days.
    LastDays(u32),
    /// Milestone-scoped: include all merged/closed snapshots for this milestone title.
    Milestone(String),
    /// Explicit date range (ISO 8601 dates, inclusive start, exclusive end).
    Range { from: String, to: String },
}

/// Full filter passed to [`aggregate`].
#[derive(Debug, Clone, Default)]
pub struct QueryFilter {
    pub window: Option<TimeWindow>,
    pub project_id: Option<String>,
    pub author: Option<String>,
    pub reviewer: Option<String>,
    pub target_branch: Option<String>,
    /// Sprint duration in weeks — used by throughput forecasts.
    /// Defaults to 2 when absent.
    pub sprint_weeks: Option<u32>,
}

/// Cycle-time and volume stats for one MR diff-size bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrSizeBucketStats {
    /// Human-readable bucket label.
    pub label: String,

    /// Inclusive lower bound for changed lines.
    pub min_changed_lines: u32,

    /// Inclusive upper bound for changed lines. `None` means unbounded.
    pub max_changed_lines: Option<u32>,

    /// Number of latest MRs in this bucket.
    pub total_mrs: usize,

    /// Number of merged MRs in this bucket.
    pub merged_mrs: usize,

    /// Median cycle time for merged MRs in this bucket.
    pub cycle_time_median_hours: Option<f64>,
}

/// Aggregated statistics computed over a set of MR snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedStats {
    // ── Throughput ────────────────────────────────────────────────────────────
    /// Total number of MRs in the sample.
    pub total_mrs: usize,

    /// MRs merged within the query window.
    pub merged_count: usize,

    /// MRs closed (abandoned) within the query window.
    pub closed_count: usize,

    /// Share of terminal MRs that were closed without being merged.
    /// `None` when no terminal MR exists in the sample.
    pub abandon_rate: Option<f64>,

    /// MRs merged on the current UTC calendar day.
    pub merged_today: usize,

    /// MRs merged during the current UTC ISO week.
    pub merged_this_week: usize,

    /// MRs merged during the current UTC calendar month.
    pub merged_this_month: usize,

    /// MRs merged during the last 7 rolling days.
    pub merged_last_7_days: usize,

    /// MRs merged during the last 30 rolling days.
    pub merged_last_30_days: usize,

    /// Average number of MRs merged per calendar week in the window.
    /// `None` when the window duration is unknown or zero.
    pub throughput_per_week: Option<f64>,

    // ── Cycle time (hours) ────────────────────────────────────────────────────
    /// Median cycle time (P50) across all merged MRs in the sample.
    pub cycle_time_median_hours: Option<f64>,

    /// 75th-percentile cycle time — upper quartile, useful mid-point between median and P90.
    pub cycle_time_p75_hours: Option<f64>,

    /// 90th-percentile cycle time (captures the "long tail" of stuck MRs).
    pub cycle_time_p90_hours: Option<f64>,

    /// Average cycle time grouped by MR author.
    pub cycle_time_by_author: HashMap<String, f64>,

    /// Average cycle time grouped by reviewer (across all reviewers on each MR).
    pub cycle_time_by_reviewer: HashMap<String, f64>,

    /// Average cycle time grouped by milestone title.
    pub cycle_time_by_milestone: HashMap<String, f64>,

    // ── Diff and review quality ───────────────────────────────────────────────
    /// Mean diff size (additions + deletions) across all MRs.
    pub avg_diff_size: f64,

    /// Mean comment count per MR.
    pub avg_comments: f64,

    /// Mean comments per 100 changed lines.
    /// `None` when no MR has a non-empty diff.
    pub avg_comment_density: Option<f64>,

    /// Mean pipeline failure rate across all MRs that had at least one pipeline.
    /// `None` when no MR in the sample had pipeline data.
    pub avg_pipeline_failure_rate: Option<f64>,

    /// Share of MRs in the sample that have pipeline data.
    pub pipeline_data_coverage: f64,

    /// Number of MRs with usable merged cycle-time data.
    pub cycle_time_sample_size: usize,

    /// Number of MRs with usable pipeline data.
    pub pipeline_sample_size: usize,

    /// Share of latest MRs that have at least one reviewer assigned.
    pub reviewer_coverage: f64,

    /// Share of latest MRs that have a milestone.
    pub milestone_coverage: f64,

    /// Cycle-time distribution split by changed-line buckets.
    pub size_buckets: Vec<MrSizeBucketStats>,

    // ── Backlog health ────────────────────────────────────────────────────────
    /// Ages (in days) of MRs that are still open at snapshot time.
    /// Sorted ascending. Empty when no open MRs are in the sample.
    pub open_mr_ages_days: Vec<f64>,

    /// Number of open MRs older than 7 days.
    pub stale_open_mrs_7d: usize,

    /// Number of open MRs older than 14 days.
    pub stale_open_mrs_14d: usize,

    /// Number of open MRs older than 30 days.
    pub stale_open_mrs_30d: usize,

    /// Throughput (merged MR count) broken down by milestone title.
    pub throughput_by_milestone: HashMap<String, u32>,
}

/// Loads snapshots from the DB for the given filter and computes aggregated statistics.
pub async fn aggregate(
    db: &dyn StatsDb,
    filter: &QueryFilter,
) -> Result<AggregatedStats, StatsError> {
    let snapshots = db.query(&build_snapshot_query(filter)).await?;
    let metrics: Vec<PerMrMetrics> = snapshots.iter().map(PerMrMetrics::from_snapshot).collect();

    Ok(compute_stats(&snapshots, &metrics, filter))
}

/// Translates a [`QueryFilter`] into a [`SnapshotQuery`] for the DB layer.
pub fn build_snapshot_query(filter: &QueryFilter) -> SnapshotQuery {
    let mut q = SnapshotQuery {
        project_id: filter.project_id.clone(),
        author: filter.author.clone(),
        reviewer: filter.reviewer.clone(),
        target_branch: filter.target_branch.clone(),
        ..Default::default()
    };

    match &filter.window {
        Some(TimeWindow::LastDays(days)) => {
            let from = Utc::now()
                .checked_sub_signed(Duration::days(*days as i64))
                .map(|dt| dt.to_rfc3339());
            q.from_date = from;
        }
        Some(TimeWindow::Range { from, to }) => {
            q.from_date = Some(from.clone());
            q.to_date = Some(to.clone());
        }
        Some(TimeWindow::Milestone(title)) => {
            q.milestone = Some(title.clone());
        }
        None => {}
    }

    q
}

/// Pure computation over already-loaded snapshots and their derived metrics.
pub(crate) fn compute_stats(
    snapshots: &[StoredSnapshot],
    metrics: &[PerMrMetrics],
    filter: &QueryFilter,
) -> AggregatedStats {
    // Latest known snapshot per MR is the source of truth for stateful/current
    // metrics. Queries are ordered by recorded_at ASC, so later inserts overwrite
    // older snapshots for the same MR.
    // `snapshots` and `metrics` are parallel slices, so the latest entry per MR is
    // tracked by index and both views borrow it — no metric is computed twice.
    let latest_indices: Vec<usize> = {
        let mut latest = HashMap::<&str, usize>::new();
        for (i, snap) in snapshots.iter().enumerate() {
            latest.insert(snap.snapshot.mr_id.as_str(), i);
        }
        latest.into_values().collect()
    };
    let latest_snapshots: Vec<&StoredSnapshot> =
        latest_indices.iter().map(|&i| &snapshots[i]).collect();
    let latest_metrics: Vec<&PerMrMetrics> = latest_indices.iter().map(|&i| &metrics[i]).collect();

    // Deduplicate terminal lifecycle events by mr_id. Backfills may create the
    // same on_merge/on_close event on different recorded dates; all throughput
    // and cycle-time metrics must count each MR once.
    let merged_pairs: Vec<(&StoredSnapshot, &PerMrMetrics)> = {
        let mut seen = std::collections::HashSet::new();
        snapshots
            .iter()
            .zip(metrics.iter())
            .rev()
            .filter(|(_, metric)| {
                metric.trigger == SnapshotTrigger::OnMerge && seen.insert(metric.mr_id.as_str())
            })
            .collect()
    };
    let merged: Vec<&PerMrMetrics> = merged_pairs.iter().map(|(_, metric)| *metric).collect();

    let now = Utc::now();
    let merged_period_counts = compute_merged_period_counts(snapshots.iter(), now);

    let closed_count = {
        let mut seen = std::collections::HashSet::new();
        metrics
            .iter()
            .rev()
            .filter(|m| m.trigger == SnapshotTrigger::OnClose && seen.insert(m.mr_id.as_str()))
            .count()
    };

    // ── Cycle times ───────────────────────────────────────────────────────────
    let mut cycle_times: Vec<f64> = merged.iter().filter_map(|m| m.cycle_time_hours).collect();
    cycle_times.sort_by(f64::total_cmp);

    let cycle_time_median_hours = percentile(&cycle_times, 50.0);
    let cycle_time_p75_hours = percentile(&cycle_times, 75.0);
    let cycle_time_p90_hours = percentile(&cycle_times, 90.0);

    // ── Cycle time by author ──────────────────────────────────────────────────
    let cycle_time_by_author = group_average(
        merged.iter().copied(),
        |m| m.author.clone(),
        |m| m.cycle_time_hours,
    );

    // ── Cycle time by reviewer ────────────────────────────────────────────────
    let mut reviewer_times: HashMap<String, Vec<f64>> = HashMap::new();
    for (snap, metric) in &merged_pairs {
        if let Some(ct) = metric.cycle_time_hours {
            for reviewer in &snap.snapshot.reviewers {
                reviewer_times.entry(reviewer.clone()).or_default().push(ct);
            }
        }
    }
    let cycle_time_by_reviewer: HashMap<String, f64> = reviewer_times
        .into_iter()
        .map(|(reviewer, mut times)| {
            times.sort_by(f64::total_cmp);
            (reviewer, percentile(&times, 50.0).unwrap_or(0.0))
        })
        .collect();

    // ── Cycle time by milestone ───────────────────────────────────────────────
    let cycle_time_by_milestone = group_average(
        merged.iter().copied(),
        |m| m.milestone.clone().unwrap_or_default(),
        |m| m.cycle_time_hours,
    );

    // ── Throughput ────────────────────────────────────────────────────────────
    let throughput_per_week = compute_throughput_per_week(filter, merged.len());

    let mut throughput_by_milestone: HashMap<String, u32> = HashMap::new();
    for m in &merged {
        let key = m.milestone.clone().unwrap_or_default();
        *throughput_by_milestone.entry(key).or_insert(0) += 1;
    }

    // ── Diff & review quality ─────────────────────────────────────────────────
    let avg_diff_size = mean(latest_metrics.iter().map(|m| m.diff_size as f64));
    let avg_comments = mean(latest_metrics.iter().map(|m| m.user_notes_count as f64));

    let comment_densities: Vec<f64> = latest_metrics
        .iter()
        .filter_map(|m| m.comment_density)
        .collect();
    let avg_comment_density = if comment_densities.is_empty() {
        None
    } else {
        Some(comment_densities.iter().sum::<f64>() / comment_densities.len() as f64)
    };

    let failure_rates: Vec<f64> = latest_metrics
        .iter()
        .filter_map(|m| m.pipeline_failure_rate)
        .collect();
    let avg_pipeline_failure_rate = if failure_rates.is_empty() {
        None
    } else {
        Some(failure_rates.iter().sum::<f64>() / failure_rates.len() as f64)
    };
    let pipeline_sample_size = failure_rates.len();
    let pipeline_data_coverage = if latest_metrics.is_empty() {
        0.0
    } else {
        pipeline_sample_size as f64 / latest_metrics.len() as f64
    };
    let cycle_time_sample_size = cycle_times.len();
    let reviewer_coverage = coverage_ratio(latest_snapshots.iter(), |snap| {
        !snap.snapshot.reviewers.is_empty()
    });
    let milestone_coverage =
        coverage_ratio(latest_metrics.iter(), |metric| metric.milestone.is_some());
    let size_buckets = compute_size_buckets(&latest_metrics, &merged);

    // ── Backlog ages (open MRs) ───────────────────────────────────────────────
    let mut open_mr_ages_days: Vec<f64> = latest_snapshots
        .iter()
        .filter(|s| s.snapshot.state == "opened")
        .filter_map(|s| {
            let created: DateTime<Utc> = s.snapshot.created_at.as_deref()?.parse().ok()?;
            let age = now.signed_duration_since(created).num_seconds() as f64 / 86400.0;
            if age >= 0.0 {
                Some(age)
            } else {
                None
            }
        })
        .collect();
    open_mr_ages_days.sort_by(f64::total_cmp);
    let stale_open_mrs_7d = open_mr_ages_days.iter().filter(|age| **age >= 7.0).count();
    let stale_open_mrs_14d = open_mr_ages_days.iter().filter(|age| **age >= 14.0).count();
    let stale_open_mrs_30d = open_mr_ages_days.iter().filter(|age| **age >= 30.0).count();

    let terminal_count = merged.len() + closed_count;
    let abandon_rate = if terminal_count == 0 {
        None
    } else {
        Some(closed_count as f64 / terminal_count as f64)
    };

    AggregatedStats {
        total_mrs: latest_snapshots.len(),
        merged_count: merged.len(),
        closed_count,
        abandon_rate,
        merged_today: merged_period_counts.today,
        merged_this_week: merged_period_counts.this_week,
        merged_this_month: merged_period_counts.this_month,
        merged_last_7_days: merged_period_counts.last_7_days,
        merged_last_30_days: merged_period_counts.last_30_days,
        throughput_per_week,
        cycle_time_median_hours,
        cycle_time_p75_hours,
        cycle_time_p90_hours,
        cycle_time_by_author,
        cycle_time_by_reviewer,
        cycle_time_by_milestone,
        avg_diff_size,
        avg_comments,
        avg_comment_density,
        avg_pipeline_failure_rate,
        pipeline_data_coverage,
        cycle_time_sample_size,
        pipeline_sample_size,
        reviewer_coverage,
        milestone_coverage,
        size_buckets,
        open_mr_ages_days,
        stale_open_mrs_7d,
        stale_open_mrs_14d,
        stale_open_mrs_30d,
        throughput_by_milestone,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct MergedPeriodCounts {
    today: usize,
    this_week: usize,
    this_month: usize,
    last_7_days: usize,
    last_30_days: usize,
}

/// Counts merged MRs across common UTC periods using the actual GitLab merge date.
fn compute_merged_period_counts<'a, I>(snapshots: I, now: DateTime<Utc>) -> MergedPeriodCounts
where
    I: Iterator<Item = &'a StoredSnapshot> + DoubleEndedIterator,
{
    let today = now.date_naive();
    let current_iso_week = now.iso_week();
    let current_year = now.year();
    let current_month = now.month();
    let mut counts = MergedPeriodCounts::default();
    let mut seen = std::collections::HashSet::new();

    for snap in snapshots.rev() {
        if snap.snapshot.trigger != SnapshotTrigger::OnMerge
            || !seen.insert(snap.snapshot.mr_id.as_str())
        {
            continue;
        }

        let Some(merged_at) = snap
            .snapshot
            .merged_at
            .as_deref()
            .and_then(|date| date.parse::<DateTime<Utc>>().ok())
        else {
            continue;
        };

        if merged_at.date_naive() == today {
            counts.today += 1;
        }
        if merged_at.iso_week() == current_iso_week {
            counts.this_week += 1;
        }
        if merged_at.year() == current_year && merged_at.month() == current_month {
            counts.this_month += 1;
        }
        if merged_at >= now - Duration::days(7) {
            counts.last_7_days += 1;
        }
        if merged_at >= now - Duration::days(30) {
            counts.last_30_days += 1;
        }
    }

    counts
}

/// Computes the weekly throughput from the number of merged MRs and the query window.
fn compute_throughput_per_week(filter: &QueryFilter, merged_count: usize) -> Option<f64> {
    let window_days = match &filter.window {
        Some(TimeWindow::LastDays(d)) => *d as f64,
        Some(TimeWindow::Range { from, to }) => {
            let f: DateTime<Utc> = from.parse().ok()?;
            let t: DateTime<Utc> = to.parse().ok()?;
            t.signed_duration_since(f).num_days() as f64
        }
        // Milestone windows have no fixed duration.
        Some(TimeWindow::Milestone(_)) | None => return None,
    };
    if window_days <= 0.0 {
        return None;
    }
    Some(merged_count as f64 / window_days * 7.0)
}

/// Computes cycle-time stats for fixed MR diff-size buckets.
fn compute_size_buckets(
    latest_metrics: &[&PerMrMetrics],
    merged_metrics: &[&PerMrMetrics],
) -> Vec<MrSizeBucketStats> {
    const BUCKETS: [(&str, u32, Option<u32>); 4] = [
        ("Small", 0, Some(199)),
        ("Medium", 200, Some(799)),
        ("Large", 800, Some(1999)),
        ("Huge", 2000, None),
    ];

    BUCKETS
        .iter()
        .map(|(label, min, max)| {
            let includes = |diff_size: u32| -> bool {
                diff_size >= *min && max.map(|upper| diff_size <= upper).unwrap_or(true)
            };

            let total_mrs = latest_metrics
                .iter()
                .filter(|metric| includes(metric.diff_size))
                .count();
            let mut cycle_times = merged_metrics
                .iter()
                .filter(|metric| includes(metric.diff_size))
                .filter_map(|metric| metric.cycle_time_hours)
                .collect::<Vec<_>>();
            cycle_times.sort_by(f64::total_cmp);

            MrSizeBucketStats {
                label: (*label).to_string(),
                min_changed_lines: *min,
                max_changed_lines: *max,
                total_mrs,
                merged_mrs: cycle_times.len(),
                cycle_time_median_hours: percentile(&cycle_times, 50.0),
            }
        })
        .collect()
}

// ── Statistical helpers ───────────────────────────────────────────────────────

/// Returns the p-th percentile of an already-sorted slice, using linear interpolation.
fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    let n = sorted.len();
    if n == 0 {
        return None;
    }
    let idx = (p / 100.0) * (n - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - lo as f64;
    Some(sorted[lo] + frac * (sorted[hi] - sorted[lo]))
}

/// Computes the arithmetic mean of an iterator of `f64`.
/// Returns `0.0` when the iterator is empty.
fn mean(iter: impl Iterator<Item = f64>) -> f64 {
    let (sum, count) = iter.fold((0.0f64, 0usize), |(s, c), v| (s + v, c + 1));
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

/// Computes the share of items that satisfy a predicate.
fn coverage_ratio<'a, I, T, F>(iter: I, predicate: F) -> f64
where
    I: IntoIterator<Item = &'a T>,
    T: 'a,
    F: Fn(&T) -> bool,
{
    let (matched, total) = iter
        .into_iter()
        .fold((0usize, 0usize), |(matched, total), item| {
            (matched + usize::from(predicate(item)), total + 1)
        });
    if total == 0 {
        0.0
    } else {
        matched as f64 / total as f64
    }
}

/// Groups metrics by a string key and computes the **median** of a numeric field per group.
///
/// Median is preferred over mean for cycle-time breakdowns: a single long-running
/// MR (e.g. a multi-week feature branch) would otherwise dominate the average for
/// authors or milestones with few MRs, giving a misleading picture.
fn group_average<'a, I, K, V>(iter: I, key_fn: K, val_fn: V) -> HashMap<String, f64>
where
    I: Iterator<Item = &'a PerMrMetrics>,
    K: Fn(&PerMrMetrics) -> String,
    V: Fn(&PerMrMetrics) -> Option<f64>,
{
    let mut groups: HashMap<String, Vec<f64>> = HashMap::new();
    for m in iter {
        if let Some(v) = val_fn(m) {
            groups.entry(key_fn(m)).or_default().push(v);
        }
    }
    groups
        .into_iter()
        .map(|(k, mut vals)| {
            vals.sort_by(f64::total_cmp);
            let median = percentile(&vals, 50.0).unwrap_or(0.0);
            (k, median)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::MrStatsSnapshot;

    fn stored(
        id: i64,
        mr_id: &str,
        trigger: SnapshotTrigger,
        state: &str,
        lines: u32,
    ) -> StoredSnapshot {
        StoredSnapshot {
            id,
            recorded_at: format!("2024-01-{:02}T00:00:00Z", id),
            snapshot: MrStatsSnapshot {
                mr_id: mr_id.into(),
                project_id: "p".into(),
                title: "t".into(),
                trigger,
                author: "a".into(),
                assignee: None,
                reviewers: if mr_id == "1" {
                    vec!["r".into()]
                } else {
                    vec![]
                },
                merged_by: None,
                milestone: None,
                labels: vec![],
                target_branch: "main".into(),
                state: state.into(),
                created_at: Some("2024-01-01T00:00:00Z".into()),
                merged_at: (trigger == SnapshotTrigger::OnMerge)
                    .then(|| "2024-01-01T10:00:00Z".into()),
                updated_at: None,
                files_changed: 1,
                additions: lines,
                deletions: 0,
                commits_count: 1,
                diff_difficulty: None,
                user_notes_count: 2,
                pipeline_count: 0,
                pipeline_failure_count: 0,
            },
        }
    }

    #[test]
    fn latest_snapshot_per_mr_and_deduplicated_terminal_events() {
        use SnapshotTrigger::*;
        // MR 1: refreshed then merged twice (backfill duplicate). MR 2: refreshed, then closed.
        // MR 3: still open. Latest diff sizes: 1 → 300, 2 → 50, 3 → 2500.
        let snapshots = vec![
            stored(1, "1", OnRefresh, "opened", 100),
            stored(2, "2", OnRefresh, "opened", 50),
            stored(3, "1", OnMerge, "merged", 300),
            stored(4, "1", OnMerge, "merged", 300),
            stored(5, "3", OnRefresh, "opened", 2500),
            stored(6, "2", OnClose, "closed", 50),
        ];
        let metrics: Vec<PerMrMetrics> =
            snapshots.iter().map(PerMrMetrics::from_snapshot).collect();
        let stats = compute_stats(&snapshots, &metrics, &QueryFilter::default());

        assert_eq!(stats.total_mrs, 3);
        assert_eq!(stats.merged_count, 1);
        assert_eq!(stats.closed_count, 1);
        assert_eq!(stats.abandon_rate, Some(0.5));
        assert_eq!(stats.cycle_time_sample_size, 1);
        assert_eq!(stats.cycle_time_median_hours, Some(10.0));
        assert!((stats.avg_diff_size - (300.0 + 50.0 + 2500.0) / 3.0).abs() < 1e-9);
        assert!((stats.reviewer_coverage - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(stats.open_mr_ages_days.len(), 1);
        let totals: Vec<usize> = stats.size_buckets.iter().map(|b| b.total_mrs).collect();
        assert_eq!(totals, [1, 1, 0, 1]); // Small(50), Medium(300), Large, Huge(2500)
    }
}
