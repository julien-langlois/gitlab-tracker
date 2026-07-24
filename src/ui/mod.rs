pub mod inspector;
pub mod table;

use crate::app::{App, SortColumn, SortOrder};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::Stylize,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render_ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(chunks[0]);

    // --- Left Pane: Main Table ---
    let table = table::render_table(app);
    f.render_stateful_widget(table, main_chunks[0], &mut app.table_state);

    // --- Right Pane: Context Inspector Panel ---
    let inspector_block = Block::default()
        .borders(Borders::ALL)
        .title(" MR Context Inspector ");

    if let Some(selected) = app.table_state.selected() {
        if let Some(mr) = app.mrs.get(selected) {
            let rendered_text = inspector::render_safe_inspector_text(mr, &app.config);
            let inspector_paragraph = Paragraph::new(rendered_text)
                .block(inspector_block)
                .wrap(Wrap { trim: false });
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
    let input_help = format!(
        "Input: '142' / 'develop' │ [S/s]: {} │ [▲/▼]: Scroll │ [O]: Open │ [R]: Refresh │ [Del/X]: Delete │ [ESC]: Quit",
        sort_status
    );

    let input_box = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(input_help));
    f.render_widget(input_box, chunks[1]);
}
