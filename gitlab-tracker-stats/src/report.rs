use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::aggregator::latest_per_mr;
use crate::aggregator::{
    aggregate, build_snapshot_query, compute_stats, AggregatedStats, QueryFilter, TimeWindow,
};
use crate::correlation::{compute_all_correlations, CorrelationResult};
use crate::db::{SqliteStatsDb, StatsError};
use crate::metrics::PerMrMetrics;
use crate::poisson::PoissonInsights;
use crate::promotion::{lead_times, PromotionLeadTime};

/// The final, fully-computed analytics report.
///
/// Serialises to JSON via `serde_json::to_string_pretty` for TUI consumption
/// or external tooling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatReport {
    /// ISO 8601 timestamp when this report was generated.
    pub generated_at: String,

    /// Human-readable description of the time window and filters applied.
    pub window_label: String,

    /// High-level aggregated statistics.
    pub aggregated: AggregatedStats,

    /// Spearman correlations between all defined metric pairs.
    pub correlations: Vec<CorrelationResult>,

    /// Poisson-based forecasts, anomaly signals, and queue insights.
    pub poisson: PoissonInsights,

    /// Promotion lead time per branch (arrivals within the window). Empty when no
    /// project is selected or nothing was promoted yet.
    #[serde(default)]
    pub promotions: Vec<PromotionLeadTime>,
}

impl StatReport {
    /// Loads all required data from the stats store and builds a complete report.
    pub async fn load(db: &SqliteStatsDb, filter: &QueryFilter) -> Result<Self, StatsError> {
        // The current window is loaded once and shared by the aggregation and the
        // correlations; only the baseline needs its own query.
        let snapshots = db.query(&build_snapshot_query(filter)).await?;
        let metrics = snapshots
            .iter()
            .map(PerMrMetrics::from_snapshot)
            .collect::<Vec<_>>();
        let aggregated = compute_stats(&snapshots, &metrics, filter);
        // Correlations use one point per MR (its latest snapshot): an MR open for 30
        // days has 30 daily refresh snapshots, and counting each of them would weigh
        // it like 30 MRs and inflate n (pseudo-replication), crushing the p-values.
        let latest_metrics: Vec<PerMrMetrics> = latest_per_mr(&snapshots)
            .into_iter()
            .map(|i| metrics[i].clone())
            .collect();
        let baseline = match build_baseline_filter(filter) {
            Some(baseline_filter) => Some(aggregate(db, &baseline_filter).await?),
            None => None,
        };

        let window = build_snapshot_query(filter);
        let promotions = match &filter.project_id {
            Some(project_id) => lead_times(
                &db.query_branch_arrivals(
                    project_id,
                    window.from_date.as_deref(),
                    window.to_date.as_deref(),
                )
                .await?,
            ),
            None => Vec::new(),
        };

        Ok(Self {
            promotions,
            ..Self::build_with_baseline(aggregated, baseline.as_ref(), &latest_metrics, filter)
        })
    }

    /// Builds a [`StatReport`] using an optional historical baseline for anomaly detection.
    pub fn build_with_baseline(
        aggregated: AggregatedStats,
        baseline: Option<&AggregatedStats>,
        metrics: &[PerMrMetrics],
        filter: &QueryFilter,
    ) -> Self {
        let sprint_weeks = filter.sprint_weeks.unwrap_or(2);
        let poisson =
            PoissonInsights::from_stats_with_baseline(&aggregated, baseline, sprint_weeks);
        Self {
            generated_at: chrono::Utc::now().to_rfc3339(),
            window_label: describe_window(filter),
            aggregated,
            correlations: compute_all_correlations(metrics),
            poisson,
            promotions: Vec::new(),
        }
    }
}

