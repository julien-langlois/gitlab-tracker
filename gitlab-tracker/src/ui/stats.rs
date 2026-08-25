//! Stats fullscreen overlay — renders the [`StatReport`] produced by `gitlab-tracker-stats`.
//!
//! # Layout (top → bottom)
//!
//! ```text
//! ┌─ Stats ─ Last 30 days ─ [W]: Change window ─ [↑/↓]: Scroll ─ [G/Esc]: Close ─┐
//! │  THROUGHPUT          CYCLE TIME                                                  │
//! │  23 merged           Median  18.4 h                                             │
//! │  2  closed           P90     61.2 h                                             │
//! │  2.1 MR/week         …                                                          │
//! │                                                                                  │
//! │  BY AUTHOR ────────────────────────────────────────────────────────────────────  │
//! │  alice       ████████████████████  42.1 h avg                                   │
//! │  bob         ████████             18.3 h avg                                    │
//! │                                                                                  │
//! │  CORRELATIONS ─────────────────────────────────────────────────────────────────  │
//! │  ● DiffSize ↔ CycleTime   ρ = 0.72  p = 0.001  ██  VeryStrong  (n=23)          │
//! │  ○ Comments ↔ CycleTime   ρ = 0.41  p = 0.048  █   Moderate    (n=21)          │
//! │  · DiffSize ↔ Comments    ρ = 0.18  p = 0.280      Weak        (n=23)          │
//! └────────────────────────────────────────────────────────────────────────────────  ┘
//! ```
//!
//! Correlations with `|ρ| < 0.10` or `p ≥ 0.10` are rendered dimmed.
//! Significant correlations (`|ρ| ≥ 0.30` and `p < 0.05`) get a coloured prefix bullet.

use gitlab_tracker_stats::correlation::{CorrelationResult, CorrelationStrength};
use gitlab_tracker_stats::StatReport;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::ui::theme;

