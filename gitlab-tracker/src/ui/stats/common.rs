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

#[cfg(test)]
mod tests {
    use super::super::tests::line_text;
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn truncate_counts_characters_and_marks_the_cut() {
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcdef", 4), "abc…");
        // Multi-byte characters are never split.
        assert_eq!(truncate("éèàùô", 3), "éè…");
        assert_eq!(truncate("abc", 0), "…");
    }

    #[test]
    fn durations_switch_to_days_from_24_hours() {
        assert_eq!(format_duration_hours(0.0), "0.0h");
        assert_eq!(format_duration_hours(23.94), "23.9h");
        assert_eq!(format_duration_hours(24.0), "1.0d");
        assert_eq!(format_duration_hours(60.0), "2.5d");
    }

    #[test]
    fn layout_helpers_stay_inside_their_area() {
        assert_eq!(inner_area(Rect::new(0, 0, 10, 6)), Rect::new(1, 1, 8, 4));
        // Too small for a border: empty, not an underflow.
        let empty = inner_area(Rect::new(0, 0, 1, 1));
        assert_eq!((empty.width, empty.height), (0, 0));

        let area = Rect::new(2, 3, 100, 10);
        let [left, right] = split_horizontal(area, 30);
        assert_eq!((left.x, left.width, right.x, right.width), (2, 30, 32, 70));
        let thirds = split_horizontal_thirds(area);
        assert_eq!(thirds.iter().map(|r| r.width).sum::<u16>(), 100);
        assert!(thirds.iter().all(|r| r.height == 10 && r.y == 3));
    }

    #[test]
    fn key_value_rows_align_their_values() {
        let short = line_text(&kv_line("Merged", "12", Color::Green));
        let long = line_text(&kv_line("Median cycle time", "3.0d", Color::Green));
        assert_eq!(short.find("12"), long.find("3.0d"));
        assert_eq!(
            kv_line("k", "v", Color::Red).spans[1].style.fg,
            Some(Color::Red)
        );
        assert!(line_text(&section_header("Signals", 4)).ends_with("Signals  ────"));
    }

    #[test]
    fn scroll_is_clamped_to_the_lines_below_the_viewport() {
        let draw = |height: u16, lines: usize, scroll: u16| {
            let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
            let mut clamped = 0;
            terminal
                .draw(|f| {
                    let lines = (0..lines)
                        .map(|i| Line::from(format!("line {i}")))
                        .collect();
                    let area = Rect::new(0, 0, 40, height);
                    clamped = render_scrollable_lines(f, area, styled_block(" T "), lines, scroll);
                })
                .unwrap();
            let first_row: String = (1..39)
                .map(|x| terminal.backend().buffer()[(x, 1)].symbol().to_string())
                .collect();
            (clamped, first_row.trim().to_string())
        };
        // 10 lines in a 6-row block (4 visible): 6 lines can scroll away.
        assert_eq!(draw(6, 10, 0), (0, "line 0".into()));
        assert_eq!(draw(6, 10, 3), (3, "line 3".into()));
        assert_eq!(draw(6, 10, 99), (6, "line 6".into()));
        // Everything fits: no scroll at all.
        assert_eq!(draw(12, 10, 5).0, 0);
        // Nothing to draw into: the offset is left as it was.
        assert_eq!(draw(0, 10, 5).0, 5);
    }
}
