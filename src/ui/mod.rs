pub mod inspector;
pub mod table;

use crate::app::{ActivePane, App, InputMode, InspectorView, SortColumn, SortOrder};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
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
    let inspector_title = match (inspector_is_active, app.inspector_view) {
        (true, InspectorView::MrInfo) => " MR Inspector [FOCUS] │ [P]: Pipelines ",
        (false, InspectorView::MrInfo) => " MR Inspector │ [P]: Pipelines ",
        (true, InspectorView::Pipelines) => " Pipelines [FOCUS] │ [P]: MR Info ",
        (false, InspectorView::Pipelines) => " Pipelines │ [P]: MR Info ",
    };
    let inspector_block = Block::default()
        .borders(Borders::ALL)
        .border_style(pane_border_style(inspector_is_active))
        .title(inspector_title);

    if let Some(selected) = app.table_state.selected() {
        if let Some(mr) = app.mrs.get(selected) {
            // Dispatch to the correct inspector view based on the current toggle state.
            let rendered_text = match app.inspector_view {
                InspectorView::MrInfo => inspector::render_safe_inspector_text(mr, &app.config),
                InspectorView::Pipelines => inspector::render_pipelines_text(mr),
            };

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
    // The input bar title and border change depending on whether the field has focus.
    let (input_title, input_border_style) = match app.input_mode {
        InputMode::Editing => (
            " INSERT │ type MR ID or branch name │ [Enter]: Confirm │ [Esc]: Cancel ".to_string(),
            Style::default().fg(Color::Yellow),
        ),
        InputMode::ColumnPicker => (
            " COLUMNS │ [↑/↓]: Navigate │ [Space]: Toggle │ [Esc]: Close & Save ".to_string(),
            Style::default().fg(Color::Cyan),
        ),
        InputMode::Normal => (
            format!(
                " [i] or [/]: Insert mode │ [Tab]: {} │ [S/s]: {} │ [C]: Columns │ [▲/▼]: Scroll │ [O]: Open │ [R]: Refresh │ [Del/X]: Delete │ [Esc]: Quit ",
                pane_hint, sort_status
            ),
            Style::default(),
        ),
    };

    let input_box = Paragraph::new(app.input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(input_border_style)
            .title(input_title),
    );
    f.render_widget(input_box, chunks[1]);

    // Render the column-picker popup on top of the UI when active.
    if app.input_mode == InputMode::ColumnPicker {
        render_column_picker(f, app, f.area());
    }
}

/// Renders the column-picker popup centred over the terminal area.
///
/// The popup is overlaid via [`Clear`] so it erases whatever is beneath it.
/// Arrow keys move the cursor; Space toggles the highlighted entry.
fn render_column_picker(f: &mut Frame, app: &App, area: Rect) {
    // The popup entries mirror the fields of `VisibleColumns` in declaration order.
    let entries: &[(&str, bool)] = &[
        ("Activity", app.config.visible_columns.activity),
        ("Target branch", app.config.visible_columns.target_branch),
        ("Labels", app.config.visible_columns.labels),
        ("Milestone", app.config.visible_columns.milestone),
        ("Notes", app.config.visible_columns.notes),
    ];

    // Fixed popup size: wide enough for the longest label + checkbox, tall enough for all rows.
    let popup_width: u16 = 36;
    let popup_height: u16 = entries.len() as u16 + 2; // +2 for top/bottom borders

    // Centre the popup within the terminal area.
    let popup_x = area.x + area.width.saturating_sub(popup_width) / 2;
    let popup_y = area.y + area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Erase the background behind the popup.
    f.render_widget(Clear, popup_area);

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, (label, enabled))| {
            let checkbox = if *enabled { "☑" } else { "☐" };
            let is_selected = i == app.column_picker_cursor;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {} ", checkbox), style),
                Span::styled(label.to_string(), style),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Columns "),
    );

    let mut list_state = ListState::default();
    list_state.select(Some(app.column_picker_cursor));
    f.render_stateful_widget(list, popup_area, &mut list_state);
}
