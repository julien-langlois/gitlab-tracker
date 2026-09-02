//! Stats fullscreen overlay — renders the [`StatReport`] produced by `gitlab-tracker-stats`.
//!
//! # Layout
//!
//! ```text
//! ┌─ Stats ─ Last 30 days ─ [W] Window ─ [↑/↓] Scroll ─ [Esc] Close ──────────┐
//! │ ┌─ Throughput ────────────┐  ┌─ Cycle Time ───────────────────────────────┐ │
//! │ │  23 merged  2.1 MR/week │  │ Median ██████████░░░░░░░░░░  18.4 h       │ │
//! │ │   2 closed              │  │ P90    ████████████████████  61.2 h       │ │
//! │ └─────────────────────────┘  └───────────────────────────────────────────┘ │
//! │ ┌─ Backlog Health ────────┐  ┌─ Poisson · Queue · Anomalies ─────────────┐ │
//! │ │  Open: 5  Median: 7d   │  │  ρ=0.45  Wait 12h  ≥10 in 2w → 82%      │ │
//! │ └─────────────────────────┘  └───────────────────────────────────────────┘ │
//! │ ┌─ Cycle time by Author ──┐  ┌─ Cycle time by Reviewer ──────────────────┐ │
//! │ │ alice ████████  42.1 h  │  │ bob  ██████████  18.3 h                  │ │
//! │ └─────────────────────────┘  └───────────────────────────────────────────┘ │
//! │ ┌─ Spearman Correlations ──────────────────────────────────────────────────┐ │
//! │ │ ● Diff size ↔ Cycle time   ρ=+0.72  p=0.001  ██ Very strong  (n=23)   │ │
//! │ └──────────────────────────────────────────────────────────────────────────┘ │
//! │  Generated 2025-01-01  ·  23 MRs in sample                                  │
//! └──────────────────────────────────────────────────────────────────────────────┘
//! ```

use gitlab_tracker_stats::correlation::{CorrelationResult, CorrelationStrength};
use gitlab_tracker_stats::poisson::{AnomalySeverity, AnomalySignal, ThroughputForecast};
use gitlab_tracker_stats::StatReport;
use ratatui::{
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::app::App;
use crate::ui::theme;

/// Renders the Stats overlay as a fullscreen multi-block dashboard.
pub fn render_stats_overlay(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(Clear, area);

    let view = &app.stats_view;
    let window_label = view.window.label();

    // ── Outer border with title bar ───────────────────────────────────────────
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" Stats ─ {window_label} "));

    // Loading / error / empty states are rendered inside the outer block.
    if view.loading {
        f.render_widget(
            Paragraph::new("  ⟳  Computing statistics…")
                .block(outer_block)
                .style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }

    if let Some(err) = &view.error {
        f.render_widget(
            Paragraph::new(format!("  ✘  {err}"))
                .block(outer_block)
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
            .block(outer_block)
            .style(Style::default().fg(theme::MUTED))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };

    // Render the outer frame first, then compute inner area for content.
    f.render_widget(outer_block, area);
    let inner = inner_area(area);

    // ── Status bar area (bottom, fixed 3 lines like the main input bar) ───────
    // Split inner vertically: scrollable content on top, fixed status bar at bottom.
    let [content_area, status_area] = {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(inner);
        [chunks[0], chunks[1]]
    };

    // ── Vertical bands ────────────────────────────────────────────────────────
    // Band 0: Throughput | Cycle Time        (fixed 7 lines)
    // Band 1: Backlog    | Poisson/Queue     (dynamic, min 7)
    // Band 2: By Author  | By Reviewer       (dynamic, min 5)
    // Band 3: Correlations                   (dynamic)
    // Band 4: Footer                         (scrollable forecasts only)
    let agg = &report.aggregated;

    let author_rows = agg.cycle_time_by_author.len().max(1) as u16 + 2; // +2 for block borders
    let reviewer_rows = agg.cycle_time_by_reviewer.len().max(1) as u16 + 2;
    let milestone_rows = agg.cycle_time_by_milestone.len().max(1) as u16 + 2;
    let bar_band_h = author_rows.max(reviewer_rows).max(milestone_rows).max(5);
    let corr_rows = report.correlations.len().max(1) as u16 + 2;

    let bands = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),          // Band 0: Throughput + Cycle Time
            Constraint::Length(7),          // Band 1: Backlog + Poisson
            Constraint::Length(bar_band_h), // Band 2: By Author + By Reviewer
            Constraint::Length(corr_rows),  // Band 3: Correlations
            Constraint::Min(1),             // Band 4: Scrollable forecasts
        ])
        .split(content_area);

    // ── Band 0: Throughput | Cycle Time ───────────────────────────────────────
    let [left0, right0] = split_horizontal(bands[0], 50);
    render_throughput_block(f, report, left0);
    render_cycle_time_block(f, report, right0);

    // ── Band 1: Backlog Health | Poisson Queue + Anomalies ────────────────────
    let [left1, right1] = split_horizontal(bands[1], 35);
    render_backlog_block(f, report, left1);
    render_poisson_summary_block(f, report, right1);

    // ── Band 2: By Author | By Reviewer | By Milestone ───────────────────────
    let [left2, mid2, right2] = split_horizontal_thirds(bands[2]);
    render_by_author_block(f, report, left2);
    render_by_reviewer_block(f, report, mid2);
    render_by_milestone_block(f, report, right2);

    // ── Band 3: Correlations ──────────────────────────────────────────────────
    render_correlations_block(f, report, bands[3]);

    // ── Band 4: Scrollable forecasts ──────────────────────────────────────────
    // Extract all data needed after the borrow ends while `report` and `agg` are
    // still in scope, then assign scroll back once the immutable borrow is released.
    let current_scroll = app.stats_view.scroll;
    let generated_at = report.generated_at.clone();
    let total_mrs = agg.total_mrs;
    let window_lbl = window_label.to_string();
    let clamped_scroll = render_footer_band(f, report, bands[4], current_scroll);
    // `report` / `agg` / `view` borrows end here — safe to mutably write scroll back.
    app.stats_view.scroll = clamped_scroll;

    // ── Fixed status bar (full width, like the main input bar) ────────────────
    let status_bar = Paragraph::new(format!(
        " Generated {generated_at}  ·  {total_mrs} MRs in sample  ·  Window: {window_lbl}"
    ))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::MUTED_DIM))
            .title(Span::styled(
                " STATS │ [W]: Window │ [R]: Refresh │ [↑/↓]: Scroll │ [G/Esc]: Close ",
                Style::default().fg(Color::Cyan),
            )),
    )
    .style(Style::default().fg(theme::MUTED_DIM));
    f.render_widget(status_bar, status_area);
}

