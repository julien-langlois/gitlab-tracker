//! Popups and overlays drawn over the main layout: Log Time, milestone
//! autocomplete, filter picker, column picker and project settings.
//! (The help popup lives in `help_popup.rs`.)

use super::centered_popup;
use crate::app::{App, LogTimeField};
use gitlab_tracker_core::{ProjectSettingKind, ProjectSettingValue};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

/// Renders the Log Time popup centred over the terminal.
///
/// The popup is a modal overlay using [`Clear`] so it erases whatever is beneath.
/// Layout (top→bottom):
///   1. Duration text field
///   2. Activity selector list (scrollable)
///   3. Comment text field
///   4. Error line (when present) + shortcut hint
pub(super) fn render_log_time_popup(f: &mut Frame, app: &App, area: Rect) {
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

    let popup_area = centered_popup(area, popup_width, popup_height);

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
    // Uses the active palette so unfocused borders adapt to dark/light themes.
    let muted_hint = app.theme.muted_hint;
    let field_style = |focused: bool| -> Style {
        if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(muted_hint)
        }
    };

    // ── Duration field ────────────────────────────────────────────────────────
    let duration_focused = app.log_time_form.focused_field == LogTimeField::Duration;
    let duration_block = Block::default()
        .borders(Borders::ALL)
        .border_style(field_style(duration_focused))
        .title(Span::styled(
            " Duration (e.g. 1h30, 90m, 1.5h) ",
            Style::default().fg(crate::ui::theme::fg()),
        ));
    let duration_widget = Paragraph::new(app.log_time_form.duration_input.as_str())
        .block(duration_block)
        .style(Style::default().fg(crate::ui::theme::fg()));
    f.render_widget(duration_widget, zones[0]);

    // ── Activity selector ─────────────────────────────────────────────────────
    let activity_focused = app.log_time_form.focused_field == LogTimeField::Activity;
    let activity_block = Block::default()
        .borders(Borders::ALL)
        .border_style(field_style(activity_focused))
        .title(Span::styled(
            " Activity [↑/↓] ",
            Style::default().fg(crate::ui::theme::fg()),
        ));

    if app.activities.is_empty() {
        // Failed fetch: show why (reopening the popup retries) instead of an endless
        // "Loading…".
        let (text, color) = match &app.activities_error {
            Some(error) => (format!("Failed to load activities: {error}"), Color::Red),
            None => ("Loading activities…".to_string(), Color::DarkGray),
        };
        let loading = Paragraph::new(text)
            .block(activity_block)
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: true });
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
                    Style::default().fg(crate::ui::theme::fg())
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
            Style::default().fg(crate::ui::theme::fg()),
        ));
    let comment_widget = Paragraph::new(app.log_time_form.comment_input.as_str())
        .block(comment_block)
        .style(Style::default().fg(crate::ui::theme::fg()));
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
            Span::styled(" [Tab] ", Style::default().fg(muted_hint)),
            Span::styled("Next field  ", Style::default().fg(muted_hint)),
            Span::styled("[Enter] ", Style::default().fg(muted_hint)),
            Span::styled("Submit  ", Style::default().fg(muted_hint)),
            Span::styled("[Esc] ", Style::default().fg(muted_hint)),
            Span::styled("Cancel", Style::default().fg(muted_hint)),
        ])
    };
    f.render_widget(Paragraph::new(bottom_line), zones[3]);
}

