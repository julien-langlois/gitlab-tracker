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

#[cfg(test)]
mod tests {
    use super::super::tests::{demo_report, line_text};
    use super::*;

    #[test]
    fn confidence_levels_and_ratio_scores() {
        assert_eq!(confidence_level(1.0), ("HIGH", Color::Green));
        assert_eq!(confidence_level(0.75), ("HIGH", Color::Green));
        assert_eq!(confidence_level(0.74), ("MED", Color::Yellow));
        assert_eq!(confidence_level(0.45), ("MED", Color::Yellow));
        assert_eq!(confidence_level(0.44), ("LOW", Color::Red));

        assert_eq!(ratio_score(5, 10), 0.5);
        assert_eq!(ratio_score(50, 10), 1.0, "capped");
        assert_eq!(ratio_score(0, 10), 0.0);
        assert_eq!(ratio_score(3, 0), 1.0, "no target: always met");
    }

    #[tokio::test]
    async fn overall_confidence_weighs_sample_cycle_time_and_metadata() {
        let mut report = demo_report().await;
        let set = |report: &mut StatReport, mrs: usize, merged: usize, coverage: f64| {
            let agg = &mut report.aggregated;
            agg.total_mrs = mrs;
            agg.cycle_time_sample_size = merged;
            agg.pipeline_data_coverage = coverage;
            agg.reviewer_coverage = coverage;
            agg.milestone_coverage = coverage;
        };
        set(&mut report, 20, 10, 1.0);
        assert!((confidence_score(&report) - 1.0).abs() < 1e-9);
        set(&mut report, 0, 0, 0.0);
        assert_eq!(confidence_score(&report), 0.0);
        // Half the sample (40 %), full cycle-time sample (35 %), no metadata (25 %).
        set(&mut report, 10, 10, 0.0);
        assert!((confidence_score(&report) - 0.55).abs() < 1e-9);

        let rows: Vec<String> = data_confidence_lines(&report)
            .iter()
            .map(line_text)
            .collect();
        assert_eq!(rows.len(), 5);
        assert!(
            rows[0].contains("Overall") && rows[0].contains("MED"),
            "{}",
            rows[0]
        );
        assert!(rows[0].contains("10 MRs in sample"));
        assert!(rows[1].contains("HIGH") && rows[1].contains("10 merged MRs"));
        assert!(rows[2].contains("LOW") && rows[2].contains("0% coverage"));
    }

    #[test]
    fn size_bucket_rows_show_range_counts_and_median() {
        let bucket = |label: &str, max, median| MrSizeBucketStats {
            label: label.into(),
            min_changed_lines: 100,
            max_changed_lines: max,
            total_mrs: 7,
            merged_mrs: 5,
            cycle_time_median_hours: median,
        };
        let closed = size_bucket_line(&bucket("Medium", Some(499), Some(48.0)));
        let text = line_text(&closed);
        assert!(
            text.contains("100-499 lines") && text.contains("P50 2.0d"),
            "{text}"
        );
        assert!(text.contains("total=7") && text.contains("merged=5"));
        assert_eq!(closed.spans[0].style.fg, Some(Color::Cyan));

        // Open-ended last bucket, without any merged MR to time.
        let open = size_bucket_line(&bucket("Huge", None, None));
        let text = line_text(&open);
        assert!(
            text.contains("100+ lines") && text.contains("P50 n/a"),
            "{text}"
        );
        assert_eq!(open.spans[0].style.fg, Some(Color::Red));
    }
}