// ── Block renderers ───────────────────────────────────────────────────────────

/// Throughput numbers + pipeline failure rate.
fn render_throughput_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;
    let throughput = agg
        .throughput_per_week
        .map(|v| format!("{v:.1} MR/week"))
        .unwrap_or_else(|| "n/a".to_string());

    let mut lines = vec![
        kv_line("Merged", &agg.merged_count.to_string(), Color::Green),
        kv_line("Closed", &agg.closed_count.to_string(), theme::MUTED),
        kv_line("Throughput", &throughput, Color::Cyan),
        kv_line(
            "Avg diff size",
            &format!("{:.0} lines", agg.avg_diff_size),
            theme::MUTED,
        ),
        kv_line(
            "Avg comments",
            &format!("{:.1}", agg.avg_comments),
            theme::MUTED,
        ),
    ];
    if let Some(pfr) = agg.avg_pipeline_failure_rate {
        lines.push(kv_line(
            "Pipeline fail rate",
            &format!("{:.1}%", pfr * 100.0),
            if pfr > 0.3 { Color::Red } else { theme::MUTED },
        ));
    }

    f.render_widget(
        Paragraph::new(lines)
            .block(styled_block(" Throughput "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Cycle time displayed as Gauge bars for median and P90.
fn render_cycle_time_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;

    let median = agg.cycle_time_median_hours.unwrap_or(0.0);
    let p75 = agg.cycle_time_p75_hours.unwrap_or(0.0);
    let p90 = agg.cycle_time_p90_hours.unwrap_or(0.0);
    let max = p90.max(1.0);

    // We split the inner area into rows for each gauge + a summary line.
    let block = styled_block(" Cycle Time (created → merged) ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 5 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Median gauge
            Constraint::Length(1), // P75 gauge
            Constraint::Length(1), // P90 gauge
            Constraint::Length(1), // empty spacer
            Constraint::Min(0),    // summary line
        ])
        .split(inner);

    // Label style: white + bold so the text stays readable on both the filled
    // (coloured) and unfilled (DarkGray) portions of the gauge bar.
    let label_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    // Build the value strings independently so we can measure their widths and
    // pad every label to the same total length. This guarantees that ratatui
    // centres all three gauge labels at the exact same horizontal position,
    // keeping both the name column (left) and the value column (right) aligned.
    //
    // Layout per row:  "Median  " + right-aligned hours + " h " + days
    //                  "P75     " + right-aligned hours + " h " + days
    //                  "P90     " + right-aligned hours + " h " + days
    let fmt_value = |hours: f64| -> String {
        let days = hours / 24.0;
        format!("{:.1} h ({:.1}d)", hours, days)
    };

    let median_val = fmt_value(median);
    let p75_val = fmt_value(p75);
    let p90_val = fmt_value(p90);

    // Determine the width of the widest value string so all can be right-padded
    // to the same length, making the trailing characters line up column-wise.
    let val_width = median_val.len().max(p75_val.len()).max(p90_val.len());

    // Fixed name column: 6 chars ("Median" is the longest label).
    // Two spaces separate name from value.
    let fmt_gauge_label = |name: &str, val: &str| -> String {
        // Right-pad the value string so every label has identical total length.
        format!("{:<6}  {:<width$}", name, val, width = val_width)
    };

    // Median (P50) gauge
    let median_ratio = (median / max).clamp(0.0, 1.0);
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
            .ratio(median_ratio)
            .label(Span::styled(
                fmt_gauge_label("Median", &median_val),
                label_style,
            )),
        rows[0],
    );

    // P75 gauge
    let p75_ratio = (p75 / max).clamp(0.0, 1.0);
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .ratio(p75_ratio)
            .label(Span::styled(fmt_gauge_label("P75", &p75_val), label_style)),
        rows[1],
    );

    // P90 gauge
    let p90_ratio = (p90 / max).clamp(0.0, 1.0);
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Red).bg(Color::DarkGray))
            .ratio(p90_ratio)
            .label(Span::styled(fmt_gauge_label("P90", &p90_val), label_style)),
        rows[2],
    );

    // Summary line: spread between median and P90
    let spread = p90 - median;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Spread  ", Style::default().fg(theme::MUTED)),
            Span::styled(
                format!("{:>5.1} h ({:.1}d)  (P90 − Median)", spread, spread / 24.0),
                Style::default().fg(theme::MUTED_DIM),
            ),
        ])),
        rows[4],
    );
}

