pub mod inspector;
pub mod table;

use crate::app::{ActivePane, App, SortColumn, SortOrder};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Returns the border style to apply to a pane based on whether it is active.
///
/// Active pane gets a highlighted (cyan) border so the user knows where focus is.
fn pane_border_style(is_active: bool) -> Style {
    if is_active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    }
}

pub fn render_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(chunks[0]);

    // --- Left Pane: Main Table ---
    // The table block highlights its border when it is the active pane.
    let table = table::render_table(app, main_chunks[0]);
    f.render_stateful_widget(table, main_chunks[0], &mut app.table_state);

    // --- Right Pane: Context Inspector Panel ---
    let inspector_is_active = app.active_pane == ActivePane::Inspector;
    let inspector_title = if inspector_is_active {
        " MR Context Inspector [FOCUS] "
    } else {
        " MR Context Inspector "
    };
    let inspector_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border_style(inspector_is_active))
        .title(inspector_title);

    if let Some(selected) = app.table_state.selected() {
        if let Some(mr) = app.mrs.get(selected) {
            let rendered_text = inspector::render_safe_inspector_text(mr, &app.config);

            // Update content/pane dimensions so scroll clamping in App is accurate.
            // Inner height removes the 2 border rows (top + bottom).
            app.inspector_content_lines = rendered_text.lines.len() as u16;
            app.inspector_pane_height = main_chunks[1].height.saturating_sub(2);

            let inspector_paragraph = Paragraph::new(rendered_text)
                .block(inspector_block)
                .wrap(Wrap { trim: false })
                .scroll((app.inspector_scroll, 0));
            f.render_widget(inspector_paragraph, main_chunks[1]);
        } else {
            let inspector_paragraph =
                Paragraph::new("Selected metadata unavailable.").block(inspector_block);
            f.render_widget(inspector_paragraph, main_chunks[1]);
        }
    } else {
        let inspector_paragraph = Paragraph::new(
            "Select an active Merge Request row to display side inspector panels context.",
        )
        .block(inspector_block)
        .dark_gray();
        f.render_widget(inspector_paragraph, main_chunks[1]);
    };

    let sort_status = match (app.sort_column, app.sort_order) {
        (SortColumn::UpdatedAt, SortOrder::Ascending) => "Sort: Updated ▲",
        (SortColumn::UpdatedAt, SortOrder::Descending) => "Sort: Updated ▼",
        (SortColumn::Id, SortOrder::Ascending) => "Sort: ID ▲",
        (SortColumn::Id, SortOrder::Descending) => "Sort: ID ▼",
        (SortColumn::Milestone, SortOrder::Ascending) => "Sort: Milestone ▲",
        (SortColumn::Milestone, SortOrder::Descending) => "Sort: Milestone ▼",
        (SortColumn::Title, SortOrder::Ascending) => "Sort: Title ▲",
        (SortColumn::Title, SortOrder::Descending) => "Sort: Title ▼",
    };

    // --- Bottom Input Bar ---
    let pane_hint = match app.active_pane {
        ActivePane::Dashboard => "Pane: Dashboard",
        ActivePane::Inspector => "Pane: Inspector",
    };
    let input_help = format!(
        "Input: '142' / 'develop' │ [Tab]: {} │ [S/s]: {} │ [▲/▼]: Scroll │ [O]: Open │ [R]: Refresh │ [Del/X]: Delete │ [ESC]: Quit",
        pane_hint, sort_status
    );

    let input_box = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(input_help));
    f.render_widget(input_box, chunks[1]);
}