/// Renders the milestone autocomplete dropdown just above the input bar.
///
/// The popup lists all matching milestone suggestions and highlights the currently
/// selected one. It is anchored to the left edge of the input bar and grows upward
/// so it never overlaps the input field itself.
pub(super) fn render_milestone_autocomplete(f: &mut Frame, app: &App, input_area: Rect) {
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
    // Clamp so a narrow or short terminal never pushes the dropdown off-screen.
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height).clamp(f.area());

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
                Style::default().fg(crate::ui::theme::fg())
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
/// Iterates `app.filter_defs` (collected via `inventory` at startup) — no hardcoded
/// index mapping needed. Plugin filters (e.g. Redmine's "Has linked ticket") appear
/// automatically when their crate is linked.
pub(super) fn render_filter_picker(f: &mut Frame, app: &App, area: Rect) {
    // Use only the filters visible for the current project configuration.
    // Username-gated filters ("Assigned to me", "Reviewer: me") are excluded
    // when `gitlab_username` is not set in projects.toml.
    // visible_filters: Vec<(full_list_index, &FilterDef)>
    // The cursor navigates the *visible* list; full_list_index is used to detect
    // is_current (the ● marker) and is forwarded to apply_filter_picker.
    let visible_filters = app.visible_filter_defs();
    let cursor = app.filter_picker.cursor;
    let needs_text_input = visible_filters
        .get(cursor)
        .map(|(_, def)| def.needs_text_input)
        .unwrap_or(false);

    let list_height = visible_filters.len() as u16;
    let input_extra: u16 = if needs_text_input { 3 } else { 0 };
    let popup_height = list_height + 2 + input_extra;
    let popup_width: u16 = 48;

    let popup_area = centered_popup(area, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

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

    let inner = Rect::new(
        popup_area.x + 1,
        popup_area.y + 1,
        popup_area.width.saturating_sub(2),
        popup_area.height.saturating_sub(2),
    );

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

    // Pre-compute applicability hints for context-dependent filters.
    let has_any_linked_ticket = app.mrs.iter().any(|mr| mr.linked_ticket.is_some());
    let has_any_pipeline = app.mrs.iter().any(|mr| !mr.pipelines.is_empty());

    let items: Vec<ListItem> = visible_filters
        .iter()
        .enumerate()
        .map(|(visible_i, (full_i, def))| {
            let is_active = visible_i == cursor;
            // ● marker: compare against the full-list index stored in active_filter.
            let is_current = *full_i == app.active_filter.index;

            // Dim context-dependent filters when they are not applicable.
            let is_na = match def.id {
                "has_linked_ticket" => !has_any_linked_ticket,
                "ci_failing" => !has_any_pipeline,
                _ => false,
            };

            let prefix = if is_current { "● " } else { "  " };
            let display_label = if is_na {
                format!("{}{}  (n/a)", prefix, def.label)
            } else {
                format!("{}{}", prefix, def.label)
            };

            let style = if is_na {
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
                Style::default().fg(crate::ui::theme::fg())
            };
            ListItem::new(Line::from(Span::styled(display_label, style)))
        })
        .collect();

    let list = List::new(items);
    let mut list_state = ListState::default();
    list_state.select(Some(cursor));
    f.render_stateful_widget(list, zones[0], &mut list_state);

    // Text input field for parametric filters (Milestone, Assignee, …).
    if needs_text_input {
        let field_label = visible_filters
            .get(cursor)
            .map(|(_, def)| def.active_label)
            .unwrap_or("Query");
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                format!(" {} ", field_label),
                Style::default().fg(crate::ui::theme::fg()),
            ));
        let input_widget = Paragraph::new(app.filter_picker.input.as_str())
            .block(input_block)
            .style(Style::default().fg(crate::ui::theme::fg()));
        f.render_widget(input_widget, zones[1]);
    }
}