/// Open MR backlog health summary.
fn render_backlog_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;

    if agg.open_mr_ages_days.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No open MRs.",
                Style::default().fg(theme::MUTED),
            )))
            .block(styled_block(" Backlog Health ")),
            area,
        );
        return;
    }

    let oldest = agg.open_mr_ages_days.last().copied().unwrap_or(0.0);
    let median_age = percentile_f64(&agg.open_mr_ages_days, 50.0).unwrap_or(0.0);

    let lines = vec![
        kv_line(
            "Open MRs",
            &agg.open_mr_ages_days.len().to_string(),
            theme::MUTED,
        ),
        kv_line(
            "Median age",
            &format!("{:.0} days", median_age),
            if median_age > 14.0 {
                Color::Yellow
            } else {
                theme::MUTED
            },
        ),
        kv_line(
            "Oldest",
            &format!("{:.0} days", oldest),
            if oldest > 30.0 {
                Color::Red
            } else {
                theme::MUTED
            },
        ),
    ];

    f.render_widget(
        Paragraph::new(lines).block(styled_block(" Backlog Health ")),
        area,
    );
}

/// Poisson queue model summary + top anomalies.
fn render_poisson_summary_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let p = &report.poisson;
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Queue M/M/1 summary
    match &p.queue_insight {
        Some(q) => {
            let rho_color = if q.traffic_intensity >= 0.80 {
                Color::Red
            } else if q.traffic_intensity >= 0.60 {
                Color::Yellow
            } else {
                Color::Green
            };
            lines.push(kv_line(
                "Traffic ρ (M/M/1)",
                &format!("{:.2}", q.traffic_intensity),
                rho_color,
            ));
            lines.push(kv_line(
                "MRs in system",
                &format!("{:.1}", q.expected_mrs_in_system),
                theme::MUTED,
            ));
            let wait_color = if q.expected_wait_hours > 48.0 {
                Color::Red
            } else if q.expected_wait_hours > 24.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            lines.push(kv_line(
                "Expected wait",
                &format!("{:.1} h", q.expected_wait_hours),
                wait_color,
            ));
        }
        None => {
            lines.push(Line::from(Span::styled(
                "  Queue: not enough data.",
                Style::default().fg(theme::MUTED),
            )));
        }
    }

    // Top anomaly (first only to save space)
    if let Some(signal) = p.anomalies.first() {
        let (icon, color) = match signal.severity {
            AnomalySeverity::Critical => ("✘ CRITICAL", Color::Red),
            AnomalySeverity::Warning => ("⚠ WARNING", Color::Yellow),
            AnomalySeverity::Elevated => ("↑ ELEVATED", Color::Cyan),
            AnomalySeverity::Normal => ("✔ NORMAL", Color::Green),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", icon),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(signal.metric.clone(), Style::default().fg(Color::White)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "  ✔ All signals normal",
            Style::default().fg(Color::Green),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(styled_block(" Poisson · Queue · Signals ")),
        area,
    );
}