/// Renders the Stats overlay as a fullscreen modal over the current terminal area.
///
/// Uses the same `Clear` + overlay pattern as the other popups in `ui/mod.rs`.
pub fn render_stats_overlay(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Clear, area);

    let view = &app.stats_view;
    let window_label = view.window.label();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(
            " Stats ─ {window_label} ─ [W]: Window ─ [↑/↓]: Scroll ─ [G/Esc]: Close "
        ));

    // ── Loading / error / empty states ───────────────────────────────────────
    if view.loading {
        f.render_widget(
            Paragraph::new("  ⟳  Computing statistics…")
                .block(block)
                .style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }

    if let Some(err) = &view.error {
        f.render_widget(
            Paragraph::new(format!("  ✘  {err}"))
                .block(block)
                .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    let Some(report) = &view.report else {
        f.render_widget(
            Paragraph::new(
                "  No stats available yet.\n\
                 \n\
                 Stats are recorded automatically as MRs are merged or refreshed.\n\
                 Open this view again after a few MR events have been processed.",
            )
            .block(block)
            .style(Style::default().fg(theme::MUTED))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };

    // ── Build the scrollable text content ────────────────────────────────────
    let lines = build_report_lines(report, area.width.saturating_sub(4));
    let total_lines = lines.len() as u16;

    // Clamp scroll so the last line is always visible.
    let inner_height = area.height.saturating_sub(2);
    let max_scroll = total_lines.saturating_sub(inner_height);
    app.stats_view.scroll = app.stats_view.scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.stats_view.scroll, 0));

    f.render_widget(paragraph, area);
}

/// Builds the full list of [`Line`]s that make up the report body.
///
/// Keeping this as a pure function (no `Frame` mutation) makes it trivially
/// testable and keeps rendering logic separate from layout logic.
fn build_report_lines(report: &StatReport, _content_width: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let agg = &report.aggregated;

    // ── Section: Throughput + Cycle time (side-by-side as two columns) ───────
    lines.push(section_header("THROUGHPUT & CYCLE TIME"));

    let ct_median = fmt_opt_hours(agg.cycle_time_median_hours);
    let ct_p90 = fmt_opt_hours(agg.cycle_time_p90_hours);
    let throughput = agg
        .throughput_per_week
        .map(|v| format!("{v:.1} MR/week"))
        .unwrap_or_else(|| "n/a".to_string());

    lines.push(kv_line(
        "Merged",
        &agg.merged_count.to_string(),
        Color::Green,
    ));
    lines.push(kv_line(
        "Closed",
        &agg.closed_count.to_string(),
        theme::MUTED,
    ));
    lines.push(kv_line("Throughput", &throughput, Color::Cyan));
    lines.push(kv_line("Cycle time median", &ct_median, Color::Yellow));
    lines.push(kv_line("Cycle time P90", &ct_p90, Color::Red));
    lines.push(kv_line(
        "Avg diff size",
        &format!("{:.0} lines", agg.avg_diff_size),
        theme::MUTED,
    ));
    lines.push(kv_line(
        "Avg comments",
        &format!("{:.1}", agg.avg_comments),
        theme::MUTED,
    ));
    if let Some(pfr) = agg.avg_pipeline_failure_rate {
        lines.push(kv_line(
            "Pipeline failure rate",
            &format!("{:.1}%", pfr * 100.0),
            if pfr > 0.3 { Color::Red } else { theme::MUTED },
        ));
    }
    lines.push(Line::from(""));

    // ── Section: Backlog health ───────────────────────────────────────────────
    if !agg.open_mr_ages_days.is_empty() {
        lines.push(section_header("OPEN MR BACKLOG"));
        let oldest = agg.open_mr_ages_days.last().copied().unwrap_or(0.0);
        let median_age = percentile_f64(&agg.open_mr_ages_days, 50.0).unwrap_or(0.0);
        lines.push(kv_line(
            "Open MRs",
            &agg.open_mr_ages_days.len().to_string(),
            theme::MUTED,
        ));
        lines.push(kv_line(
            "Median age",
            &format!("{:.0} days", median_age),
            if median_age > 14.0 {
                Color::Yellow
            } else {
                theme::MUTED
            },
        ));
        lines.push(kv_line(
            "Oldest",
            &format!("{:.0} days", oldest),
            if oldest > 30.0 {
                Color::Red
            } else {
                theme::MUTED
            },
        ));
        lines.push(Line::from(""));
    }

    // ── Section: Cycle time by author ─────────────────────────────────────────
    if !agg.cycle_time_by_author.is_empty() {
        lines.push(section_header("CYCLE TIME BY AUTHOR"));
        let mut by_author: Vec<(&String, f64)> = agg
            .cycle_time_by_author
            .iter()
            .map(|(k, v)| (k, *v))
            .collect();
        by_author.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let max_val = by_author.first().map(|(_, v)| *v).unwrap_or(1.0);
        for (author, hours) in &by_author {
            lines.push(bar_line(author, *hours, max_val, 24, Color::Cyan));
        }
        lines.push(Line::from(""));
    }

    // ── Section: Cycle time by reviewer ──────────────────────────────────────
    if !agg.cycle_time_by_reviewer.is_empty() {
        lines.push(section_header("CYCLE TIME BY REVIEWER (avg h waiting)"));
        let mut by_reviewer: Vec<(&String, f64)> = agg
            .cycle_time_by_reviewer
            .iter()
            .map(|(k, v)| (k, *v))
            .collect();
        by_reviewer.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let max_val = by_reviewer.first().map(|(_, v)| *v).unwrap_or(1.0);
        for (reviewer, hours) in &by_reviewer {
            lines.push(bar_line(reviewer, *hours, max_val, 24, Color::Magenta));
        }
        lines.push(Line::from(""));
    }

    // ── Section: Throughput by milestone ─────────────────────────────────────
    if !agg.throughput_by_milestone.is_empty() {
        lines.push(section_header("THROUGHPUT BY MILESTONE"));
        let mut by_ms: Vec<(&String, u32)> = agg
            .throughput_by_milestone
            .iter()
            .map(|(k, v)| (k, *v))
            .collect();
        by_ms.sort_by(|a, b| b.1.cmp(&a.1));
        let max_val = by_ms.first().map(|(_, v)| *v as f64).unwrap_or(1.0);
        for (ms, count) in &by_ms {
            lines.push(bar_line(ms, *count as f64, max_val, 24, Color::Green));
        }
        lines.push(Line::from(""));
    }

    // ── Section: Correlations ─────────────────────────────────────────────────
    lines.push(section_header(
        "SPEARMAN CORRELATIONS  (|ρ| ≥ 0.30 & p < 0.05 highlighted)",
    ));
    if report.correlations.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Not enough data (need ≥ 3 MRs per pair).",
            Style::default().fg(theme::MUTED),
        )));
    } else {
        let mut sorted = report.correlations.clone();
        // Sort by |ρ| descending so the most meaningful correlations appear first.
        sorted.sort_by(|a, b| {
            b.rho
                .abs()
                .partial_cmp(&a.rho.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for cr in &sorted {
            lines.push(correlation_line(cr));
        }
    }
    lines.push(Line::from(""));

    // ── Footer ────────────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled(
        format!(
            "  Generated {}  ·  {} MRs in sample",
            report.generated_at, agg.total_mrs
        ),
        Style::default().fg(theme::MUTED_DIM),
    )));

    lines
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

