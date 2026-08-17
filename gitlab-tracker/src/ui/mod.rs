pub mod inspector;
pub mod table;
pub mod theme;
pub mod tracker;

use crate::app::{
    ActivePane, App, InputMode, InspectorView, LogTimeField, SortColumn, SortOrder, TrackerView,
    FILTER_PICKER_ENTRIES,
};
use crate::models::TrackedMr;
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
    // Bump the frame counter on every render so the spinner animates at full frame rate,
    // independently of the 1-second tick timer.
    app.spinner_frame = app.spinner_frame.wrapping_add(1);

    let chunks = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(chunks[0]);

    // --- Left Pane: Main Table ---
    let table = table::render_table(app, main_chunks[0]);
    f.render_stateful_widget(table, main_chunks[0], &mut app.table_state);

    // --- Right Column: split vertically when a tracker ticket is available ---
    let has_ticket = app
        .table_state
        .selected()
        .and_then(|i| app.mrs.get(i))
        .and_then(|mr| mr.linked_ticket.as_ref())
        .is_some();

    let right_chunks = if has_ticket {
        // 2/3 Inspector (top) + 1/3 Tracker (bottom)
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(67), Constraint::Percentage(33)])
            .split(main_chunks[1])
    } else {
        // Full height for Inspector only
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100)])
            .split(main_chunks[1])
    };

    let inspector_area = right_chunks[0];
    let tracker_area = if has_ticket {
        Some(right_chunks[1])
    } else {
        None
    };

    // --- Inspector Pane (upper-right) ---
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

    // Resolve the selected MR from the *filtered* list before any mutable access to `app`.
    // The iterator returned by visible_mrs() holds an immutable borrow on `app`, so we
    // materialise the clone in a plain `let` binding — this drops the iterator (and the
    // borrow) immediately, before the `if let` block that writes back to `app`.
    let selected_inspector_mr: Option<TrackedMr> = app
        .table_state
        .selected()
        .and_then(|i| app.visible_mrs().nth(i).cloned());

    if let Some(mr) = selected_inspector_mr {
        let rendered_text = match app.inspector_view {
            InspectorView::MrInfo => inspector::render_safe_inspector_text(&mr, &app.config),
            InspectorView::Pipelines => inspector::render_pipelines_text(&mr),
        };

        app.inspector_content_lines = rendered_text.lines.len() as u16;
        app.inspector_pane_height = inspector_area.height.saturating_sub(2);

        let inspector_paragraph = Paragraph::new(rendered_text)
            .block(inspector_block)
            .wrap(Wrap { trim: false })
            .scroll((app.inspector_scroll, 0));
        f.render_widget(inspector_paragraph, inspector_area);
    } else if app.table_state.selected().is_some() {
        f.render_widget(
            Paragraph::new("Selected metadata unavailable.").block(inspector_block),
            inspector_area,
        );
    } else {
        f.render_widget(
            Paragraph::new(
                "Select an active Merge Request row to display side inspector panels context.",
            )
            .block(inspector_block)
            .dark_gray(),
            inspector_area,
        );
    }

    // --- Tracker Pane (lower-right) — only when a ticket is linked ---
    if let Some(area) = tracker_area {
        // Same pattern: resolve + clone before any mutable write to `app`.
        let selected_tracker_mr: Option<TrackedMr> = app
            .table_state
            .selected()
            .and_then(|i| app.visible_mrs().nth(i).cloned());

        if let Some(mr) = selected_tracker_mr {
            let tracker_is_active = app.active_pane == ActivePane::Tracker;

            let tracker_title = match (tracker_is_active, app.tracker_view) {
                (true, TrackerView::TicketInfo) => {
                    " Tracker [FOCUS] │ [P]: Time Log │ [L]: Log Time │ [T]: Open URL "
                }
                (false, TrackerView::TicketInfo) => " Tracker │ [T]: Focus ",
                (true, TrackerView::TimeLog) => {
                    " Time Log [FOCUS] │ [P]: Ticket Info │ [L]: Log Time "
                }
                (false, TrackerView::TimeLog) => " Time Log │ [T]: Focus ",
            };

            let tracker_block = Block::default()
                .borders(Borders::ALL)
                .border_style(pane_border_style(tracker_is_active))
                .title(tracker_title);

            let rendered_text = match app.tracker_view {
                TrackerView::TicketInfo => tracker::render_ticket_info(&mr, &app.tracker_colors),
                TrackerView::TimeLog => tracker::render_time_log(&mr, &app.time_entries),
            };

            app.tracker_content_lines = rendered_text.lines.len() as u16;
            app.tracker_pane_height = area.height.saturating_sub(2);

            let tracker_paragraph = Paragraph::new(rendered_text)
                .block(tracker_block)
                .wrap(Wrap { trim: false })
                .scroll((app.tracker_scroll, 0));
            f.render_widget(tracker_paragraph, area);
        }
    }

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
        ActivePane::Tracker => "Pane: Tracker",
    };
    // The input bar title and border change depending on whether the field has focus.
    let (input_title, input_border_style) = match app.input_mode {
        InputMode::Editing if !app.milestone_suggestions.is_empty() => (
            " MILESTONE │ [↑/↓ Tab]: Navigate │ [Enter]: Bulk-add MRs │ [Esc]: Close ".to_string(),
            Style::default().fg(Color::Yellow),
        ),
        InputMode::Editing => (
            " INSERT │ MR ID, branch name, or @milestone │ [Enter]: Confirm │ [Esc]: Cancel ".to_string(),
            Style::default().fg(Color::Yellow),
        ),
        InputMode::ColumnPicker => (
            " COLUMNS │ [↑/↓]: Navigate │ [Space]: Toggle │ [Esc]: Close & Save ".to_string(),
            Style::default().fg(Color::Cyan),
        ),
        InputMode::Normal if app.quit_confirm => (
            " Quit? Press [Esc] or [y] to confirm, any other key to cancel ".to_string(),
            Style::default().fg(Color::Red),
        ),
        InputMode::Normal => (
            format!(
                " [i] or [/]: Insert mode │ [Tab]: {} │ [S/s]: {} │ [F]: Filter │ [Space]: Flag │ [C]: Columns │ [▲/▼]: Scroll │ [O]: Open │ [R]: Refresh │ [Del]: Delete │ [Esc]: Quit ",
                pane_hint, sort_status
            ),
            Style::default(),
        ),
        InputMode::FilterPicker => (
            " FILTER │ [↑/↓]: Navigate │ [Enter]: Apply │ [Esc]: Cancel ".to_string(),
            Style::default().fg(Color::Green),
        ),
        // The Log Time popup handles its own rendering — the input bar is hidden behind it.
        // We still need to cover this arm to satisfy exhaustiveness.
        InputMode::LogTime => (
            " LOG TIME │ [Tab]: Next field │ [Enter]: Submit │ [Esc]: Cancel ".to_string(),
            Style::default().fg(Color::Magenta),
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

    // Render the filter picker popup on top of the UI when active.
    if app.input_mode == InputMode::FilterPicker {
        render_filter_picker(f, app, f.area());
    }

    // Render the milestone autocomplete dropdown above the input bar when suggestions exist.
    if app.input_mode == InputMode::Editing && !app.milestone_suggestions.is_empty() {
        render_milestone_autocomplete(f, app, chunks[1]);
    }

    // Render the Log Time popup on top of everything when active.
    if app.input_mode == InputMode::LogTime {
        render_log_time_popup(f, app, f.area());
    }
}

/// Renders the Log Time popup centred over the terminal.
///
/// The popup is a modal overlay using [`Clear`] so it erases whatever is beneath.
/// Layout (top→bottom):
///   1. Duration text field
///   2. Activity selector list (scrollable)
///   3. Comment text field
///   4. Error line (when present) + shortcut hint
fn render_log_time_popup(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::List;

    // Ticket context for the popup title.
    let ticket_label = app
        .table_state
        .selected()
        .and_then(|i| app.visible_mrs().nth(i))
        .and_then(|mr| mr.linked_ticket.as_ref())
        .map(|t| format!(" ⏱  Log Time — #{} ", t.id))
        .unwrap_or_else(|| " ⏱  Log Time ".to_string());

    // Fixed popup dimensions.
    let popup_width: u16 = 60;
    // Base height: title(1) + duration(3) + activity list (up to 6 visible) + comment(3) +
    // error/hint(2) + borders(2) = 17 rows max
    let activity_rows = (app.activities.len() as u16).clamp(2, 6);
    let popup_height: u16 = 3 + activity_rows + 3 + 2 + 2;

    let popup_x = area.x + area.width.saturating_sub(popup_width) / 2;
    let popup_y = area.y + area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    // Outer border block.
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Span::styled(
            ticket_label,
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(outer_block, popup_area);

    // Inner layout: split vertically into 4 zones inside the border.
    let inner = Rect::new(
        popup_area.x + 1,
        popup_area.y + 1,
        popup_area.width.saturating_sub(2),
        popup_area.height.saturating_sub(2),
    );

    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),             // Duration field
            Constraint::Length(activity_rows), // Activity selector
            Constraint::Length(3),             // Comment field
            Constraint::Min(1),                // Error / hint line
        ])
        .split(inner);

    // Helper: border colour based on whether the field is focused.
    let field_style = |focused: bool| -> Style {
        if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(theme::MUTED_HINT)
        }
    };

    // ── Duration field ────────────────────────────────────────────────────────
    let duration_focused = app.log_time_form.focused_field == LogTimeField::Duration;
    let duration_block = Block::default()
        .borders(Borders::ALL)
        .border_style(field_style(duration_focused))
        .title(Span::styled(
            " Duration (e.g. 1h30, 90m, 1.5h) ",
            Style::default().fg(Color::White),
        ));
    let duration_widget = Paragraph::new(app.log_time_form.duration_input.as_str())
        .block(duration_block)
        .style(Style::default().fg(Color::White));
    f.render_widget(duration_widget, zones[0]);

    // ── Activity selector ─────────────────────────────────────────────────────
    let activity_focused = app.log_time_form.focused_field == LogTimeField::Activity;
    let activity_block = Block::default()
        .borders(Borders::ALL)
        .border_style(field_style(activity_focused))
        .title(Span::styled(
            " Activity [↑/↓] ",
            Style::default().fg(Color::White),
        ));

    if app.activities.is_empty() {
        let loading = Paragraph::new("Loading activities…")
            .block(activity_block)
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(loading, zones[1]);
    } else {
        let cursor = app.log_time_form.selected_activity_idx;
        let visible = activity_rows as usize;
        let scroll_offset = if cursor >= visible {
            cursor + 1 - visible
        } else {
            0
        };

        let items: Vec<ListItem> = app
            .activities
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible)
            .map(|(i, act)| {
                let selected = i == cursor;
                let style = if selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(Span::styled(format!("  {} ", act.name), style)))
            })
            .collect();

        let list = List::new(items).block(activity_block);
        let mut list_state = ratatui::widgets::ListState::default();
        list_state.select(Some(cursor.saturating_sub(scroll_offset)));
        f.render_stateful_widget(list, zones[1], &mut list_state);
    }

    // ── Comment field ─────────────────────────────────────────────────────────
    let comment_focused = app.log_time_form.focused_field == LogTimeField::Comment;
    let comment_block = Block::default()
        .borders(Borders::ALL)
        .border_style(field_style(comment_focused))
        .title(Span::styled(
            " Comment (optional) ",
            Style::default().fg(Color::White),
        ));
    let comment_widget = Paragraph::new(app.log_time_form.comment_input.as_str())
        .block(comment_block)
        .style(Style::default().fg(Color::White));
    f.render_widget(comment_widget, zones[2]);

    // ── Error / hint line ─────────────────────────────────────────────────────
    let bottom_line = if let Some(err) = &app.log_time_form.error {
        Line::from(vec![
            Span::styled(
                " ✘ ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(err.clone(), Style::default().fg(Color::Red)),
        ])
    } else if app.log_time_form.submitting {
        Line::from(vec![Span::styled(
            " ⟳ Submitting…",
            Style::default().fg(Color::Yellow),
        )])
    } else {
        Line::from(vec![
            Span::styled(" [Tab] ", Style::default().fg(theme::MUTED_HINT)),
            Span::styled("Next field  ", Style::default().fg(theme::MUTED_HINT)),
            Span::styled("[Enter] ", Style::default().fg(theme::MUTED_HINT)),
            Span::styled("Submit  ", Style::default().fg(theme::MUTED_HINT)),
            Span::styled("[Esc] ", Style::default().fg(theme::MUTED_HINT)),
            Span::styled("Cancel", Style::default().fg(theme::MUTED_HINT)),
        ])
    };
    f.render_widget(Paragraph::new(bottom_line), zones[3]);
}