/// Horizontal bar chart for cycle time by author.
fn render_by_author_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;

    let block = styled_block(" Cycle Time by Author (median) ");
    if agg.cycle_time_by_author.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  No author data.",
                Style::default().fg(theme::MUTED),
            ))
            .block(block),
            area,
        );
        return;
    }

    let mut entries: Vec<(&String, f64)> = agg
        .cycle_time_by_author
        .iter()
        .map(|(k, v)| (k, *v))
        .collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let max_val = entries.first().map(|(_, v)| *v).unwrap_or(1.0);

    render_bar_chart_block(f, area, block, &entries, max_val, Color::Cyan);
}

/// Horizontal bar chart for cycle time by reviewer.
fn render_by_reviewer_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;

    let block = styled_block(" Cycle Time by Reviewer (median) ");
    if agg.cycle_time_by_reviewer.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  No reviewer data.",
                Style::default().fg(theme::MUTED),
            ))
            .block(block),
            area,
        );
        return;
    }

    let mut entries: Vec<(&String, f64)> = agg
        .cycle_time_by_reviewer
        .iter()
        .map(|(k, v)| (k, *v))
        .collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let max_val = entries.first().map(|(_, v)| *v).unwrap_or(1.0);

    render_bar_chart_block(f, area, block, &entries, max_val, Color::Magenta);
}

/// Horizontal bar chart for cycle time by milestone.
fn render_by_milestone_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;

    let block = styled_block(" Cycle Time by Milestone (median) ");
    if agg.cycle_time_by_milestone.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  No milestone data.",
                Style::default().fg(theme::MUTED),
            ))
            .block(block),
            area,
        );
        return;
    }

    let mut entries: Vec<(&String, f64)> = agg
        .cycle_time_by_milestone
        .iter()
        .map(|(k, v)| (k, *v))
        .collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let max_val = entries.first().map(|(_, v)| *v).unwrap_or(1.0);

    render_bar_chart_block(f, area, block, &entries, max_val, Color::Yellow);
}

