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

/// The final, fully-computed analytics report.
///
/// Serialises to JSON via `serde_json::to_string_pretty` for TUI consumption
/// or external tooling. The `to_csv_rows` method emits one line per metric
/// for spreadsheet import.
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

        Ok(Self::build_with_baseline(
            aggregated,
            baseline.as_ref(),
            &latest_metrics,
            filter,
        ))
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