/// Builds a previous-period baseline filter for rolling and explicit date windows.
fn build_baseline_filter(filter: &QueryFilter) -> Option<QueryFilter> {
    let baseline_window = match &filter.window {
        Some(TimeWindow::LastDays(days)) => {
            let now = Utc::now();
            let current_start = now.checked_sub_signed(Duration::days(*days as i64))?;
            let baseline_start = current_start.checked_sub_signed(Duration::days(*days as i64))?;
            Some(TimeWindow::Range {
                from: baseline_start.to_rfc3339(),
                to: current_start.to_rfc3339(),
            })
        }
        Some(TimeWindow::Range { from, to }) => {
            let from_dt: DateTime<Utc> = from.parse().ok()?;
            let to_dt: DateTime<Utc> = to.parse().ok()?;
            let duration = to_dt.signed_duration_since(from_dt);
            if duration <= Duration::zero() {
                return None;
            }
            let baseline_start = from_dt.checked_sub_signed(duration)?;
            Some(TimeWindow::Range {
                from: baseline_start.to_rfc3339(),
                to: from_dt.to_rfc3339(),
            })
        }
        None => None,
    }?;

    Some(QueryFilter {
        window: Some(baseline_window),
        project_id: filter.project_id.clone(),
        author: filter.author.clone(),
        reviewer: filter.reviewer.clone(),
        target_branch: filter.target_branch.clone(),
        sprint_weeks: filter.sprint_weeks,
    })
}