/// Generic Flex-based bar chart renderer shared by author / reviewer / milestone blocks.
///
/// Each row is split with `Layout::horizontal + Flex::Legacy` into three columns:
///
///   ┌─────────────────────────────────────────┐
///   │ label (Fill) │ bar (Fill) │ value (Max) │
///   └─────────────────────────────────────────┘
///
/// - `Constraint::Fill(2)` → label grows to absorb available space (2× weight).
/// - `Constraint::Fill(1)` → bar grows proportionally (1× weight).
/// - `Constraint::Max(22)` → value column capped at 22 chars, never truncated.
///
/// Because ratatui resolves Fill constraints after Max/Length/Min ones, the
/// value column is always satisfied first — labels and bars share what remains.
fn render_bar_chart_block(
    f: &mut Frame,
    area: Rect,
    block: Block<'static>,
    entries: &[(&String, f64)],
    max_val: f64,
    bar_color: Color,
) {
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    // One Rect per row (one entry per line).
    let row_constraints: Vec<Constraint> = entries.iter().map(|_| Constraint::Length(1)).collect();

    let row_rects = Layout::vertical(row_constraints).split(inner);

    for (i, (label, hours)) in entries.iter().enumerate() {
        let Some(row_rect) = row_rects.get(i) else {
            break;
        };

        // Split each row into 3 columns using Flex — value col is satisfied first.
        let [label_rect, bar_rect, value_rect] = {
            let cols = Layout::horizontal([
                Constraint::Fill(2), // label  — grows, absorbs leftover space
                Constraint::Fill(1), // bar    — grows at half the label's rate
                Constraint::Max(22), // value  — reserved first, never clipped
            ])
            .flex(Flex::Legacy)
            .split(*row_rect);
            [cols[0], cols[1], cols[2]]
        };

        // ── Label column ──────────────────────────────────────────────────────
        let label_width = (label_rect.width as usize).saturating_sub(2); // 2-char left indent
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    truncate(label, label_width),
                    Style::default().fg(theme::MUTED),
                ),
            ])),
            label_rect,
        );

        // ── Bar column ────────────────────────────────────────────────────────
        let bar_width = bar_rect.width as usize;
        let filled = if max_val > 0.0 {
            ((hours / max_val) * bar_width as f64).round() as usize
        } else {
            0
        }
        .min(bar_width);

        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("█".repeat(filled), Style::default().fg(bar_color)),
                Span::styled(
                    " ".repeat(bar_width - filled),
                    Style::default().bg(Color::Reset),
                ),
            ])),
            bar_rect,
        );

        // ── Value column ──────────────────────────────────────────────────────
        let days = hours / 24.0;
        let value_str = format!(" {:.1} h ({:.1}d) ", hours, days);
        f.render_widget(
            Paragraph::new(Span::styled(value_str, Style::default().fg(theme::MUTED))),
            value_rect,
        );
    }
}

/// Full-width Spearman correlations table.
fn render_correlations_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let block = styled_block(" Spearman Correlations  (|ρ| ≥ 0.30 & p < 0.05 highlighted) ");

    if report.correlations.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  Not enough data (need ≥ 3 MRs per pair).",
                Style::default().fg(theme::MUTED),
            ))
            .block(block),
            area,
        );
        return;
    }

    let mut sorted = report.correlations.clone();
    sorted.sort_by(|a, b| {
        b.rho
            .abs()
            .partial_cmp(&a.rho.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let lines: Vec<Line<'static>> = sorted.iter().map(correlation_line).collect();

    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// Footer band: throughput forecasts (scrollable) + status footer line.
///
/// Returns the clamped scroll offset so the caller can write it back to
/// `app.stats_view.scroll` without holding a simultaneous mutable borrow on `app`.
fn render_footer_band(f: &mut Frame, report: &StatReport, area: Rect, current_scroll: u16) -> u16 {
    if area.height == 0 {
        return current_scroll;
    }

    let p = &report.poisson;

    let mut lines: Vec<Line<'static>> = Vec::new();

    // ── Throughput forecasts ──────────────────────────────────────────────────
    if !p.throughput_forecasts.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                "  Forecasts  ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("─".repeat(40), Style::default().fg(theme::MUTED_DIM)),
        ]));
        for (idx, f_item) in p.throughput_forecasts.iter().enumerate() {
            lines.push(throughput_forecast_line(f_item, idx));
        }
        // Render all anomalies (first one is already summarised in the Poisson block).
        if p.anomalies.len() > 1 {
            lines.push(Line::from(vec![
                Span::styled(
                    "  Anomaly signals  ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("─".repeat(30), Style::default().fg(theme::MUTED_DIM)),
            ]));
            for signal in &p.anomalies {
                lines.push(anomaly_signal_line(signal));
            }
        }
        lines.push(Line::from(""));
    }

    let total = lines.len() as u16;
    let max_scroll = total.saturating_sub(area.height);
    let clamped = current_scroll.min(max_scroll);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::MUTED_DIM));

    f.render_widget(
        Paragraph::new(lines).block(block).scroll((clamped, 0)),
        area,
    );

    clamped
}

// ── Layout helpers ────────────────────────────────────────────────────────────

/// Returns the inner area of a fullscreen bordered block (subtracts 1px border on each side).
fn inner_area(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Splits a rect horizontally, returning `[left, right]` where left takes `pct`% of width.
fn split_horizontal(area: Rect, pct: u16) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(pct),
            Constraint::Percentage(100 - pct),
        ])
        .split(area);
    [chunks[0], chunks[1]]
}

/// Splits a rect horizontally into three equal columns (~33% each).
fn split_horizontal_thirds(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

/// A uniformly styled inner block used by all panels.
fn styled_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::MUTED_DIM))
        .title(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
}

