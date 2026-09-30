use gitlab_tracker_stats::{MrSizeBucketStats, StatReport};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};

use crate::ui::theme;

use super::common::{format_duration_hours, render_scrollable_lines, section_header, styled_block};

pub(super) fn render_quality_tab(
    f: &mut Frame,
    report: &StatReport,
    area: Rect,
    current_scroll: u16,
) -> u16 {
    let mut lines = Vec::new();
    lines.push(section_header("Data confidence", 32));
    lines.extend(data_confidence_lines(report));
    lines.push(Line::from(""));

    if !report.aggregated.size_buckets.is_empty() {
        lines.push(section_header("MR size buckets", 34));
        for bucket in &report.aggregated.size_buckets {
            lines.push(size_bucket_line(bucket));
        }
    }

    render_scrollable_lines(f, area, styled_block(" Quality "), lines, current_scroll)
}

/// Data confidence summary rows.
fn data_confidence_lines(report: &StatReport) -> Vec<Line<'static>> {
    let agg = &report.aggregated;
    vec![
        confidence_line(
            "Overall",
            confidence_score(report),
            format!("{} MRs in sample", agg.total_mrs),
        ),
        confidence_line(
            "Cycle time",
            ratio_score(agg.cycle_time_sample_size, 10),
            format!("{} merged MRs", agg.cycle_time_sample_size),
        ),
        confidence_line(
            "Pipeline data",
            agg.pipeline_data_coverage,
            format!(
                "{} MRs · {:.0}% coverage",
                agg.pipeline_sample_size,
                agg.pipeline_data_coverage * 100.0
            ),
        ),
        confidence_line(
            "Reviewers",
            agg.reviewer_coverage,
            format!("{:.0}% coverage", agg.reviewer_coverage * 100.0),
        ),
        confidence_line(
            "Milestones",
            agg.milestone_coverage,
            format!("{:.0}% coverage", agg.milestone_coverage * 100.0),
        ),
    ]
}

fn confidence_score(report: &StatReport) -> f64 {
    let agg = &report.aggregated;
    let sample_score = ratio_score(agg.total_mrs, 20);
    let cycle_score = ratio_score(agg.cycle_time_sample_size, 10);
    let metadata_score =
        (agg.pipeline_data_coverage + agg.reviewer_coverage + agg.milestone_coverage) / 3.0;
    (sample_score * 0.40) + (cycle_score * 0.35) + (metadata_score * 0.25)
}

fn ratio_score(value: usize, target: usize) -> f64 {
    if target == 0 {
        return 1.0;
    }
    (value as f64 / target as f64).clamp(0.0, 1.0)
}

fn confidence_line(label: &str, score: f64, detail: String) -> Line<'static> {
    let (level, color) = confidence_level(score);
    Line::from(vec![
        Span::styled(
            format!("  {:<14}", label),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(
            format!("{:<6}", level),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {detail}"),
            Style::default().fg(theme::muted_dim()),
        ),
    ])
}

fn confidence_level(score: f64) -> (&'static str, Color) {
    if score >= 0.75 {
        ("HIGH", Color::Green)
    } else if score >= 0.45 {
        ("MED", Color::Yellow)
    } else {
        ("LOW", Color::Red)
    }
}

/// MR size bucket row.
fn size_bucket_line(bucket: &MrSizeBucketStats) -> Line<'static> {
    let range = match bucket.max_changed_lines {
        Some(max) => format!("{}-{} lines", bucket.min_changed_lines, max),
        None => format!("{}+ lines", bucket.min_changed_lines),
    };
    let median = bucket
        .cycle_time_median_hours
        .map(|hours| format!("P50 {}", format_duration_hours(hours)))
        .unwrap_or_else(|| "P50 n/a".to_string());
    let color = match bucket.label.as_str() {
        "Small" => Color::Green,
        "Medium" => Color::Cyan,
        "Large" => Color::Yellow,
        "Huge" => Color::Red,
        _ => theme::muted(),
    };

    Line::from(vec![
        Span::styled(
            format!("  {:<7}", bucket.label),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<15}", range),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(
            format!(
                "total={:<4} merged={:<4}",
                bucket.total_mrs, bucket.merged_mrs
            ),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(
            format!("  {median}"),
            Style::default().fg(theme::muted_dim()),
        ),
    ])
}
