use gitlab_tracker_stats::poisson::{AnomalySeverity, QueueStatus};
use gitlab_tracker_stats::StatReport;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Gauge, Paragraph, Wrap},
    Frame,
};

use crate::ui::theme;

use super::common::{kv_line, split_horizontal, styled_block};
use gitlab_tracker_stats::aggregator::percentile;

pub(super) fn render_overview_tab(f: &mut Frame, report: &StatReport, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(1),
        ])
        .split(area);

    let [left0, right0] = split_horizontal(chunks[0], 50);
    render_throughput_block(f, report, left0);
    render_cycle_time_block(f, report, right0);

    let [left1, right1] = split_horizontal(chunks[1], 35);
    render_backlog_block(f, report, left1);
    render_poisson_summary_block(f, report, right1);

    render_summary_insights_block(f, report, chunks[2]);
}

/// Throughput numbers + review quality signals.
fn render_throughput_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;
    let throughput = agg
        .throughput_per_week
        .map(|v| format!("{v:.1} MR/week"))
        .unwrap_or_else(|| "n/a".to_string());

    let mut lines = vec![
        kv_line("Merged", &agg.merged_count.to_string(), Color::Green),
        kv_line("Closed", &agg.closed_count.to_string(), theme::muted()),
        kv_line("Throughput", &throughput, Color::Cyan),
    ];

    if let Some(rate) = agg.abandon_rate {
        lines.push(kv_line(
            "Abandon rate",
            &format!("{:.1}%", rate * 100.0),
            if rate >= 0.25 {
                Color::Red
            } else if rate >= 0.10 {
                Color::Yellow
            } else {
                theme::muted()
            },
        ));
    }

    lines.push(kv_line(
        "Avg diff size",
        &format!("{:.0} lines", agg.avg_diff_size),
        theme::muted(),
    ));
    lines.push(kv_line(
        "Avg comments",
        &format!("{:.1}", agg.avg_comments),
        theme::muted(),
    ));

    if let Some(density) = agg.avg_comment_density {
        lines.push(kv_line(
            "Comments /100l",
            &format!("{density:.1}"),
            if density >= 5.0 {
                Color::Yellow
            } else {
                theme::muted()
            },
        ));
    }

    if let Some(pfr) = agg.avg_pipeline_failure_rate {
        lines.push(kv_line(
            "Pipeline fail rate",
            &format!("{:.1}%", pfr * 100.0),
            if pfr > 0.3 {
                Color::Red
            } else {
                theme::muted()
            },
        ));
    }

    lines.push(kv_line(
        "Pipeline coverage",
        &format!("{:.0}%", agg.pipeline_data_coverage * 100.0),
        if agg.pipeline_data_coverage < 0.5 {
            Color::Yellow
        } else {
            theme::muted()
        },
    ));

    f.render_widget(
        Paragraph::new(lines)
            .block(styled_block(" Throughput · Quality "))
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

    let block = styled_block(" Cycle Time (created → merged) ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 5 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let label_style = Style::default()
        .fg(crate::ui::theme::fg())
        .add_modifier(Modifier::BOLD);

    let fmt_value = |hours: f64| -> String {
        let days = hours / 24.0;
        format!("{:.1} h ({:.1}d)", hours, days)
    };

    let median_val = fmt_value(median);
    let p75_val = fmt_value(p75);
    let p90_val = fmt_value(p90);
    let val_width = median_val.len().max(p75_val.len()).max(p90_val.len());
    let fmt_gauge_label = |name: &str, val: &str| -> String {
        format!("{:<6}  {:<width$}", name, val, width = val_width)
    };

    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
            .ratio((median / max).clamp(0.0, 1.0))
            .label(Span::styled(
                fmt_gauge_label("Median", &median_val),
                label_style,
            )),
        rows[0],
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
            .ratio((p75 / max).clamp(0.0, 1.0))
            .label(Span::styled(fmt_gauge_label("P75", &p75_val), label_style)),
        rows[1],
    );
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Red).bg(Color::DarkGray))
            .ratio((p90 / max).clamp(0.0, 1.0))
            .label(Span::styled(fmt_gauge_label("P90", &p90_val), label_style)),
        rows[2],
    );

    let spread = p90 - median;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Spread  ", Style::default().fg(theme::muted())),
            Span::styled(
                format!("{:>5.1} h ({:.1}d)  (P90 − Median)", spread, spread / 24.0),
                Style::default().fg(theme::muted_dim()),
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
                Style::default().fg(theme::muted()),
            )))
            .block(styled_block(" Backlog Health ")),
            area,
        );
        return;
    }

    let oldest = agg.open_mr_ages_days.last().copied().unwrap_or(0.0);
    let median_age = percentile(&agg.open_mr_ages_days, 50.0).unwrap_or(0.0);

    let lines = vec![
        kv_line(
            "Open MRs",
            &agg.open_mr_ages_days.len().to_string(),
            theme::muted(),
        ),
        kv_line(
            "Median age",
            &format!("{:.0} days", median_age),
            if median_age > 14.0 {
                Color::Yellow
            } else {
                theme::muted()
            },
        ),
        kv_line(
            "Oldest",
            &format!("{:.0} days", oldest),
            if oldest > 30.0 {
                Color::Red
            } else {
                theme::muted()
            },
        ),
        kv_line(
            "Stale ≥7d",
            &agg.stale_open_mrs_7d.to_string(),
            if agg.stale_open_mrs_7d > 0 {
                Color::Yellow
            } else {
                theme::muted()
            },
        ),
        kv_line(
            "Stale ≥14d",
            &agg.stale_open_mrs_14d.to_string(),
            if agg.stale_open_mrs_14d > 0 {
                Color::Yellow
            } else {
                theme::muted()
            },
        ),
        kv_line(
            "Stale ≥30d",
            &agg.stale_open_mrs_30d.to_string(),
            if agg.stale_open_mrs_30d > 0 {
                Color::Red
            } else {
                theme::muted()
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

    let q = &p.queue_insight;
    match q.status {
        QueueStatus::Stable => {
            let rho = q.traffic_intensity.unwrap_or(0.0);
            let rho_color = if rho >= 0.80 {
                Color::Red
            } else if rho >= 0.60 {
                Color::Yellow
            } else {
                Color::Green
            };
            lines.push(kv_line("Flow pressure ρ", &format!("{rho:.2}"), rho_color));
            if let Some(mrs) = q.expected_mrs_in_system {
                lines.push(kv_line(
                    "MRs in system",
                    &format!("{mrs:.1}"),
                    theme::muted(),
                ));
            }
            if let Some(wait_hours) = q.expected_wait_hours {
                let wait_color = if wait_hours > 48.0 {
                    Color::Red
                } else if wait_hours > 24.0 {
                    Color::Yellow
                } else {
                    Color::Green
                };
                lines.push(kv_line(
                    "Expected wait",
                    &format!("{wait_hours:.1} h"),
                    wait_color,
                ));
            }
        }
        QueueStatus::OverCapacity => {
            lines.push(Line::from(Span::styled(
                "  Flow pressure: at or above estimated capacity.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            if let Some(rho) = q.traffic_intensity {
                lines.push(kv_line("Flow pressure ρ", &format!("{rho:.2}"), Color::Red));
            }
        }
        QueueStatus::InsufficientData => {
            lines.push(Line::from(Span::styled(
                "  Flow pressure: not enough data.",
                Style::default().fg(theme::muted()),
            )));
        }
    }

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
            Span::styled(
                signal.metric.clone(),
                Style::default().fg(crate::ui::theme::fg()),
            ),
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

fn render_summary_insights_block(f: &mut Frame, report: &StatReport, area: Rect) {
    let agg = &report.aggregated;
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "  Snapshot  ",
            Style::default()
                .fg(crate::ui::theme::fg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{} total · {} merged · {} open · {} stale ≥14d",
                agg.total_mrs,
                agg.merged_count,
                agg.open_mr_ages_days.len(),
                agg.stale_open_mrs_14d
            ),
            Style::default().fg(theme::muted()),
        ),
    ])];

    let abandon = agg
        .abandon_rate
        .map(|rate| format!("abandon {:.1}%", rate * 100.0))
        .unwrap_or_else(|| "abandon n/a".to_string());
    lines.push(Line::from(vec![
        Span::styled(
            "  Delivery  ",
            Style::default()
                .fg(crate::ui::theme::fg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{abandon} · pipeline coverage {:.0}%",
                agg.pipeline_data_coverage * 100.0
            ),
            Style::default().fg(theme::muted()),
        ),
    ]));

    let top_signal = report
        .poisson
        .anomalies
        .first()
        .map(|signal| signal.metric.as_str())
        .unwrap_or("none");
    lines.push(Line::from(vec![
        Span::styled(
            "  Signal    ",
            Style::default()
                .fg(crate::ui::theme::fg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(top_signal.to_string(), Style::default().fg(theme::muted())),
    ]));

    f.render_widget(
        Paragraph::new(lines)
            .block(styled_block(" Summary insights "))
            .wrap(Wrap { trim: false }),
        area,
    );
}
