use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::ui::theme;

/// Returns the inner area of a fullscreen bordered block.
pub(crate) fn inner_area(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Splits a rect horizontally, returning `[left, right]` where left takes `pct`% of width.
pub(crate) fn split_horizontal(area: Rect, pct: u16) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(pct),
            Constraint::Percentage(100 - pct),
        ])
        .split(area);
    [chunks[0], chunks[1]]
}

/// Splits a rect horizontally into three equal columns.
pub(crate) fn split_horizontal_thirds(area: Rect) -> [Rect; 3] {
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

/// A uniformly styled inner block used by all stats panels.
pub(crate) fn styled_block(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::muted_dim()))
        .title(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
}

pub(crate) fn render_scrollable_lines(
    f: &mut Frame,
    area: Rect,
    block: Block<'static>,
    lines: Vec<Line<'static>>,
    current_scroll: u16,
) -> u16 {
    if area.height == 0 {
        return current_scroll;
    }
    let total = lines.len() as u16;
    let max_scroll = total.saturating_sub(area.height.saturating_sub(2));
    let clamped = current_scroll.min(max_scroll);
    f.render_widget(
        Paragraph::new(lines).block(block).scroll((clamped, 0)),
        area,
    );
    clamped
}

pub(crate) fn section_header(label: &'static str, dash_count: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label}  "),
            Style::default()
                .fg(crate::ui::theme::fg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "─".repeat(dash_count),
            Style::default().fg(theme::muted_dim()),
        ),
    ])
}

/// `key  value` row with fixed label column.
pub(crate) fn kv_line(key: &str, value: &str, value_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<22}", key),
            Style::default().fg(theme::muted()),
        ),
        Span::styled(value.to_string(), Style::default().fg(value_color)),
    ])
}

/// Truncates a string to `max_chars` characters, appending `…` when truncated.
pub(crate) fn truncate(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        format!("{truncated}…")
    }
}

pub(crate) fn format_duration_hours(hours: f64) -> String {
    if hours >= 24.0 {
        format!("{:.1}d", hours / 24.0)
    } else {
        format!("{hours:.1}h")
    }
}
