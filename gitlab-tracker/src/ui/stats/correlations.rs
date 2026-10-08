use gitlab_tracker_stats::correlation::{CorrelationResult, CorrelationStrength};
use gitlab_tracker_stats::{MetricPair, StatReport};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::ui::theme;

use super::common::{render_scrollable_lines, styled_block};

pub(super) fn render_correlations_tab(
    f: &mut Frame,
    report: &StatReport,
    area: Rect,
    current_scroll: u16,
) -> u16 {
    render_scrollable_lines(
        f,
        area,
        styled_block(" Spearman Correlations  (|ρ| ≥ 0.30 & p < 0.05 highlighted) "),
        correlation_lines(report),
        current_scroll,
    )
}

fn correlation_lines(report: &StatReport) -> Vec<Line<'static>> {
    if report.correlations.is_empty() {
        return vec![Line::from(Span::styled(
            "  Not enough data (need ≥ 3 MRs per pair).",
            Style::default().fg(theme::muted()),
        ))];
    }

    let mut sorted = report.correlations.clone();
    sorted.sort_by(|a, b| {
        b.rho
            .abs()
            .partial_cmp(&a.rho.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted.iter().map(correlation_line).collect()
}

/// Spearman correlation row.
fn correlation_line(cr: &CorrelationResult) -> Line<'static> {
    let significant = cr.p_value < 0.05 && cr.rho.abs() >= 0.30;

    let (bullet, bullet_color) = if significant {
        match cr.interpretation {
            CorrelationStrength::VeryStrong | CorrelationStrength::Strong => ("●", Color::Cyan),
            CorrelationStrength::Moderate => ("●", Color::Yellow),
            _ => ("○", theme::muted()),
        }
    } else if matches!(cr.interpretation, CorrelationStrength::Negligible) {
        ("·", theme::muted_dim())
    } else {
        ("○", theme::muted())
    };

    let pair_lbl = pair_label(&cr.pair);
    let rho_col = rho_color(cr.rho, significant);
    let strength_lbl = strength_label(&cr.interpretation);
    let direction = if cr.rho > 0.0 { "↑↑" } else { "↑↓" };

    let text_style = if significant {
        Style::default().fg(crate::ui::theme::fg())
    } else {
        Style::default().fg(theme::muted())
    };

    Line::from(vec![
        Span::styled(format!("  {bullet} "), Style::default().fg(bullet_color)),
        Span::styled(format!("{:<36}", pair_lbl), text_style),
        Span::styled(
            format!("ρ = {:+.3}    ", cr.rho),
            Style::default().fg(rho_col).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("p = {:.3}    ", cr.p_value),
            Style::default().fg(if cr.p_value < 0.05 {
                theme::muted()
            } else {
                theme::muted_dim()
            }),
        ),
        Span::styled(format!("{direction}  {:<13}", strength_lbl), text_style),
        Span::styled(
            format!("(n={})", cr.sample_size),
            Style::default().fg(theme::muted_dim()),
        ),
    ])
}

/// Maps a [`MetricPair`] to a concise human-readable label.
fn pair_label(pair: &MetricPair) -> &'static str {
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
        return theme::muted_dim();
    }
    let abs = rho.abs();
    if abs >= 0.70 {
        Color::Cyan
    } else if abs >= 0.50 {
        Color::Green
    } else if abs >= 0.30 {
        Color::Yellow
    } else {
        theme::muted()
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

#[cfg(test)]
mod tests {
    use super::super::tests::{demo_report, line_text};
    use super::*;

    fn result(rho: f64, p_value: f64, interpretation: CorrelationStrength) -> CorrelationResult {
        CorrelationResult {
            pair: MetricPair::DiffSizeVsCycleTime,
            rho,
            p_value,
            sample_size: 12,
            interpretation,
        }
    }

    #[test]
    fn only_significant_correlations_are_highlighted() {
        // Significant = p < 0.05 and |ρ| ≥ 0.30.
        let strong = correlation_line(&result(0.82, 0.001, CorrelationStrength::VeryStrong));
        let text = line_text(&strong);
        assert!(text.contains("●") && text.contains("ρ = +0.820"), "{text}");
        assert!(text.contains("↑↑") && text.contains("Very strong") && text.contains("(n=12)"));
        assert_eq!(strong.spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(strong.spans[2].style.fg, Some(Color::Cyan));

        let moderate = correlation_line(&result(-0.45, 0.03, CorrelationStrength::Moderate));
        let text = line_text(&moderate);
        assert!(
            text.contains("●") && text.contains("ρ = -0.450") && text.contains("↑↓"),
            "{text}"
        );
        assert_eq!(moderate.spans[0].style.fg, Some(Color::Yellow));

        // Same ρ, but it could be chance: hollow bullet, dimmed value.
        let unsure = correlation_line(&result(-0.45, 0.20, CorrelationStrength::Moderate));
        assert!(line_text(&unsure).contains("○"));
        assert_eq!(unsure.spans[2].style.fg, Some(theme::muted_dim()));
        // Low p-value on a tiny effect is not a finding either.
        let tiny = correlation_line(&result(0.10, 0.01, CorrelationStrength::Negligible));
        assert!(line_text(&tiny).contains("·"));
    }

    #[test]
    fn rho_colour_follows_the_magnitude_when_significant() {
        assert_eq!(rho_color(-0.75, true), Color::Cyan);
        assert_eq!(rho_color(0.55, true), Color::Green);
        assert_eq!(rho_color(0.30, true), Color::Yellow);
        assert_eq!(rho_color(0.95, false), theme::muted_dim());
    }

    #[tokio::test]
    async fn rows_are_sorted_by_strength_whatever_the_sign() {
        let mut report = demo_report().await;
        report.correlations = vec![
            result(0.20, 0.5, CorrelationStrength::Weak),
            result(-0.90, 0.001, CorrelationStrength::VeryStrong),
            result(0.50, 0.01, CorrelationStrength::Moderate),
        ];
        let rows: Vec<String> = correlation_lines(&report).iter().map(line_text).collect();
        let position = |needle: &str| rows.iter().position(|row| row.contains(needle)).unwrap();
        assert!(position("-0.900") < position("+0.500"));
        assert!(position("+0.500") < position("+0.200"));

        report.correlations.clear();
        let rows = correlation_lines(&report);
        assert_eq!(rows.len(), 1);
        assert!(line_text(&rows[0]).contains("Not enough data"));
    }

    #[test]
    fn every_metric_pair_has_its_own_label() {
        let pairs = [
            MetricPair::DiffSizeVsCycleTime,
            MetricPair::DiffSizeVsComments,
            MetricPair::CommentsVsCycleTime,
            MetricPair::PipelineFailuresVsCycleTime,
            MetricPair::CommitsCountVsCycleTime,
            MetricPair::DiffDifficultyVsCycleTime,
        ];
        let labels: std::collections::HashSet<&str> = pairs.iter().map(pair_label).collect();
        assert_eq!(labels.len(), pairs.len());
        // Labels fit the 36-column pair column of `correlation_line`.
        assert!(labels.iter().all(|label| label.chars().count() <= 36));
    }
}
