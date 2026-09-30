use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::aggregator::latest_per_mr;
use crate::aggregator::{
    aggregate, build_snapshot_query, compute_stats, AggregatedStats, QueryFilter, TimeWindow,
};
use crate::correlation::{compute_all_correlations, CorrelationResult};
use crate::db::{StatsDb, StatsError};
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
    pub async fn load(db: &dyn StatsDb, filter: &QueryFilter) -> Result<Self, StatsError> {
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

    /// Builds a [`StatReport`] from already-computed aggregated stats and raw metrics.
    ///
    /// Correlations and Poisson insights are computed here from the raw per-MR
    /// metrics so that the report is self-contained and reproducible from its inputs.
    pub fn build(
        aggregated: AggregatedStats,
        metrics: &[PerMrMetrics],
        filter: &QueryFilter,
    ) -> Self {
        Self::build_with_baseline(aggregated, None, metrics, filter)
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

    /// Serialises the report to a pretty-printed JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Emits a flat CSV representation of the aggregated scalars.
    ///
    /// Each row is `metric_name,value` — suitable for piping into a spreadsheet
    /// or a future CLI `--output csv` flag. Grouped metrics (by_author, etc.)
    /// are expanded with a `metric[key]` column name convention.
    pub fn to_csv_rows(&self) -> Vec<String> {
        let mut rows = vec![
            format!("generated_at,{}", self.generated_at),
            format!("window,\"{}\"", self.window_label),
            format!("total_mrs,{}", self.aggregated.total_mrs),
            format!("merged_count,{}", self.aggregated.merged_count),
            format!("closed_count,{}", self.aggregated.closed_count),
            format!(
                "pipeline_data_coverage,{:.4}",
                self.aggregated.pipeline_data_coverage
            ),
            format!(
                "cycle_time_sample_size,{}",
                self.aggregated.cycle_time_sample_size
            ),
            format!(
                "pipeline_sample_size,{}",
                self.aggregated.pipeline_sample_size
            ),
            format!("reviewer_coverage,{:.4}", self.aggregated.reviewer_coverage),
            format!(
                "milestone_coverage,{:.4}",
                self.aggregated.milestone_coverage
            ),
            format!("stale_open_mrs_7d,{}", self.aggregated.stale_open_mrs_7d),
            format!("stale_open_mrs_14d,{}", self.aggregated.stale_open_mrs_14d),
            format!("stale_open_mrs_30d,{}", self.aggregated.stale_open_mrs_30d),
        ];

        if let Some(v) = self.aggregated.abandon_rate {
            rows.push(format!("abandon_rate,{v:.4}"));
        }
        if let Some(v) = self.aggregated.throughput_per_week {
            rows.push(format!("throughput_per_week,{v:.2}"));
        }
        if let Some(v) = self.aggregated.cycle_time_median_hours {
            rows.push(format!("cycle_time_median_hours,{v:.2}"));
        }
        if let Some(v) = self.aggregated.cycle_time_p75_hours {
            rows.push(format!("cycle_time_p75_hours,{v:.2}"));
        }
        if let Some(v) = self.aggregated.cycle_time_p90_hours {
            rows.push(format!("cycle_time_p90_hours,{v:.2}"));
        }

        rows.push(format!(
            "avg_diff_size,{:.1}",
            self.aggregated.avg_diff_size
        ));
        rows.push(format!("avg_comments,{:.2}", self.aggregated.avg_comments));

        if let Some(v) = self.aggregated.avg_comment_density {
            rows.push(format!("avg_comment_density,{v:.4}"));
        }
        if let Some(v) = self.aggregated.avg_pipeline_failure_rate {
            rows.push(format!("avg_pipeline_failure_rate,{:.4}", v));
        }

        for (author, ct) in &self.aggregated.cycle_time_by_author {
            rows.push(format!("cycle_time_by_author[{author}],{ct:.2}"));
        }
        for (reviewer, ct) in &self.aggregated.cycle_time_by_reviewer {
            rows.push(format!("cycle_time_by_reviewer[{reviewer}],{ct:.2}"));
        }
        for (milestone, count) in &self.aggregated.throughput_by_milestone {
            rows.push(format!("throughput_by_milestone[{milestone}],{count}"));
        }

        rows.push(String::from(
            "# size_buckets: label,min_changed_lines,max_changed_lines,total_mrs,merged_mrs,cycle_time_median_hours",
        ));
        for bucket in &self.aggregated.size_buckets {
            let max = bucket
                .max_changed_lines
                .map(|value| value.to_string())
                .unwrap_or_else(|| "".to_string());
            let median = bucket
                .cycle_time_median_hours
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "".to_string());
            rows.push(format!(
                "size_bucket,{},{},{},{},{},{}",
                bucket.label,
                bucket.min_changed_lines,
                max,
                bucket.total_mrs,
                bucket.merged_mrs,
                median
            ));
        }

        // Correlations section.
        rows.push(String::from(
            "# correlations: pair,rho,p_value,sample_size,strength",
        ));
        for cr in &self.correlations {
            rows.push(format!(
                "correlation[{:?}],{:.4},{:.4},{},{:?}",
                cr.pair, cr.rho, cr.p_value, cr.sample_size, cr.interpretation
            ));
        }

        rows
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
        Some(TimeWindow::Milestone(_)) | None => None,
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
        Some(TimeWindow::Milestone(m)) => format!("milestone \"{m}\""),
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