/// Renders the milestone autocomplete dropdown just above the input bar.
///
/// The popup lists all matching milestone suggestions and highlights the currently
/// selected one. It is anchored to the left edge of the input bar and grows upward
/// so it never overlaps the input field itself.
fn render_milestone_autocomplete(f: &mut Frame, app: &App, input_area: Rect) {
    let suggestions = &app.milestone_suggestions;
    if suggestions.is_empty() {
        return;
    }

    // Cap visible rows to avoid overflowing the screen.
    let max_visible: u16 = 8;
    let visible_count = (suggestions.len() as u16).min(max_visible);
    // +2 for top/bottom borders.
    let popup_height = visible_count + 2;
    let popup_width = (input_area.width / 2).max(40);

    // Anchor to the left of the input bar and grow upward.
    let popup_x = input_area.x;
    let popup_y = input_area.y.saturating_sub(popup_height);
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    // Determine the scroll offset so the selected item is always visible.
    let cursor = app.milestone_suggestion_cursor;
    let scroll_offset = if cursor >= max_visible as usize {
        cursor + 1 - max_visible as usize
    } else {
        0
    };

    let items: Vec<ListItem> = suggestions
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(max_visible as usize)
        .map(|(i, title)| {
            let is_selected = i == cursor;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!("  {} ", title), style)))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Milestones │ [↑/↓ Tab]: Navigate │ [Enter]: Select "),
    );

    let mut list_state = ListState::default();
    list_state.select(Some(cursor.saturating_sub(scroll_offset)));
    f.render_stateful_widget(list, popup_area, &mut list_state);
}