/// Produces a human-readable label describing the active query window and filters.
fn describe_window(filter: &QueryFilter) -> String {
    let window_part = match &filter.window {
        Some(TimeWindow::LastDays(d)) => format!("last {d} days"),
        Some(TimeWindow::Range { from, to }) => format!("{from} → {to}"),
        None => "all time".to_string(),
    };

    let mut parts = vec![window_part];
    if let Some(p) = &filter.project_id {
        parts.push(format!("project={p}"));
    }
    if let Some(a) = &filter.author {
        parts.push(format!("author={a}"));
    }
    if let Some(r) = &filter.reviewer {
        parts.push(format!("reviewer={r}"));
    }
    if let Some(b) = &filter.target_branch {
        parts.push(format!("branch={b}"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promotion::{ArrivalKind, BranchArrival};

    fn filter(window: Option<TimeWindow>) -> QueryFilter {
        QueryFilter {
            window,
            ..QueryFilter::default()
        }
    }

    fn range(from: &str, to: &str) -> Option<TimeWindow> {
        Some(TimeWindow::Range {
            from: from.into(),
            to: to.into(),
        })
    }

    fn bounds(filter: &QueryFilter) -> (DateTime<Utc>, DateTime<Utc>) {
        match &filter.window {
            Some(TimeWindow::Range { from, to }) => (from.parse().unwrap(), to.parse().unwrap()),
            other => panic!("expected an explicit range, got {other:?}"),
        }
    }

    #[test]
    fn window_label_lists_the_window_then_the_active_filters() {
        assert_eq!(describe_window(&filter(None)), "all time");
        assert_eq!(
            describe_window(&filter(Some(TimeWindow::LastDays(30)))),
            "last 30 days"
        );
        let full = QueryFilter {
            window: range("2026-01-01", "2026-02-01"),
            project_id: Some("42".into()),
            author: Some("alice".into()),
            reviewer: Some("bob".into()),
            target_branch: Some("main".into()),
            sprint_weeks: Some(3),
        };
        assert_eq!(
            describe_window(&full),
            "2026-01-01 → 2026-02-01, project=42, author=alice, reviewer=bob, branch=main"
        );
    }

    #[test]
    fn baseline_is_the_period_of_equal_length_just_before_the_window() {
        // Explicit range: the same duration, ending where the window starts.
        let current = QueryFilter {
            project_id: Some("42".into()),
            author: Some("alice".into()),
            sprint_weeks: Some(3),
            ..filter(range("2026-03-01T00:00:00Z", "2026-03-11T00:00:00Z"))
        };
        let baseline = build_baseline_filter(&current).unwrap();
        let (from, to) = bounds(&baseline);
        assert_eq!(from.to_rfc3339(), "2026-02-19T00:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2026-03-01T00:00:00+00:00");
        // Compared like for like: every other filter is carried over.
        assert_eq!(baseline.project_id.as_deref(), Some("42"));
        assert_eq!(baseline.author.as_deref(), Some("alice"));
        assert_eq!(baseline.sprint_weeks, Some(3));

        // Rolling window: [now − 2n, now − n].
        let rolling = build_baseline_filter(&filter(Some(TimeWindow::LastDays(30)))).unwrap();
        let (from, to) = bounds(&rolling);
        assert_eq!((to - from).num_days(), 30);
        let age = Utc::now() - to;
        assert!((age - Duration::days(30)).num_seconds().abs() < 60, "{age}");
    }

    #[test]
    fn no_baseline_without_a_usable_window() {
        // "All time" has nothing before it.
        assert!(build_baseline_filter(&filter(None)).is_none());
        // Empty, reversed or unparseable ranges.
        let same = "2026-03-01T00:00:00Z";
        assert!(build_baseline_filter(&filter(range(same, same))).is_none());
        assert!(build_baseline_filter(&filter(range("2026-03-11T00:00:00Z", same))).is_none());
        assert!(build_baseline_filter(&filter(range("yesterday", same))).is_none());
    }

    #[tokio::test]
    async fn empty_store_yields_an_empty_report_not_an_error() {
        let db = SqliteStatsDb::open(":memory:").await.unwrap();
        for window in [None, Some(TimeWindow::LastDays(30))] {
            let query = QueryFilter {
                project_id: Some("42".into()),
                ..filter(window)
            };
            let report = StatReport::load(&db, &query).await.unwrap();
            assert_eq!(report.aggregated.total_mrs, 0);
            assert!(report.correlations.is_empty() && report.promotions.is_empty());
            assert!(report.window_label.ends_with("project=42"));
            assert!(report.generated_at.parse::<DateTime<Utc>>().is_ok());
        }
    }

    #[tokio::test]
    async fn promotions_are_scoped_to_the_project_and_the_window() {
        let db = SqliteStatsDb::open(":memory:").await.unwrap();
        let arrival = |project: &str, mr: &str, days_ago: i64| BranchArrival {
            project_id: project.into(),
            mr_id: mr.into(),
            branch: "production".into(),
            arrived_at: Utc::now() - Duration::days(days_ago),
            how: ArrivalKind::CherryPick,
            merged_at: Some((Utc::now() - Duration::days(days_ago + 2)).to_rfc3339()),
        };
        for item in [
            arrival("42", "1", 5),
            arrival("42", "2", 60),
            arrival("7", "3", 5),
        ] {
            assert!(db.record_branch_arrival(&item).await.unwrap());
        }
        let load = |project: Option<&str>, window| {
            let query = QueryFilter {
                project_id: project.map(str::to_string),
                ..filter(window)
            };
            let db = &db;
            async move { StatReport::load(db, &query).await.unwrap().promotions }
        };

        let all_time = load(Some("42"), None).await;
        assert_eq!(all_time.len(), 1);
        assert_eq!(
            (all_time[0].branch.as_str(), all_time[0].arrivals),
            ("production", 2)
        );
        assert_eq!(all_time[0].copied_share, 1.0);
        // Merged two days before reaching production.
        let median = all_time[0].median_hours.unwrap();
        assert!((median - 48.0).abs() < 0.1, "{median}");

        // The 60-day-old arrival is outside a 30-day window.
        assert_eq!(
            load(Some("42"), Some(TimeWindow::LastDays(30))).await[0].arrivals,
            1
        );
        // Arrivals are per project: none is shown without one.
        assert!(load(None, None).await.is_empty());
        assert!(load(Some("404"), None).await.is_empty());
    }
}