/// Renders a bold section separator line (e.g. `"BY AUTHOR ──────────────────"`).
fn section_header(title: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {title} "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "─".repeat(40usize.saturating_sub(title.len())),
            Style::default().fg(theme::MUTED_DIM),
        ),
    ])
}

/// Renders a `key  value` line with a fixed-width label column.
fn kv_line(key: &str, value: &str, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<26}", key), Style::default().fg(theme::MUTED)),
        Span::styled(value.to_string(), Style::default().fg(value_color)),
    ])
}

/// Renders a horizontal bar chart row.
///
/// `max_val` is the reference value (longest bar). `bar_width` is the maximum
/// number of `█` characters. Values are always shown numerically after the bar.
fn bar_line(
    label: &str,
    value: f64,
    max_val: f64,
    bar_width: usize,
    bar_color: Color,
) -> Line<'static> {
    let filled = if max_val > 0.0 {
        ((value / max_val) * bar_width as f64).round() as usize
    } else {
        0
    }
    .min(bar_width);

    let bar = "█".repeat(filled);
    let empty = " ".repeat(bar_width - filled);

    Line::from(vec![
        Span::styled(
            format!("  {:<18}", truncate(label, 17)),
            Style::default().fg(theme::MUTED),
        ),
        Span::styled(bar, Style::default().fg(bar_color)),
        Span::styled(empty, Style::default()),
        Span::styled(
            format!("  {:.1} h", value),
            Style::default().fg(theme::MUTED),
        ),
    ])
}

/// Renders one correlation row with a coloured significance bullet.
///
/// ● Cyan    = strong or very strong and significant (|ρ| ≥ 0.50, p < 0.05)
/// ● Yellow  = moderate and significant (0.30 ≤ |ρ| < 0.50, p < 0.05)
/// ○ Muted   = weak or not significant
/// · DimMuted = negligible
fn correlation_line(cr: &CorrelationResult) -> Line<'static> {
    let significant = cr.p_value < 0.05 && cr.rho.abs() >= 0.30;
    let (bullet, bullet_color) = if significant {
        match cr.interpretation {
            CorrelationStrength::VeryStrong | CorrelationStrength::Strong => ("●", Color::Cyan),
            CorrelationStrength::Moderate => ("●", Color::Yellow),
            _ => ("○", theme::MUTED),
        }
    } else if matches!(cr.interpretation, CorrelationStrength::Negligible) {
        ("·", theme::MUTED_DIM)
    } else {
        ("○", theme::MUTED)
    };

    let pair_label = pair_label(&cr.pair);
    let rho_color = rho_color(cr.rho, significant);
    let strength_label = strength_label(&cr.interpretation);
    let direction = if cr.rho > 0.0 { "↑↑" } else { "↑↓" };

    let text_style = if significant {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(theme::MUTED)
    };

    Line::from(vec![
        Span::styled(format!("  {bullet} "), Style::default().fg(bullet_color)),
        Span::styled(format!("{:<32}", pair_label), text_style),
        Span::styled(
            format!("ρ = {:+.3}  ", cr.rho),
            Style::default().fg(rho_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("p = {:.3}  ", cr.p_value),
            Style::default().fg(if cr.p_value < 0.05 {
                theme::MUTED
            } else {
                theme::MUTED_DIM
            }),
        ),
        Span::styled(format!("{direction}  {:<12}", strength_label), text_style),
        Span::styled(
            format!("(n={})", cr.sample_size),
            Style::default().fg(theme::MUTED_DIM),
        ),
    ])
}