/// Renders the filter picker popup centred over the terminal area.
///
/// Displays all available filter entries. Entries 13 (Milestone) and 14 (Assignee)
/// show an inline text input field beneath the list when selected.
fn render_filter_picker(f: &mut Frame, app: &App, area: Rect) {
    let entries = FILTER_PICKER_ENTRIES;
    let needs_text_input = matches!(app.filter_picker.cursor, 15 | 16);

    // Height: one row per entry + borders + optional text input row (3 lines).
    let list_height = entries.len() as u16;
    let input_extra: u16 = if needs_text_input { 3 } else { 0 };
    let popup_height = list_height + 2 + input_extra; // +2 for borders
    let popup_width: u16 = 48;

    let popup_x = area.x + area.width.saturating_sub(popup_width) / 2;
    let popup_y = area.y + area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    // Outer border.
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " Filter ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    f.render_widget(outer_block, popup_area);

    // Inner area (inside the border).
    let inner = Rect::new(
        popup_area.x + 1,
        popup_area.y + 1,
        popup_area.width.saturating_sub(2),
        popup_area.height.saturating_sub(2),
    );

    // Split inner area into list + optional text field.
    let zones = if needs_text_input {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .split(inner)
    };

    // Build list items.
    let cursor = app.filter_picker.cursor;

    // Pre-compute availability of context-dependent filters so they can be
    // rendered as dimmed when no data makes them applicable. The indices must
    // stay stable (no entries are hidden) to keep the cursor math correct.
    let has_any_linked_ticket = app.mrs.iter().any(|mr| mr.linked_ticket.is_some());
    let has_any_pipeline = app.mrs.iter().any(|mr| !mr.pipelines.is_empty());

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let is_active = i == cursor;
            // Mark the currently applied filter with a bullet.
            let is_current = {
                use crate::app::FilterMode;
                use crate::models::{GitlabMrState, MergeabilityStatus};
                match i {
                    0 => app.filter_mode == FilterMode::All,
                    1 => app.filter_mode == FilterMode::Flagged,
                    2 => app.filter_mode == FilterMode::State(GitlabMrState::Opened),
                    3 => app.filter_mode == FilterMode::State(GitlabMrState::Merged),
                    4 => app.filter_mode == FilterMode::State(GitlabMrState::Closed),
                    5 => app.filter_mode == FilterMode::Mergeability(MergeabilityStatus::Mergeable),
                    6 => app.filter_mode == FilterMode::Mergeability(MergeabilityStatus::Conflict),
                    7 => {
                        app.filter_mode == FilterMode::Mergeability(MergeabilityStatus::NeedsRebase)
                    }
                    8 => {
                        app.filter_mode == FilterMode::Mergeability(MergeabilityStatus::NotApproved)
                    }
                    9 => {
                        app.filter_mode
                            == FilterMode::Mergeability(MergeabilityStatus::RequestedChanges)
                    }
                    10 => app.filter_mode == FilterMode::Mergeability(MergeabilityStatus::Draft),
                    11 => {
                        app.filter_mode
                            == FilterMode::Mergeability(MergeabilityStatus::DiscussionsNotResolved)
                    }
                    12 => app.filter_mode == FilterMode::HasNotes,
                    13 => app.filter_mode == FilterMode::HasLinkedTicket,
                    14 => app.filter_mode == FilterMode::CiFailing,
                    15 => matches!(app.filter_mode, FilterMode::Milestone(_)),
                    16 => matches!(app.filter_mode, FilterMode::Assignee(_)),
                    _ => false,
                }
            };

            // Entries that are inapplicable in the current context are dimmed
            // (n/a label suffix) to inform the user without removing them —
            // keeping all indices stable avoids any cursor mapping bug.
            let is_na = match i {
                13 => !has_any_linked_ticket,
                14 => !has_any_pipeline,
                _ => false,
            };

            let prefix = if is_current { "● " } else { "  " };
            let display_label = if is_na {
                format!("{}{}  (n/a)", prefix, label)
            } else {
                format!("{}{}", prefix, label)
            };
            let style = if is_na {
                // Dimmed regardless of cursor position — not useful to select.
                Style::default().fg(Color::Rgb(90, 90, 90))
            } else if is_active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if is_current {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(display_label, style)))
        })
        .collect();

    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(cursor));
    f.render_stateful_widget(list, zones[0], &mut list_state);

    // Text input field for Milestone / Assignee entries.
    if needs_text_input {
        let field_label = if app.filter_picker.cursor == 15 {
            " Milestone contains "
        } else {
            " Assignee contains "
        };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(field_label, Style::default().fg(Color::White)));
        let input_widget = Paragraph::new(app.filter_picker.input.as_str())
            .block(input_block)
            .style(Style::default().fg(Color::White));
        f.render_widget(input_widget, zones[1]);
    }
}

/// Renders the column-picker popup centred over the terminal area.
///
/// The popup is overlaid via [`Clear`] so it erases whatever is beneath it.
/// Arrow keys move the cursor; Space toggles the highlighted entry.
fn render_column_picker(f: &mut Frame, app: &App, area: Rect) {
    // The popup entries mirror the fields of `VisibleColumns` in declaration order.
    // The "Tracker" entry is appended only when a tracker provider is configured at runtime.
    let mut entries_vec: Vec<(&str, bool)> = vec![
        ("Activity", app.config.visible_columns.activity),
        ("Target branch", app.config.visible_columns.target_branch),
        ("Labels", app.config.visible_columns.labels),
        ("Milestone", app.config.visible_columns.milestone),
        ("Notes", app.config.visible_columns.notes),
    ];
    if app.tracker.is_some() {
        entries_vec.push(("Tracker", app.config.visible_columns.tracker_ticket));
    }
    let entries: &[(&str, bool)] = &entries_vec;

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
