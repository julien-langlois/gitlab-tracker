use serde::{Deserialize, Serialize};

use crate::aggregator::{AggregatedStats, QueryFilter, TimeWindow};
use crate::correlation::{compute_all_correlations, CorrelationResult};
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
    /// Builds a [`StatReport`] from already-computed aggregated stats and raw metrics.
    ///
    /// Correlations and Poisson insights are computed here from the raw per-MR
    /// metrics so that the report is self-contained and reproducible from its inputs.
    pub fn build(
        aggregated: AggregatedStats,
        metrics: &[PerMrMetrics],
        filter: &QueryFilter,
    ) -> Self {
        let poisson = PoissonInsights::from_stats(&aggregated);
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
        ];

        if let Some(v) = self.aggregated.throughput_per_week {
            rows.push(format!("throughput_per_week,{v:.2}"));
        }
        if let Some(v) = self.aggregated.cycle_time_median_hours {
            rows.push(format!("cycle_time_median_hours,{v:.2}"));
        }
        if let Some(v) = self.aggregated.cycle_time_p90_hours {
            rows.push(format!("cycle_time_p90_hours,{v:.2}"));
        }

        rows.push(format!(
            "avg_diff_size,{:.1}",
            self.aggregated.avg_diff_size
        ));
        rows.push(format!("avg_comments,{:.2}", self.aggregated.avg_comments));

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
