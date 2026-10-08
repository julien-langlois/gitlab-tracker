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
            Style::default().fg(theme::muted()),
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
            Style::default().fg(theme::muted()),
        ),
        Span::styled(
            format!("{:.1}%", pct),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  (λ={:.1}/w)", f.lambda_per_week),
            Style::default().fg(theme::muted_dim()),
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
    // Integers (counts) without decimals, rates and durations with two.
    let fmt = |v: f64| {
        if v.fract() == 0.0 {
            format!("{v:.0}")
        } else {
            format!("{v:.2}")
        }
    };
    let detail = match (signal.baseline, signal.p_value, signal.ratio) {
        (Some(expected), Some(p), _) => format!(
            "observed={:<5} expected={:.1}  p={:.3}",
            fmt(signal.observed),
            expected,
            p
        ),
        (Some(baseline), None, Some(ratio)) => format!(
            "observed={:<5} baseline={}  ×{:.1}",
            fmt(signal.observed),
            fmt(baseline),
            ratio
        ),
        _ => format!("value={:<5} pressure signal", fmt(signal.observed)),
    };

    Line::from(vec![
        Span::styled(
            format!("  {} ", icon),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<30}", signal.metric),
            Style::default().fg(crate::ui::theme::fg()),
        ),
        Span::styled(detail, Style::default().fg(theme::muted())),
    ])
}

#[cfg(test)]
mod tests {
    use super::super::tests::{demo_report, line_text};
    use super::*;

    fn forecast(probability: f64) -> ThroughputForecast {
        ThroughputForecast {
            lambda_per_week: 3.5,
            forecast_weeks: 2,
            target_merges: 7,
            probability,
            expected_merges: 7.0,
        }
    }

    #[test]
    fn forecast_rows_are_labelled_by_position_and_coloured_by_probability() {
        let labels: Vec<String> = (0..6)
            .map(|index| line_text(&throughput_forecast_line(&forecast(0.5), index)))
            .collect();
        assert!(labels[0].contains("≥7 this week"));
        assert!(labels[1].contains("≥7 this sprint (2w)"));
        assert!(labels[2].contains("≥7 in 2w (realistic)"));
        assert!(labels[3].contains("≥7 in 2w (+20%)"));
        assert!(labels[4].contains("≥1 merge this week"));
        assert!(labels[5].contains("≥7 in 2w"));
        assert!(labels
            .iter()
            .all(|l| l.contains("50.0%") && l.contains("(λ=3.5/w)")));

        let colour = |p| throughput_forecast_line(&forecast(p), 0).spans[1].style.fg;
        assert_eq!(colour(0.75), Some(Color::Green));
        assert_eq!(colour(0.74), Some(Color::Yellow));
        assert_eq!(colour(0.40), Some(Color::Yellow));
        assert_eq!(colour(0.39), Some(Color::Red));
    }

    #[test]
    fn anomaly_rows_show_the_evidence_they_have() {
        let signal = |baseline, ratio, p_value, severity| AnomalySignal {
            metric: "Open MRs".into(),
            observed: 14.0,
            baseline,
            ratio,
            p_value,
            severity,
        };
        // Poisson test against an expected count.
        let tested = anomaly_signal_line(&signal(
            Some(6.25),
            None,
            Some(0.004),
            AnomalySeverity::Critical,
        ));
        let text = line_text(&tested);
        assert!(
            text.contains("✘ CRITICAL") && text.contains("Open MRs"),
            "{text}"
        );
        assert!(
            text.contains("observed=14")
                && text.contains("expected=6.2")
                && text.contains("p=0.004")
        );
        assert_eq!(tested.spans[0].style.fg, Some(Color::Red));

        // Ratio against a baseline: counts without decimals, rates with two.
        let mut ratio = signal(Some(7.0), Some(2.0), None, AnomalySeverity::Warning);
        let text = line_text(&anomaly_signal_line(&ratio));
        assert!(
            text.contains("⚠ WARNING") && text.contains("baseline=7  ×2.0"),
            "{text}"
        );
        ratio.observed = 1.256;
        assert!(line_text(&anomaly_signal_line(&ratio)).contains("observed=1.26"));

        // Neither: a bare pressure value.
        let bare = anomaly_signal_line(&signal(None, None, None, AnomalySeverity::Elevated));
        let text = line_text(&bare);
        assert!(
            text.contains("↑ ELEVATED")
                && text.contains("value=14")
                && text.contains("pressure signal")
        );
        let normal = anomaly_signal_line(&signal(None, None, None, AnomalySeverity::Normal));
        assert_eq!(normal.spans[0].style.fg, Some(Color::Green));
    }

    #[tokio::test]
    async fn empty_sections_say_so() {
        let mut report = demo_report().await;
        report.poisson.throughput_forecasts.clear();
        report.poisson.anomalies.clear();
        let rows: Vec<String> = forecast_signal_lines(&report)
            .iter()
            .map(line_text)
            .collect();
        assert!(rows[0].contains("Not enough throughput data"));
        assert!(rows
            .last()
            .unwrap()
            .contains("No pressure or baseline anomaly signals"));

        report.poisson.throughput_forecasts = vec![forecast(0.9), forecast(0.2)];
        let rows: Vec<String> = forecast_signal_lines(&report)
            .iter()
            .map(line_text)
            .collect();
        assert!(rows[0].contains("Forecasts"));
        assert!(rows[1].contains("this week") && rows[2].contains("this sprint"));
    }
}
