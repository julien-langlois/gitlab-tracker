pub mod cockpit;
pub mod help_popup;
pub mod inspector;
mod popups;
#[cfg(feature = "stats")]
pub mod stats;
pub mod status_bar;
pub mod table;
pub mod theme;
pub mod tracker;

use crate::app::{ActivePane, App, InputMode, InspectorView, SortColumn, SortOrder, TrackerView};
use help_popup::render_help_popup;
use popups::{
    render_column_picker, render_filter_picker, render_log_time_popup,
    render_milestone_autocomplete, render_settings_popup,
};

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
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

/// Centres a `width` × `height` popup in `area`, shrunk to fit when the terminal is
/// smaller than the popup (so borders and cursor never land off-screen).
pub(crate) fn centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    area.centered(Constraint::Length(width), Constraint::Length(height))
}

pub fn render_ui(f: &mut Frame, app: &mut App) {
    app.begin_render_cache();
    render_frame(f, app);
    app.end_render_cache();
}

fn render_frame(f: &mut Frame, app: &mut App) {
    // Bump the frame counter on every render so the spinner animates at full frame rate,
    // independently of the 1-second tick timer.
    app.layout.spinner_frame = app.layout.spinner_frame.wrapping_add(1);

    let chunks = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(chunks[0]);

    // --- Left Pane: status bar + MR table + optional cockpit pane ---
    // Keep the cockpit responsive: on small terminal heights the table keeps all
    // remaining space, while larger layouts get a tracker-sized operational pane.
    let show_cockpit = app.config.show_cockpit && main_chunks[0].height >= 22;
    let left_chunks = if show_cockpit {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Percentage(67),
                Constraint::Percentage(33),
            ])
            .split(main_chunks[0])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(main_chunks[0])
    };

    let status_bar = status_bar::render_status_bar(app);
    f.render_widget(status_bar, left_chunks[0]);

    // The table borrows `app`, so render against a copy of the (tiny) table state
    // and write it back once the table widget has been consumed.
    let mut table_state = app.table_state;
    f.render_stateful_widget(table::render_table(app), left_chunks[1], &mut table_state);
    app.table_state = table_state;

    if show_cockpit {
        cockpit::render_cockpit(f, app, left_chunks[2]);
    }

    // --- Right Column: split vertically when a tracker ticket is available ---
    let has_ticket = app.has_tracker_ticket();

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
    // Real pane positions, used by the mouse hit-test (`handle_mouse_event`).
    app.layout.inspector_area = inspector_area;
    app.layout.tracker_area = tracker_area;

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

    // Render the Inspector panel by borrowing the selected MR from the *filtered* list.
    //
    // Pattern: all reads from `app` (via the immutable borrow held by visible_mrs()) and
    // the rendering of Text<'static> happen inside a tight inner scope `{}`. Because
    // Text<'static> owns its content, it outlives the borrow — so once the scope ends the
    // iterator is dropped and `app` is free to be mutated (content_lines / pane_height).
    let inspector_render: Option<ratatui::text::Text<'static>> =
        app.table_state.selected().and_then(|i| {
            let mr = app.visible_mrs().nth(i)?;
            let text = match app.inspector_view {
                InspectorView::MrInfo => {
                    inspector::render_safe_inspector_text(mr, &app.config, app.theme)
                }
                InspectorView::Pipelines => inspector::render_pipelines_text(mr, app.theme),
            };
            Some(text)
        });

    match inspector_render {
        Some(rendered_text) => {
            let inspector_paragraph = Paragraph::new(rendered_text)
                .block(inspector_block)
                .wrap(Wrap { trim: false })
                .scroll((app.layout.inspector.offset, 0));
            // Wrapped (on-screen) line count, borders included — matches the pane height.
            app.layout.inspector.content_lines =
                u16::try_from(inspector_paragraph.line_count(inspector_area.width))
                    .unwrap_or(u16::MAX);
            app.layout.inspector.height = inspector_area.height;
            f.render_widget(inspector_paragraph, inspector_area);
        }
        None if app.table_state.selected().is_some() => {
            f.render_widget(
                Paragraph::new("Selected metadata unavailable.").block(inspector_block),
                inspector_area,
            );
        }
        None => {
            f.render_widget(
                Paragraph::new(
                    "Select an active Merge Request row to display side inspector panels context.",
                )
                .block(inspector_block)
                .dark_gray(),
                inspector_area,
            );
        }
    }

    // --- Tracker Pane (lower-right) — only when a ticket is linked ---
    if let Some(area) = tracker_area {
        // Same borrow-scope pattern: render Text<'static> inside the closure so the
        // immutable borrow on `app` ends before we write tracker_content_lines / pane_height.
        let tracker_render: Option<(ratatui::text::Text<'static>, bool)> =
            app.table_state.selected().and_then(|i| {
                let mr = app.visible_mrs().nth(i)?;
                let tracker_is_active = app.active_pane == ActivePane::Tracker;
                let text = match app.tracker_view {
                    TrackerView::TicketInfo => tracker::render_ticket_info(mr, &app.tracker_colors),
                    TrackerView::TimeLog => tracker::render_time_log(
                        mr,
                        mr.linked_ticket
                            .as_ref()
                            .and_then(|t| app.time_entries.get(&t.id)),
                        app.theme.muted_comment,
                    ),
                };
                Some((text, tracker_is_active))
            });

        if let Some((rendered_text, tracker_is_active)) = tracker_render {
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

            let tracker_paragraph = Paragraph::new(rendered_text)
                .block(tracker_block)
                .wrap(Wrap { trim: false })
                .scroll((app.layout.tracker.offset, 0));
            // Wrapped (on-screen) line count, borders included — matches the pane height.
            app.layout.tracker.content_lines =
                u16::try_from(tracker_paragraph.line_count(area.width)).unwrap_or(u16::MAX);
            app.layout.tracker.height = area.height;
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
            " INSERT │ MR ID, branch name, or @milestone │ [Enter]: Confirm │ [Esc]: Cancel "
                .to_string(),
            Style::default().fg(Color::Yellow),
        ),
        InputMode::ColumnPicker => (
            " COLUMNS │ [↑/↓]: Navigate │ [Space]: Toggle │ [Esc]: Close & Save ".to_string(),
            Style::default().fg(Color::Cyan),
        ),
        InputMode::Settings => (
            " SETTINGS │ [↑/↓]: Navigate │ [←/→]: Adjust │ [Space]: Toggle │ [Enter]: Save │ [Esc]: Cancel ".to_string(),
            Style::default().fg(Color::Cyan),
        ),
        InputMode::Normal if app.quit_confirm => (
            " Quit? Press [Esc] or [y] to confirm, any other key to cancel ".to_string(),
            Style::default().fg(Color::Red),
        ),
        InputMode::Normal => {
            // Collect static hints registered by all linked crates (Core, Stats, Redmine, …).
            // Only entries with `status_hint: Some(…)` appear here — optional crates that are
            // not compiled simply never register, so their hints never show up.
            let plugin_hints: Vec<&'static str> = app
                .shortcut_providers
                .iter()
                .flat_map(|block| block.entries.iter())
                .filter_map(|e| e.status_hint)
                // Skip the static hints we handle dynamically below (Insert, Scroll, Quit, …)
                // so they don't appear twice. The dynamic ones are injected at fixed positions.
                .filter(|h| !matches!(*h, "[i]/[/]: Insert" | "[▲/▼]: Scroll" | "[Esc]: Quit"))
                .collect();

            // Build the full bar: dynamic hints first, then plugin-contributed hints.
            // Borrowed `&str` parts: only the two dynamic hints and the joined title allocate.
            let tab_hint = format!("[Tab]: {pane_hint}");
            let sort_hint = format!("[S/s]: {sort_status}");
            let parts: Vec<&str> = ["[i] or [/]: Insert mode", "[,]: Settings", &tab_hint, &sort_hint]
                .into_iter()
                .chain(plugin_hints)
                .chain(["[▲/▼]: Scroll", "[Esc]: Quit"])
                .collect();

            let title = format!(" {} ", parts.join(" │ "));
            (title, Style::default())
        }
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
        // The Help popup covers the full screen — the input bar is hidden behind it.
        InputMode::Help => (
            " HELP │ [any key]: Close ".to_string(),
            Style::default().fg(Color::Cyan),
        ),
        // The Stats overlay covers the full screen — the input bar is hidden behind it.
        #[cfg(feature = "stats")]
        InputMode::Stats => (
            " STATS │ [W]: Window │ [↑/↓]: Scroll │ [G/Esc]: Close ".to_string(),
            Style::default().fg(Color::Cyan),
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

    // Render the settings popup on top of the UI when active.
    if app.input_mode == InputMode::Settings {
        render_settings_popup(f, app, f.area());
    }

    // Render the milestone autocomplete dropdown above the input bar when suggestions exist.
    if app.input_mode == InputMode::Editing && !app.milestone_suggestions.is_empty() {
        render_milestone_autocomplete(f, app, chunks[1]);
    }

    // Render the Log Time popup on top of everything when active.
    if app.input_mode == InputMode::LogTime {
        render_log_time_popup(f, app, f.area());
    }

    // Render the help popup on top of everything when active.
    if app.input_mode == InputMode::Help {
        render_help_popup(f, app);
    }

    // Render the Stats fullscreen overlay on top of everything when active.
    // Compiled only when the `stats` feature is enabled.
    #[cfg(feature = "stats")]
    if app.input_mode == InputMode::Stats {
        stats::render_stats_overlay(f, app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_popup_stays_inside_small_terminal() {
        let area = Rect::new(0, 0, 40, 12);
        let popup = centered_popup(area, 60, 17);
        assert_eq!(
            popup.intersection(area),
            popup,
            "popup overflows: {popup:?}"
        );
        // Fits: centred, full requested size.
        assert_eq!(
            centered_popup(Rect::new(0, 0, 100, 40), 60, 20),
            Rect::new(20, 10, 60, 20)
        );
    }
}
