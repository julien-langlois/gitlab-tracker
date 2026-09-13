use gitlab_tracker_stats::StatReport;
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::ui::theme;

use super::common::{split_horizontal_thirds, styled_block, truncate};

pub(super) fn render_flow_tab(f: &mut Frame, report: &StatReport, area: Rect) {
    let [left, mid, right] = split_horizontal_thirds(area);
    render_by_author_block(f, report, left);
    render_by_reviewer_block(f, report, mid);
    render_by_milestone_block(f, report, right);
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

    let row_constraints: Vec<Constraint> = entries.iter().map(|_| Constraint::Length(1)).collect();
    let row_rects = Layout::vertical(row_constraints).split(inner);

    for (i, (label, hours)) in entries.iter().enumerate() {
        let Some(row_rect) = row_rects.get(i) else {
            break;
        };

        let [label_rect, bar_rect, value_rect] = {
            let cols = Layout::horizontal([
                Constraint::Fill(2),
                Constraint::Fill(1),
                Constraint::Max(22),
            ])
            .flex(Flex::Legacy)
            .split(*row_rect);
            [cols[0], cols[1], cols[2]]
        };

        let label_width = (label_rect.width as usize).saturating_sub(2);
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

        let days = hours / 24.0;
        let value_str = format!(" {:.1} h ({:.1}d) ", hours, days);
        f.render_widget(
            Paragraph::new(Span::styled(value_str, Style::default().fg(theme::MUTED))),
            value_rect,
        );
    }
}