// ── Row renderers ─────────────────────────────────────────────────────────────

/// `key  value` row with fixed label column.
fn kv_line(key: &str, value: &str, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<22}", key), Style::default().fg(theme::MUTED)),
        Span::styled(value.to_string(), Style::default().fg(value_color)),
    ])
}

/// Throughput forecast row.
///
/// Labels are derived from the position in the forecasts vec (order matches
/// `build_throughput_forecasts`):
///   index 0 → sprint pace  (2w realistic target)
///   index 1 → stretch goal (2w +20% target)
///   index 2 → floor check  (≥1 merge/week)
pub fn throughput_forecast_line(f: &ThroughputForecast, index: usize) -> Line<'static> {
    let pct = f.probability * 100.0;
    let color = if pct >= 75.0 {
        Color::Green
    } else if pct >= 40.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let sw = f.forecast_weeks;
    let label = match index {
        // Original forecasts — "will we match our usual pace?"
        0 => format!("At pace      ≥{} this week", f.target_merges),
        1 => format!("At pace      ≥{} this sprint ({}w)", f.target_merges, sw),
        // New forecasts — actionable sprint/stretch/floor
        2 => format!("Sprint pace  ≥{} in {}w (realistic)", f.target_merges, sw),
        3 => format!("Stretch goal ≥{} in {}w (+20%)", f.target_merges, sw),
        4 => format!("Floor check  ≥1 merge this week"),
        _ => format!("≥{} in {}w", f.target_merges, sw),
    };

    Line::from(vec![
        Span::styled(
            format!("  {:<36}", label),
            Style::default().fg(theme::MUTED),
        ),
        Span::styled(
            format!("{:.1}%", pct),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  (λ={:.1}/w)", f.lambda_per_week),
            Style::default().fg(theme::MUTED_DIM),
        ),
    ])
}

/// Anomaly signal row.
fn anomaly_signal_line(signal: &AnomalySignal) -> Line<'static> {
    let (icon, color) = match signal.severity {
        AnomalySeverity::Critical => ("✘ CRITICAL", Color::Red),
        AnomalySeverity::Warning => ("⚠ WARNING ", Color::Yellow),
        AnomalySeverity::Elevated => ("↑ ELEVATED", Color::Cyan),
        AnomalySeverity::Normal => ("✔ NORMAL  ", Color::Green),
    };
    Line::from(vec![
        Span::styled(
            format!("  {} ", icon),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<30}", signal.metric),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!(
                "observed={:<5} λ={:.1}  p={:.3}",
                signal.observed, signal.baseline_lambda, signal.p_value
            ),
            Style::default().fg(theme::MUTED),
        ),
    ])
}

/// Spearman correlation row.
///
/// Visual encoding:
/// - `●` Cyan    = Strong / Very Strong + significant
/// - `●` Yellow  = Moderate + significant
/// - `○` Muted   = Weak or not significant
/// - `·` DimMuted = Negligible
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

    let pair_lbl = pair_label(&cr.pair);
    let rho_col = rho_color(cr.rho, significant);
    let strength_lbl = strength_label(&cr.interpretation);
    let direction = if cr.rho > 0.0 { "↑↑" } else { "↑↓" };

    let text_style = if significant {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(theme::MUTED)
    };

    Line::from(vec![
        Span::styled(format!("  {bullet} "), Style::default().fg(bullet_color)),
        // 36 chars fits the longest label "Pipeline failures  ↔  Cycle time" with margin
        Span::styled(format!("{:<36}", pair_lbl), text_style),
        Span::styled(
            format!("ρ = {:+.3}    ", cr.rho),
            Style::default().fg(rho_col).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("p = {:.3}    ", cr.p_value),
            Style::default().fg(if cr.p_value < 0.05 {
                theme::MUTED
            } else {
                theme::MUTED_DIM
            }),
        ),
        Span::styled(format!("{direction}  {:<13}", strength_lbl), text_style),
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
#[allow(dead_code)]
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
        let sprint_weeks = app.stats_view.sprint_weeks;
        let tx2 = tx.clone();

        tokio::spawn(async move {
            let filter = QueryFilter {
                window,
                project_id: Some(project_id),
                sprint_weeks: Some(sprint_weeks),
                ..Default::default()
            };

            match aggregate(db.as_ref(), &filter).await {
                Ok(agg) => {
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
