use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::app::{
    ActivePane, App, FilterPickerState, InputMode, LogTimeField, LogTimeForm, TrackerView,
    FILTER_PICKER_ENTRIES,
};
use crate::gitlab::{spawn_milestone_mrs_fetch, spawn_mr_fetch, CachedMrData, FetchContext};
use crate::models::{AppEvent, MrStatus, TrackedMr};
use crate::storage::{save_config_async, save_state_async};
use crate::utils::parse_duration_to_hours;

/// Handles a mouse event and updates the application state accordingly.
///
/// Returns `true` when the selected dashboard row changed (scroll in the table pane),
/// so the caller can trigger a time-entries re-fetch when the TimeLog view is active.
pub fn handle_mouse_event(
    mouse: MouseEvent,
    term_width: u16,
    term_height: u16,
    app: &mut App,
    tx: &UnboundedSender<AppEvent>,
) {
    // The right column starts at 65% of the terminal width.
    let inspector_start_col = term_width * 65 / 100;
    // The Tracker pane occupies the bottom 33% of the right column.
    // Subtract 1 for the status bar at the bottom.
    let tracker_start_row = term_height.saturating_sub(1) * 67 / 100;
    // Whether the cursor is in the right column and below the Inspector pane.
    let in_tracker_pane = mouse.column >= inspector_start_col
        && app.has_tracker_ticket()
        && mouse.row >= tracker_start_row;

    match mouse.kind {
        // Update focus based on where the cursor is.
        MouseEventKind::Moved | MouseEventKind::Drag(_) => {
            if mouse.column >= inspector_start_col {
                if in_tracker_pane {
                    app.active_pane = ActivePane::Tracker;
                } else {
                    app.active_pane = ActivePane::Inspector;
                }
            } else {
                app.active_pane = ActivePane::Dashboard;
            }
        }
        // Route scroll to the pane under the cursor.
        MouseEventKind::ScrollDown => {
            if in_tracker_pane {
                app.tracker_scroll_down(3);
            } else if mouse.column >= inspector_start_col {
                app.inspector_scroll_down(3);
            } else {
                app.next_row();
                // When the TimeLog view is active, re-fetch time entries for the newly
                // selected MR's linked ticket — mirrors the keyboard ↓ / j handler.
                if app.tracker_view == TrackerView::TimeLog {
                    if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                        let ticket_id = app
                            .table_state
                            .selected()
                            .and_then(|i| app.visible_mrs().nth(i))
                            .and_then(|mr| mr.linked_ticket.as_ref())
                            .map(|t| t.id.clone());

                        if let Some(tid) = ticket_id {
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                let entries = provider.fetch_time_entries(&tid).await;
                                let _ = tx2
                                    .send(crate::models::AppEvent::TimeEntriesLoaded { entries });
                            });
                        }
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if in_tracker_pane {
                app.tracker_scroll_up(3);
            } else if mouse.column >= inspector_start_col {
                app.inspector_scroll_up(3);
            } else {
                app.prev_row();
                // When the TimeLog tracker view is active, re-fetch time entries.
                if app.tracker_view == TrackerView::TimeLog {
                    if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                        let ticket_id = app
                            .table_state
                            .selected()
                            .and_then(|i| app.visible_mrs().nth(i))
                            .and_then(|mr| mr.linked_ticket.as_ref())
                            .map(|t| t.id.clone());

                        if let Some(tid) = ticket_id {
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                let entries = provider.fetch_time_entries(&tid).await;
                                let _ = tx2
                                    .send(crate::models::AppEvent::TimeEntriesLoaded { entries });
                            });
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Handles a keyboard event and updates the application state accordingly.
///
/// Returns `true` if the main loop should exit (e.g. Esc was pressed).
pub async fn handle_key_event(
    key: KeyEvent,
    app: &mut App,
    api_semaphore: &Arc<Semaphore>,
    tx: &UnboundedSender<AppEvent>,
    last_known_branches: &mut HashMap<String, HashSet<String>>,
) -> bool {
    match app.input_mode {
        // ------------------------------------------------------------------
        // Editing mode: the input field has exclusive focus.
        // Only Esc, Enter, Backspace and printable chars are handled here.
        // ------------------------------------------------------------------
        InputMode::Editing => match key.code {
            // Esc closes autocomplete if open, otherwise cancels editing.
            KeyCode::Esc => {
                if !app.milestone_suggestions.is_empty() {
                    app.milestone_suggestions.clear();
                } else {
                    app.input.clear();
                    app.input_mode = InputMode::Normal;
                }
            }

            KeyCode::Enter => {
                // If an autocomplete suggestion is highlighted, confirm it and
                // immediately dispatch a bulk-add fetch for that milestone's MRs.
                if !app.milestone_suggestions.is_empty() {
                    if let Some(title) = app.confirm_milestone_suggestion() {
                        let ctx = build_fetch_context(app);
                        // Filter by milestone title — the GitLab MRs API uses the title,
                        // not the numeric milestone ID, for the `milestone` query parameter.
                        spawn_milestone_mrs_fetch(ctx, title, tx.clone());
                        app.input.clear();
                        app.input_mode = InputMode::Normal;
                    }
                } else {
                    handle_enter(app, api_semaphore, tx, last_known_branches).await;
                    // Return to Normal after submitting so shortcuts are available again.
                    app.input_mode = InputMode::Normal;
                }
            }

            // Navigate autocomplete suggestions with Tab / Shift+Tab.
            KeyCode::Tab => {
                if !app.milestone_suggestions.is_empty() {
                    app.milestone_suggestion_next();
                }
            }
            KeyCode::BackTab => {
                if !app.milestone_suggestions.is_empty() {
                    app.milestone_suggestion_prev();
                }
            }

            // Up/Down also navigate autocomplete when it is open.
            KeyCode::Down => {
                if !app.milestone_suggestions.is_empty() {
                    app.milestone_suggestion_next();
                }
            }
            KeyCode::Up => {
                if !app.milestone_suggestions.is_empty() {
                    app.milestone_suggestion_prev();
                }
            }

            KeyCode::Backspace => {
                app.input.pop();
                app.update_milestone_suggestions();
            }

            KeyCode::Char(c) => {
                app.input.push(c);
                app.update_milestone_suggestions();
            }

            _ => {}
        },

        // ------------------------------------------------------------------
        // Filter picker mode: the popup is open.
        // Up/Down move the cursor; Enter confirms; Backspace/chars edit text input
        // for Milestone and Assignee entries; Esc cancels without applying.
        // ------------------------------------------------------------------
        InputMode::FilterPicker => {
            const LAST_IDX: usize = FILTER_PICKER_ENTRIES.len() - 1;
            // Entries that require a text input (Milestone=13, Assignee=14).
            let needs_text_input = matches!(app.filter_picker.cursor, 13 | 14);

            match key.code {
                KeyCode::Esc => {
                    // Cancel: close popup without changing the active filter.
                    app.input_mode = InputMode::Normal;
                    app.filter_picker = FilterPickerState::default();
                }

                KeyCode::Enter => {
                    app.apply_filter_picker();
                }

                KeyCode::Up | KeyCode::Char('k') if !needs_text_input => {
                    if app.filter_picker.cursor > 0 {
                        app.filter_picker.cursor -= 1;
                        app.filter_picker.input.clear();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') if !needs_text_input => {
                    if app.filter_picker.cursor < LAST_IDX {
                        app.filter_picker.cursor += 1;
                        app.filter_picker.input.clear();
                    }
                }
                // Allow navigating away from a text-input row with arrow keys
                // even while the input is focused (Esc is the cancel path).
                KeyCode::Up => {
                    if app.filter_picker.cursor > 0 {
                        app.filter_picker.cursor -= 1;
                        app.filter_picker.input.clear();
                    }
                }
                KeyCode::Down => {
                    if app.filter_picker.cursor < LAST_IDX {
                        app.filter_picker.cursor += 1;
                        app.filter_picker.input.clear();
                    }
                }

                KeyCode::Backspace if needs_text_input => {
                    app.filter_picker.input.pop();
                }

                KeyCode::Char(c) if needs_text_input => {
                    app.filter_picker.input.push(c);
                }

                _ => {}
            }
        }

        // ------------------------------------------------------------------
        // Column-picker mode: the popup is open.
        // Up/Down move the cursor; Space toggles; Esc closes and persists.
        // ------------------------------------------------------------------
        InputMode::ColumnPicker => {
            // Column count: 5 fixed columns + 1 tracker column when a provider is configured.
            let column_count = if app.tracker.is_some() { 6 } else { 5 };

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.column_picker_cursor > 0 {
                        app.column_picker_cursor -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.column_picker_cursor < column_count - 1 {
                        app.column_picker_cursor += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    // Toggle the column at the current cursor position.
                    // Order must mirror `VisibleColumns` field declaration order
                    // and the entries array in `render_column_picker`.
                    match app.column_picker_cursor {
                        0 => {
                            app.config.visible_columns.activity =
                                !app.config.visible_columns.activity
                        }
                        1 => {
                            app.config.visible_columns.target_branch =
                                !app.config.visible_columns.target_branch
                        }
                        2 => app.config.visible_columns.labels = !app.config.visible_columns.labels,
                        3 => {
                            app.config.visible_columns.milestone =
                                !app.config.visible_columns.milestone
                        }
                        4 => app.config.visible_columns.notes = !app.config.visible_columns.notes,
                        // Entry 5 — only reachable when a tracker provider is configured.
                        5 => {
                            app.config.visible_columns.tracker_ticket =
                                !app.config.visible_columns.tracker_ticket
                        }
                        _ => {}
                    }
                }
                KeyCode::Esc | KeyCode::Enter => {
                    // Close the popup and persist the new column visibility to config.json.
                    app.input_mode = InputMode::Normal;
                    save_config_async(&app.config).await;
                }
                _ => {}
            }
        }

        // ------------------------------------------------------------------
        // Log Time popup mode — only reachable when a tracker provider is configured.
        // Tab/Shift+Tab cycle fields; Up/Down navigate activities; Enter submits.
        // ------------------------------------------------------------------
        InputMode::LogTime => {
            match key.code {
                KeyCode::Esc => {
                    // Close popup without submitting — reset the form.
                    app.input_mode = InputMode::Normal;
                    app.log_time_form = LogTimeForm::default();
                }

                // Tab / Shift+Tab cycle through fields: Duration → Activity → Comment → wrap.
                KeyCode::Tab => {
                    app.log_time_form.focused_field = match app.log_time_form.focused_field {
                        LogTimeField::Duration => LogTimeField::Activity,
                        LogTimeField::Activity => LogTimeField::Comment,
                        LogTimeField::Comment => LogTimeField::Duration,
                    };
                    app.log_time_form.error = None;
                }
                KeyCode::BackTab => {
                    app.log_time_form.focused_field = match app.log_time_form.focused_field {
                        LogTimeField::Duration => LogTimeField::Comment,
                        LogTimeField::Activity => LogTimeField::Duration,
                        LogTimeField::Comment => LogTimeField::Activity,
                    };
                    app.log_time_form.error = None;
                }

                // Up/Down navigate the activity list when that field is focused.
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.log_time_form.focused_field == LogTimeField::Activity
                        && !app.activities.is_empty()
                    {
                        let len = app.activities.len();
                        app.log_time_form.selected_activity_idx =
                            (app.log_time_form.selected_activity_idx + len - 1) % len;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if app.log_time_form.focused_field == LogTimeField::Activity
                        && !app.activities.is_empty()
                    {
                        let len = app.activities.len();
                        app.log_time_form.selected_activity_idx =
                            (app.log_time_form.selected_activity_idx + 1) % len;
                    }
                }

                // Enter: validate and submit when on Comment field, else advance field.
                KeyCode::Enter => {
                    match app.log_time_form.focused_field {
                        LogTimeField::Duration | LogTimeField::Activity => {
                            app.log_time_form.focused_field = match app.log_time_form.focused_field
                            {
                                LogTimeField::Duration => LogTimeField::Activity,
                                _ => LogTimeField::Comment,
                            };
                            app.log_time_form.error = None;
                        }
                        LogTimeField::Comment => {
                            // Validate duration before submitting.
                            let hours = parse_duration_to_hours(&app.log_time_form.duration_input);
                            match hours {
                                Err(e) => {
                                    app.log_time_form.error = Some(e);
                                    app.log_time_form.focused_field = LogTimeField::Duration;
                                }
                                Ok(h) => {
                                    if app.activities.is_empty() {
                                        app.log_time_form.error =
                                            Some("No activities loaded".into());
                                    } else if app.log_time_form.submitting {
                                        // Already in flight — ignore double-Enter.
                                    } else {
                                        // Resolve the selected activity, the Redmine ticket id,
                                        // and the parent MR id (needed to route the refresh).
                                        let activity = app
                                            .activities
                                            .get(app.log_time_form.selected_activity_idx)
                                            .cloned();
                                        let selected_mr = app
                                            .table_state
                                            .selected()
                                            .and_then(|i| app.visible_mrs().nth(i));
                                        let mr_id = selected_mr.map(|mr| mr.id.clone());
                                        let ticket_id = selected_mr
                                            .and_then(|mr| mr.linked_ticket.as_ref())
                                            .map(|t| t.id.clone());

                                        if let (Some(activity), Some(mr_id), Some(ticket_id)) =
                                            (activity, mr_id, ticket_id)
                                        {
                                            app.log_time_form.submitting = true;
                                            app.log_time_form.error = None;

                                            let provider = app.tracker.as_ref().map(Arc::clone);
                                            let comment = app.log_time_form.comment_input.clone();
                                            let spent_on =
                                                chrono::Utc::now().format("%Y-%m-%d").to_string();
                                            let tx2 = tx.clone();
                                            let tid = ticket_id.clone();

                                            if let Some(provider) = provider {
                                                tokio::spawn(async move {
                                                    let entry =
                                                        gitlab_tracker_core::TimeEntryRequest {
                                                            hours: h,
                                                            activity_id: activity.id,
                                                            comment,
                                                            spent_on,
                                                        };
                                                    match provider.log_time(&tid, entry).await {
                                                        Ok(()) => {
                                                            let _ = tx2.send(
                                                                crate::models::AppEvent::TimeLogSubmitted {
                                                                    mr_id,
                                                                    ticket_id: tid,
                                                                },
                                                            );
                                                        }
                                                        Err(e) => {
                                                            let _ = tx2.send(
                                                                crate::models::AppEvent::TimeLogFailed {
                                                                    error: e,
                                                                },
                                                            );
                                                        }
                                                    }
                                                });
                                            }
                                        } else {
                                            app.log_time_form.error =
                                                Some("No linked ticket selected".into());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Backspace edits the focused text field.
                KeyCode::Backspace => match app.log_time_form.focused_field {
                    LogTimeField::Duration => {
                        app.log_time_form.duration_input.pop();
                    }
                    LogTimeField::Comment => {
                        app.log_time_form.comment_input.pop();
                    }
                    LogTimeField::Activity => {}
                },

                // Printable chars feed the focused text field.
                KeyCode::Char(c) => match app.log_time_form.focused_field {
                    LogTimeField::Duration => {
                        app.log_time_form.duration_input.push(c);
                    }
                    LogTimeField::Comment => {
                        app.log_time_form.comment_input.push(c);
                    }
                    LogTimeField::Activity => {}
                },

                _ => {}
            }
        }

        // ------------------------------------------------------------------
        // Normal mode: shortcuts are active; input field is passive.
        // '/' or 'i' enters Editing mode (vim-style focus).
        // ------------------------------------------------------------------
        InputMode::Normal => match key.code {
            // Quit the application.
            KeyCode::Esc => return true,

            // Enter Editing mode — the input field now has exclusive focus.
            KeyCode::Char('/') | KeyCode::Char('i') => {
                app.input_mode = InputMode::Editing;
            }

            // Tab cycles focus between panes.
            KeyCode::Tab => {
                app.active_pane = app.active_pane.next(app.has_tracker_ticket());
            }

            // Arrow keys and j/k are routed based on the active pane.
            KeyCode::Down | KeyCode::Char('j') => match app.active_pane {
                ActivePane::Inspector => app.inspector_scroll_down(1),
                ActivePane::Tracker => app.tracker_scroll_down(1),
                ActivePane::Dashboard => {
                    app.next_row();
                    // When the TimeLog tracker view is active, re-fetch time entries
                    // for the newly selected MR's linked ticket.
                    if app.tracker_view == TrackerView::TimeLog {
                        if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                            let ticket_id = app
                                .table_state
                                .selected()
                                .and_then(|i| app.visible_mrs().nth(i))
                                .and_then(|mr| mr.linked_ticket.as_ref())
                                .map(|t| t.id.clone());

                            if let Some(tid) = ticket_id {
                                let tx2 = tx.clone();
                                tokio::spawn(async move {
                                    let entries = provider.fetch_time_entries(&tid).await;
                                    let _ = tx2.send(crate::models::AppEvent::TimeEntriesLoaded {
                                        entries,
                                    });
                                });
                            }
                        }
                    }
                }
            },
            KeyCode::Up | KeyCode::Char('k') => match app.active_pane {
                ActivePane::Inspector => app.inspector_scroll_up(1),
                ActivePane::Tracker => app.tracker_scroll_up(1),
                ActivePane::Dashboard => {
                    app.prev_row();
                    // When the TimeLog tracker view is active, re-fetch time entries
                    // for the newly selected MR's linked ticket.
                    if app.tracker_view == TrackerView::TimeLog {
                        if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                            let ticket_id = app
                                .table_state
                                .selected()
                                .and_then(|i| app.visible_mrs().nth(i))
                                .and_then(|mr| mr.linked_ticket.as_ref())
                                .map(|t| t.id.clone());

                            if let Some(tid) = ticket_id {
                                let tx2 = tx.clone();
                                tokio::spawn(async move {
                                    let entries = provider.fetch_time_entries(&tid).await;
                                    let _ = tx2.send(crate::models::AppEvent::TimeEntriesLoaded {
                                        entries,
                                    });
                                });
                            }
                        }
                    }
                }
            },

            // Open the MR URL in the default browser.
            KeyCode::Char('o') | KeyCode::Char('O') => {
                if let Some(selected) = app.table_state.selected() {
                    if let Some(mr) = app.mrs.get(selected) {
                        let target_url = if !mr.web_url.is_empty() {
                            mr.web_url.clone()
                        } else {
                            format!(
                                "{}/projects/{}/merge_requests/{}",
                                app.base_url, app.project_id, mr.id
                            )
                        };
                        let _ = open::that(target_url);
                    }
                }
            }

            // [Y]ank — copy the git clone command for the MR source branch to clipboard.
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(selected) = app.table_state.selected() {
                    if let Some(mr) = app.mrs.get(selected) {
                        let ssh_url = mr
                            .web_url
                            .split("/-/")
                            .next()
                            .unwrap_or("")
                            .replacen("https://", "git@", 1)
                            .replacen('/', ":", 1);
                        let cmd = format!("git clone -b {} {}.git", mr.source_branch, ssh_url);
                        if let Ok(mut ctx) = arboard::Clipboard::new() {
                            let _ = ctx.set_text(cmd);
                        }
                    }
                }
            }

            // Force a full refresh of all MRs (GitLab + Redmine tickets).
            KeyCode::Char('r') | KeyCode::Char('R') => {
                app.time_left = app.refresh_interval_secs;
                let ctx = build_fetch_context(app);

                for mr in &mut app.mrs {
                    mr.status = MrStatus::Loading;
                    let cached = cached_from_mr(mr);
                    spawn_mr_fetch(
                        ctx.clone(),
                        mr.id.clone(),
                        cached,
                        api_semaphore.clone(),
                        tx.clone(),
                    );

                    // Re-fetch the tracker ticket so that spent_hours and time entries
                    // reflect any change logged since the last refresh, regardless of
                    // whether the GitLab MR itself was updated.
                    if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                        if let Some(ticket_id) = mr.linked_ticket.as_ref().map(|t| t.id.clone()) {
                            let mr_id = mr.id.clone();
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                if let Some(ticket) = provider.fetch_ticket(&ticket_id).await {
                                    let _ =
                                        tx2.send(crate::models::AppEvent::TrackerTicketLoaded {
                                            mr_id,
                                            ticket: Box::new(ticket),
                                        });
                                }
                            });
                        }
                    }
                }
            }

            // [P] cycles the current pane's view:
            //   - Inspector pane: MrInfo ↔ Pipelines
            //   - Tracker pane:   TicketInfo ↔ TimeLog (fetches entries on enter)
            //   - Dashboard: no-op
            KeyCode::Char('p') | KeyCode::Char('P') => {
                match app.active_pane {
                    ActivePane::Tracker => {
                        app.tracker_view = app.tracker_view.next();
                        app.reset_tracker_scroll();

                        // When entering TimeLog, fetch time entries for the selected ticket.
                        if app.tracker_view == TrackerView::TimeLog {
                            let ticket_id = app
                                .table_state
                                .selected()
                                .and_then(|i| app.visible_mrs().nth(i))
                                .and_then(|mr| mr.linked_ticket.as_ref())
                                .map(|t| t.id.clone());

                            if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                                if let Some(tid) = ticket_id {
                                    let tx2 = tx.clone();
                                    tokio::spawn(async move {
                                        let entries = provider.fetch_time_entries(&tid).await;
                                        let _ =
                                            tx2.send(crate::models::AppEvent::TimeEntriesLoaded {
                                                entries,
                                            });
                                    });
                                }
                            }
                        }
                    }
                    _ => {
                        // Inspector pane (or Dashboard): cycle MrInfo ↔ Pipelines.
                        app.inspector_view = app.inspector_view.next();
                        app.reset_inspector_scroll();
                    }
                }
            }

            // [L] opens the Log Time popup for the selected MR's linked tracker ticket.
            // Only active when a tracker provider is configured and a ticket is linked.
            KeyCode::Char('l') | KeyCode::Char('L') => {
                let has_ticket = app
                    .table_state
                    .selected()
                    .and_then(|i| app.visible_mrs().nth(i))
                    .and_then(|mr| mr.linked_ticket.as_ref())
                    .is_some();

                if has_ticket {
                    app.log_time_form = LogTimeForm::default();
                    app.input_mode = InputMode::LogTime;

                    // Fetch activities lazily if not yet loaded.
                    if app.activities.is_empty() {
                        if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                let activities = provider.fetch_activities().await;
                                let _ =
                                    tx2.send(crate::models::AppEvent::ActivitiesLoaded(activities));
                            });
                        }
                    }
                }
            }

            KeyCode::Char('s') => app.cycle_sort_column(),
            KeyCode::Char('S') => app.toggle_sort_order(),

            // [F] opens the filter picker popup.
            KeyCode::Char('f') | KeyCode::Char('F') => {
                app.open_filter_picker();
            }

            // Space toggles the flagged state of the selected MR and persists immediately.
            KeyCode::Char(' ') => {
                if app.toggle_flag_selected().is_some() {
                    save_state_async(&app.mrs, &app.branches, last_known_branches).await;
                }
            }

            // [C] opens the column-picker popup.
            KeyCode::Char('c') | KeyCode::Char('C') => {
                app.column_picker_cursor = 0;
                app.input_mode = InputMode::ColumnPicker;
            }

            // [T] cycles focus to the Tracker pane (or opens the URL when already focused).
            // When focused on Tracker: [T] opens the ticket URL in the browser.
            // Otherwise: [T] moves focus to the Tracker pane (if a ticket is linked).
            KeyCode::Char('t') | KeyCode::Char('T') => {
                if app.active_pane == ActivePane::Tracker {
                    // Already on Tracker pane — open URL in browser.
                    if let Some(selected) = app.table_state.selected() {
                        if let Some(mr) = app.visible_mrs().nth(selected) {
                            if let Some(ticket) = &mr.linked_ticket {
                                let _ = open::that(&ticket.url);
                            }
                        }
                    }
                } else if app.has_tracker_ticket() {
                    // Move focus to the Tracker pane.
                    app.active_pane = ActivePane::Tracker;
                }
            }

            // Delete: remove the selected MR from the list.
            KeyCode::Delete => {
                if let Some(selected) = app.table_state.selected() {
                    if selected < app.mrs.len() {
                        app.mrs.remove(selected);
                        if app.mrs.is_empty() {
                            app.table_state.select(None);
                        } else if selected >= app.mrs.len() {
                            app.table_state.select(Some(app.mrs.len() - 1));
                        }
                        save_state_async(&app.mrs, &app.branches, last_known_branches).await;
                    }
                }
            }

            _ => {}
        },
    }

    false
}

/// Handles the Enter key: parses the input field and dispatches the appropriate action
/// (add/remove MR, add/remove branch).
async fn handle_enter(
    app: &mut App,
    api_semaphore: &Arc<Semaphore>,
    tx: &UnboundedSender<AppEvent>,
    last_known_branches: &mut HashMap<String, HashSet<String>>,
) {
    let value = app.input.trim().to_string();
    if value.is_empty() {
        return;
    }

    if value.starts_with('-') {
        // Remove an MR (numeric) or a branch (text).
        let to_remove = value.trim_start_matches('-').to_string();
        if to_remove.chars().all(|c| c.is_numeric()) {
            app.mrs.retain(|m| m.id != to_remove);
        } else {
            app.branches.retain(|b| b != &to_remove);
        }
        save_state_async(&app.mrs, &app.branches, last_known_branches).await;
        if app.mrs.is_empty() {
            app.table_state.select(None);
        }
    } else if value.chars().all(|c| c.is_numeric()) {
        // Add a new MR to track.
        if !app.mrs.iter().any(|m| m.id == value) {
            app.mrs.push(TrackedMr {
                id: value.clone(),
                title: "Loading...".to_string(),
                status: MrStatus::Loading,
                state: crate::models::GitlabMrState::Opened,
                // Mergeability is fetched live — start as Unknown until the first API response.
                mergeability: crate::models::MergeabilityStatus::Unknown,
                sha: None,
                description: String::new(),
                author: "Loading".to_string(),
                assignee: "Loading".to_string(),
                reviewers: vec![],
                milestone: "Loading".to_string(),
                milestone_due_date: None,
                web_url: String::new(),
                labels: vec![],
                updated_at: None,
                source_branch: "unknown".to_string(),
                target_branch: "unknown".to_string(),
                merged_by: None,
                merged_at: None,
                // Pipelines are fetched on demand when the user presses [P].
                pipelines: vec![],
                // New MRs are not highlighted on first load.
                recently_updated: false,
                // Notes count is unknown until the first API response.
                user_notes_count: 0,
                // New MRs start unflagged.
                flagged: false,
                // Ticket resolved live after the first MR fetch — not pre-populated.
                linked_ticket: None,
            });
            app.table_state.select(Some(app.mrs.len() - 1));
            save_state_async(&app.mrs, &app.branches, last_known_branches).await;

            let ctx = build_fetch_context(app);
            spawn_mr_fetch(
                ctx,
                value,
                CachedMrData::default(),
                api_semaphore.clone(),
                tx.clone(),
            );
        }
    } else {
        // Add a new branch to track.
        if !app.branches.contains(&value) {
            app.branches.push(value.clone());
            save_state_async(&app.mrs, &app.branches, last_known_branches).await;

            let ctx = build_fetch_context(app);
            for mr in &mut app.mrs {
                if mr.status != MrStatus::Loading {
                    mr.status = MrStatus::Loading;
                    let cached = cached_from_mr(mr);
                    spawn_mr_fetch(
                        ctx.clone(),
                        mr.id.clone(),
                        cached,
                        api_semaphore.clone(),
                        tx.clone(),
                    );
                }
            }
        }
    }

    app.input.clear();
}

/// Handles a keyboard event in demo mode (no network, no input field, no mutations).
///
/// Accepts both `Press` and `Repeat` kinds so held keys scroll smoothly.
/// Returns `true` if the main loop should exit (Esc was pressed).
pub fn handle_key_event_demo(key: KeyEvent, app: &mut App) -> bool {
    // Filter picker popup intercepts all keys when open.
    if app.input_mode == InputMode::FilterPicker {
        use crate::app::FILTER_PICKER_ENTRIES;
        const LAST_IDX: usize = FILTER_PICKER_ENTRIES.len() - 1;
        let needs_text_input = matches!(app.filter_picker.cursor, 13 | 14);
        match key.code {
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
                app.filter_picker = FilterPickerState::default();
            }
            KeyCode::Enter => {
                app.apply_filter_picker();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if app.filter_picker.cursor > 0 {
                    app.filter_picker.cursor -= 1;
                    app.filter_picker.input.clear();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.filter_picker.cursor < LAST_IDX {
                    app.filter_picker.cursor += 1;
                    app.filter_picker.input.clear();
                }
            }
            KeyCode::Backspace if needs_text_input => {
                app.filter_picker.input.pop();
            }
            KeyCode::Char(c) if needs_text_input => {
                app.filter_picker.input.push(c);
            }
            _ => {}
        }
        return false;
    }

    // Column-picker popup intercepts all keys when open.
    if app.input_mode == InputMode::ColumnPicker {
        const COLUMN_COUNT: usize = 5;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if app.column_picker_cursor > 0 {
                    app.column_picker_cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if app.column_picker_cursor < COLUMN_COUNT - 1 {
                    app.column_picker_cursor += 1;
                }
            }
            KeyCode::Char(' ') => match app.column_picker_cursor {
                0 => app.config.visible_columns.activity = !app.config.visible_columns.activity,
                1 => {
                    app.config.visible_columns.target_branch =
                        !app.config.visible_columns.target_branch
                }
                2 => app.config.visible_columns.labels = !app.config.visible_columns.labels,
                3 => app.config.visible_columns.milestone = !app.config.visible_columns.milestone,
                4 => app.config.visible_columns.notes = !app.config.visible_columns.notes,
                _ => {}
            },
            // Close the popup — no disk write in demo mode.
            // Enter is used in the demo tape instead of Esc because VHS may fire
            // the Escape sequence before the column picker frame is rendered, causing
            // it to be received in Normal mode and quitting the app instead.
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        }
        return false;
    }

    match key.code {
        KeyCode::Esc => return true,

        KeyCode::Tab => {
            app.active_pane = app.active_pane.next(app.has_tracker_ticket());
        }

        KeyCode::Down | KeyCode::Char('j') => match app.active_pane {
            ActivePane::Inspector => app.inspector_scroll_down(1),
            ActivePane::Tracker => app.tracker_scroll_down(1),
            ActivePane::Dashboard => app.next_row(),
        },
        KeyCode::Up | KeyCode::Char('k') => match app.active_pane {
            ActivePane::Inspector => app.inspector_scroll_up(1),
            ActivePane::Tracker => app.tracker_scroll_up(1),
            ActivePane::Dashboard => app.prev_row(),
        },

        // Open the MR URL in the default browser (useful even in demo mode).
        KeyCode::Char('o') | KeyCode::Char('O') => {
            if let Some(selected) = app.table_state.selected() {
                if let Some(mr) = app.mrs.get(selected) {
                    let target_url = if !mr.web_url.is_empty() {
                        mr.web_url.clone()
                    } else {
                        format!(
                            "{}/projects/{}/merge_requests/{}",
                            app.base_url, app.project_id, mr.id
                        )
                    };
                    let _ = open::that(target_url);
                }
            }
        }

        // [Y]ank — copy the git clone command for the MR source branch to clipboard.
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(selected) = app.table_state.selected() {
                if let Some(mr) = app.mrs.get(selected) {
                    let ssh_url = mr
                        .web_url
                        .split("/-/")
                        .next()
                        .unwrap_or("")
                        .replacen("https://", "git@", 1)
                        .replacen('/', ":", 1);
                    let cmd = format!("git clone -b {} {}.git", mr.source_branch, ssh_url);
                    if let Ok(mut ctx) = arboard::Clipboard::new() {
                        let _ = ctx.set_text(cmd);
                    }
                }
            }
        }

        // Reset the refresh timer display only (no actual network fetch in demo mode).
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.time_left = app.refresh_interval_secs;
        }

        KeyCode::Char('s') => app.cycle_sort_column(),
        KeyCode::Char('S') => app.toggle_sort_order(),

        // [F] opens the filter picker popup in demo mode.
        KeyCode::Char('f') | KeyCode::Char('F') => {
            app.open_filter_picker();
        }

        // Space toggles the flagged state of the selected MR in demo mode (no persistence).
        KeyCode::Char(' ') => {
            app.toggle_flag_selected();
        }

        // [P] cycles the Inspector view in demo mode (no network fetch).
        KeyCode::Char('p') | KeyCode::Char('P') => match app.active_pane {
            ActivePane::Tracker => {
                app.tracker_view = app.tracker_view.next();
                app.reset_tracker_scroll();
            }
            _ => {
                app.inspector_view = app.inspector_view.next();
                app.reset_inspector_scroll();
            }
        },

        _ => {}
    }

    false
}

/// Builds a `FetchContext` from the current application state.
fn build_fetch_context(app: &App) -> FetchContext {
    FetchContext {
        base_url: app.base_url.clone(),
        token: app.token.clone(),
        project_id: app.project_id.clone(),
        branches: app.branches.clone(),
    }
}

/// Builds a `CachedMrData` snapshot from a `TrackedMr` for use in fetch requests.
///
/// Includes `updated_at` and the current `pipelines` so the fetcher can skip
/// pipeline re-fetching when the MR has not changed since the last cycle.
fn cached_from_mr(mr: &TrackedMr) -> CachedMrData {
    CachedMrData {
        title: Some(mr.title.clone()),
        sha: mr.sha.clone(),
        description: Some(mr.description.clone()),
        author: Some(mr.author.clone()),
        assignee: Some(mr.assignee.clone()),
        web_url: Some(mr.web_url.clone()),
        labels: Some(mr.labels.clone()),
        updated_at: mr.updated_at.clone(),
        pipelines: mr.pipelines.clone(),
    }
}
