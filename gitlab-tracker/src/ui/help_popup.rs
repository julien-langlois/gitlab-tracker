use crate::app::App;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

// Column separator rendered between the two grid columns.
const COL_SEPARATOR: &str = "  │  ";
const COL_SEPARATOR_WIDTH: usize = 5;

// Gutter between the key badge and its description inside one cell.
const CELL_GUTTER: &str = "  ";
const CELL_GUTTER_WIDTH: usize = 2;

/// Renders the help popup centred over the terminal.
///
/// Layout per section:
///   - 1 column  when the inner width cannot fit two cells without truncation.
///   - 2 columns when there is enough room (each cell ≥ `key_w + GUTTER + desc_w`).
///
/// All key badges are right-aligned to the same width; all descriptions are
/// left-aligned to the same width. Both widths are computed from the actual
/// content so nothing is ever truncated.
///
/// Any key press closes the popup (handled in `events.rs`).
pub fn render_help_popup(f: &mut Frame, app: &App) {
    let area = f.area();

    // ── Measure actual content widths across ALL providers ─────────────────────
    // We compute global key/desc widths so the alignment is consistent even
    // across section boundaries (looks cleaner when sections share the grid).
    let max_key_w: usize = app
        .shortcut_providers
        .iter()
        .flat_map(|b| b.entries.iter())
        .map(|e| e.key.chars().count())
        .max()
        .unwrap_or(4);

    let max_desc_w: usize = app
        .shortcut_providers
        .iter()
        .flat_map(|b| b.entries.iter())
        .map(|e| e.description.chars().count())
        .max()
        .unwrap_or(10);

    // Width of one fully rendered cell (key + gutter + description).
    let cell_w = max_key_w + CELL_GUTTER_WIDTH + max_desc_w;

    // ── Popup geometry ─────────────────────────────────────────────────────────
    // Ideal width: 2 columns + separator + 2 side paddings (1 each).
    // Falls back to 1-column width if the terminal is too narrow.
    let two_col_content_w = (cell_w * 2 + COL_SEPARATOR_WIDTH + 2) as u16;

    // Choose the popup width: prefer 2-column layout; clamp to terminal width.
    let desired_w = two_col_content_w + 2; // +2 for borders
    let popup_width = desired_w.min(area.width.saturating_sub(4));

    // Derive the actual number of columns from the popup inner width.
    let inner_w = popup_width.saturating_sub(2); // minus borders
    let n_cols: usize = if inner_w >= two_col_content_w { 2 } else { 1 };

    // Height: sum of all section header + blank after header + entries + blank lines, clamped to 90%.
    let total_content_lines: u16 = app
        .shortcut_providers
        .iter()
        .map(|b| {
            let rows = (b.entries.len() as u16).div_ceil(n_cols as u16);
            1 + 1 + rows + 1 // header + blank after header + rows + blank separator
        })
        .sum::<u16>()
        + 1; // footer hint

    let max_height = (area.height as f32 * 0.90) as u16;
    let popup_height = (total_content_lines + 2).min(max_height); // +2 for borders

    let popup_x = area.x + area.width.saturating_sub(popup_width) / 2;
    let popup_y = area.y + area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    // Outer border with title.
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " ⌨  Keyboard Shortcuts — [any key]: Close ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(outer_block, popup_area);

    // Inner usable area (inside border), with 1-char side padding.
    let inner = Rect::new(
        popup_area.x + 1,
        popup_area.y + 1,
        popup_area.width.saturating_sub(2),
        popup_area.height.saturating_sub(2),
    );

    // ── Build lines ────────────────────────────────────────────────────────────
    // The section header must fill the exact pixel-width of the inner area.
    // We use `inner.width` (terminal columns) rather than a char-count derived
    // from content, because Unicode chars like →/★ are 1 char but 1 column,
    // while the format padding operates on chars — both coincide for ASCII but
    // diverge otherwise, leaving a gap on the right edge.
    let header_w = inner.width as usize;

    let mut lines: Vec<Line<'static>> = Vec::new();

    for block in &app.shortcut_providers {
        // Section header — fills the full inner width, title centred, uppercase.
        let title = block.section.to_uppercase();
        lines.push(
            Line::from(vec![Span::styled(
                format!("{:^width$}", title, width = header_w),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )])
            .alignment(Alignment::Left),
        );

        // Blank line after the section header for readability.
        lines.push(Line::from(Span::raw("")));

        // Distribute entries row-first across n_cols columns.
        for row_entries in block.entries.chunks(n_cols) {
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(n_cols * 4);

            for (col_idx, entry) in row_entries.iter().enumerate() {
                // Column separator (except before the first column).
                if col_idx > 0 {
                    spans.push(Span::styled(
                        COL_SEPARATOR,
                        Style::default().fg(crate::ui::theme::MUTED_DIM),
                    ));
                }

                // Key badge — right-aligned within key column, bold yellow.
                spans.push(Span::styled(
                    format!("{:>width$}", entry.key, width = max_key_w),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));

                // Gutter between key and description.
                spans.push(Span::raw(CELL_GUTTER));

                // Description — left-aligned within desc column.
                // Pad only when this is the left cell in a 2-column layout,
                // so the separator lands at a fixed position.
                let desc = if n_cols == 2 && col_idx == 0 {
                    format!("{:<width$}", entry.description, width = max_desc_w)
                } else {
                    entry.description.to_string()
                };
                spans.push(Span::styled(desc, Style::default().fg(Color::White)));
            }

            lines.push(Line::from(spans));
        }

        // Blank separator between sections.
        lines.push(Line::from(Span::raw("")));
    }

    // ── Render ─────────────────────────────────────────────────────────────────
    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    f.render_widget(Paragraph::new(lines), zones[0]);

    // Footer hint — centred.
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Press any key to close",
            Style::default().fg(crate::ui::theme::MUTED_HINT),
        )]))
        .alignment(Alignment::Center),
        zones[1],
    );
}