/// Renders the column-picker popup centred over the terminal area.
///
/// Iterates `app.column_defs` (collected via `inventory` at startup) — no hardcoded
/// index mapping needed. Plugin columns (e.g. Redmine's "Tracker") appear automatically
/// when their crate is linked and their runtime `requires` condition is met.
pub(super) fn render_column_picker(f: &mut Frame, app: &App, area: Rect) {
    // Registered columns whose runtime requirement is met (`App::visible_column_defs`).
    let entries: Vec<(&str, bool)> = app
        .visible_column_defs()
        .into_iter()
        .map(|c| (c.label, app.config.visible_columns.is_visible(c.id)))
        .collect();

    let popup_width: u16 = 36;
    let popup_height: u16 = entries.len() as u16 + 2;

    let popup_area = centered_popup(area, popup_width, popup_height);

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
                Style::default().fg(crate::ui::theme::fg())
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

/// Renders the project settings popup centred over the terminal area.
///
/// Settings are collected through `inventory`, so optional crates can expose their
/// own project-level settings without changing this renderer.
pub(super) fn render_settings_popup(f: &mut Frame, app: &App, area: Rect) {
    let popup_width: u16 = area.width.saturating_sub(4).clamp(70, 110);
    let content_lines = settings_popup_line_count(app) as u16;
    let desired_height = content_lines.saturating_add(8).max(18);
    let max_height = (area.height as f32 * 0.95) as u16;
    let popup_height: u16 = desired_height.min(max_height).max(14);

    let popup_area = centered_popup(area, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let zones = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(popup_area);

    let header_w = zones[0].width.saturating_sub(2) as usize;
    let mut list_items: Vec<ListItem> = Vec::new();
    let mut current_section: Option<&str> = None;
    let mut selected_render_idx: Option<usize> = None;

    for (setting_idx, item) in app.settings_editor.items.iter().enumerate() {
        if current_section != Some(item.def.section) {
            current_section = Some(item.def.section);
            if !list_items.is_empty() {
                list_items.push(ListItem::new(Line::from(Span::raw(""))));
            }
            list_items.push(ListItem::new(Line::from(Span::styled(
                format!(
                    "{:^width$}",
                    item.def.section.to_uppercase(),
                    width = header_w
                ),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))));
            list_items.push(ListItem::new(Line::from(Span::raw(""))));
        }

        let is_selected = setting_idx == app.settings_editor.cursor;
        let style = if is_selected {
            selected_render_idx = Some(list_items.len());
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(crate::ui::theme::fg())
        };
        let value = render_setting_value(&item.value, &item.def.kind);
        list_items.push(ListItem::new(Line::from(vec![
            Span::styled("  ", style),
            Span::styled(format!("{:<32}", item.def.label), style),
            Span::styled(value, style),
        ])));
    }

    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Project Settings — [,]: Open │ [Enter]: Save │ [Esc]: Cancel "),
    );

    let mut list_state = ListState::default();
    list_state.select(selected_render_idx);
    f.render_stateful_widget(list, zones[0], &mut list_state);

    let help = Paragraph::new(format!(
        "{}\n[↑/↓]: Navigate  [←/→]: Adjust numbers  [Space]: Toggle  [Text]: Type  [Backspace]: Delete",
        app.settings_editor.selected_help()
    ))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Help "),
    )
    .wrap(Wrap { trim: true });
    f.render_widget(help, zones[1]);
}

fn settings_popup_line_count(app: &App) -> usize {
    let mut structural_lines = 0;
    let mut previous_section: Option<&str> = None;

    for item in &app.settings_editor.items {
        if previous_section != Some(item.def.section) {
            if structural_lines > 0 {
                // Blank separator between two sections.
                structural_lines += 1;
            }
            // Section header + blank margin before the first setting.
            structural_lines += 2;
            previous_section = Some(item.def.section);
        }
    }

    app.settings_editor.items.len() + structural_lines
}

fn render_setting_value(value: &ProjectSettingValue, kind: &ProjectSettingKind) -> String {
    match (value, kind) {
        (ProjectSettingValue::Bool(enabled), _) => if *enabled {
            "☑ enabled"
        } else {
            "☐ disabled"
        }
        .to_string(),
        (ProjectSettingValue::U64(value), ProjectSettingKind::U64 { step, .. }) => {
            format!("{}  (+/- {})", value, step)
        }
        (ProjectSettingValue::U32(value), ProjectSettingKind::U32 { step, .. }) => {
            format!("{}  (+/- {})", value, step)
        }
        (ProjectSettingValue::Text(value), _) if value.is_empty() => "<empty>".to_string(),
        (ProjectSettingValue::Text(value), _) => value.clone(),
        _ => "<invalid>".to_string(),
    }
}
