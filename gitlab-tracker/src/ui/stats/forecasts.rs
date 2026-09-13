use gitlab_tracker_stats::poisson::{AnomalySeverity, AnomalySignal, ThroughputForecast};
use gitlab_tracker_stats::StatReport;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::ui::theme;

use super::common::{render_scrollable_lines, section_header, styled_block};

pub(super) fn render_forecasts_tab(
    f: &mut Frame,
    report: &StatReport,
    area: Rect,
    current_scroll: u16,
) -> u16 {
    let lines = forecast_signal_lines(report);
    render_scrollable_lines(
        f,
        area,
        styled_block(" Forecasts · Signals "),
        lines,
        current_scroll,
    )
}

fn forecast_signal_lines(report: &StatReport) -> Vec<Line<'static>> {
    let p = &report.poisson;
    let mut lines: Vec<Line<'static>> = Vec::new();

    if p.throughput_forecasts.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Not enough throughput data for forecasts.",
            Style::default().fg(theme::MUTED),
        )));
    } else {
        lines.push(section_header("Forecasts", 40));
        for (idx, f_item) in p.throughput_forecasts.iter().enumerate() {
            lines.push(throughput_forecast_line(f_item, idx));
        }
    }

    lines.push(Line::from(""));
    lines.push(section_header("Signals", 42));
    if p.anomalies.is_empty() {
        lines.push(Line::from(Span::styled(
            "  ✔ No pressure or baseline anomaly signals.",
            Style::default().fg(Color::Green),
        )));
    } else {
        for signal in &p.anomalies {
            lines.push(anomaly_signal_line(signal));
        }
    }

    lines
}

/// Throughput forecast row.
fn throughput_forecast_line(f: &ThroughputForecast, index: usize) -> Line<'static> {
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
        0 => format!("At pace      ≥{} this week", f.target_merges),
        1 => format!("At pace      ≥{} this sprint ({}w)", f.target_merges, sw),
        2 => format!("Sprint pace  ≥{} in {}w (realistic)", f.target_merges, sw),
        3 => format!("Stretch goal ≥{} in {}w (+20%)", f.target_merges, sw),
        4 => "Floor check  ≥1 merge this week".to_string(),
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
    let detail = if signal.baseline_lambda > 0.0 {
        format!(
            "observed={:<5} baseline={:.1}  p={:.3}",
            signal.observed, signal.baseline_lambda, signal.p_value
        )
    } else {
        format!("value={:<5} pressure signal", signal.observed)
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
        Span::styled(detail, Style::default().fg(theme::MUTED)),
    ])
}