/// Maps a [`MetricPair`] to a concise human-readable label.
fn pair_label(pair: &gitlab_tracker_stats::MetricPair) -> &'static str {
    use gitlab_tracker_stats::MetricPair;
    match pair {
        MetricPair::DiffSizeVsCycleTime => "Diff size  ↔  Cycle time",
        MetricPair::DiffSizeVsComments => "Diff size  ↔  Comments",
        MetricPair::CommentsVsCycleTime => "Comments   ↔  Cycle time",
        MetricPair::PipelineFailuresVsCycleTime => "Pipeline failures  ↔  Cycle time",
        MetricPair::CommitsCountVsCycleTime => "Commits  ↔  Cycle time",
        MetricPair::DiffDifficultyVsCycleTime => "Diff difficulty  ↔  Cycle time",
    }
}

/// Chooses a colour for the ρ value based on magnitude and significance.
fn rho_color(rho: f64, significant: bool) -> Color {
    if !significant {
        return theme::MUTED_DIM;
    }
    let abs = rho.abs();
    if abs >= 0.70 {
        Color::Cyan
    } else if abs >= 0.50 {
        Color::Green
    } else if abs >= 0.30 {
        Color::Yellow
    } else {
        theme::MUTED
    }
}

/// Short label for a [`CorrelationStrength`] variant.
fn strength_label(s: &CorrelationStrength) -> &'static str {
    match s {
        CorrelationStrength::Negligible => "Negligible",
        CorrelationStrength::Weak => "Weak",
        CorrelationStrength::Moderate => "Moderate",
        CorrelationStrength::Strong => "Strong",
        CorrelationStrength::VeryStrong => "Very strong",
    }
}

// ── Statistical helpers ───────────────────────────────────────────────────────

/// Linear-interpolation percentile on a pre-sorted slice.
fn percentile_f64(sorted: &[f64], p: f64) -> Option<f64> {
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

/// Truncates a string to `max_chars` characters, appending `…` when truncated.
fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        format!("{truncated}…")
    }
}

/// Formats an `Option<f64>` hours value for display (e.g. `"18.4 h"` or `"n/a"`).
fn fmt_opt_hours(v: Option<f64>) -> String {
    v.map(|h| format!("{h:.1} h"))
        .unwrap_or_else(|| "n/a".to_string())
}

// ── Public API re-exported from the overlay module ────────────────────────────

/// Called by `events.rs` to trigger an async stats recomputation.
///
/// Marks `stats_view.loading = true` immediately so the overlay shows a spinner,
/// then spawns a Tokio task that queries the DB and sends the result back via
/// `AppEvent::StatsReportReady`. This keeps the main event loop non-blocking.
pub fn trigger_stats_refresh(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<crate::models::AppEvent>,
) {
    #[cfg(feature = "stats")]
    {
        use gitlab_tracker_stats::aggregator::{aggregate, QueryFilter};

        let Some(db) = app.stats_db.clone() else {
            app.stats_view.report = None;
            app.stats_view.error = Some("Stats DB not available — check startup logs.".to_string());
            return;
        };

        app.stats_view.loading = true;
        app.stats_view.error = None;
        app.stats_view.scroll = 0;

        let window = app.stats_view.window.to_query_window();
        let project_id = app.project_id.clone();
        let tx2 = tx.clone();

        tokio::spawn(async move {
            let filter = QueryFilter {
                window,
                project_id: Some(project_id),
                ..Default::default()
            };

            match aggregate(db.as_ref(), &filter).await {
                Ok(agg) => {
                    // Derive per-MR metrics for correlation computation.
                    // We re-query with the same filter to get the raw snapshots.
                    // A future optimisation could have `aggregate` return them directly.
                    use gitlab_tracker_stats::db::StatsDb;
                    use gitlab_tracker_stats::metrics::PerMrMetrics;

                    let query = gitlab_tracker_stats::db::SnapshotQuery {
                        project_id: filter.project_id.clone(),
                        ..Default::default()
                    };
                    let metrics = match db.query(&query).await {
                        Ok(snaps) => snaps
                            .iter()
                            .map(PerMrMetrics::from_snapshot)
                            .collect::<Vec<_>>(),
                        Err(_) => vec![],
                    };

                    let report = gitlab_tracker_stats::StatReport::build(agg, &metrics, &filter);
                    let _ = tx2.send(crate::models::AppEvent::StatsReportReady(Box::new(report)));
                }
                Err(e) => {
                    let _ = tx2.send(crate::models::AppEvent::StatsReportFailed(e.to_string()));
                }
            }
        });
    }
}
