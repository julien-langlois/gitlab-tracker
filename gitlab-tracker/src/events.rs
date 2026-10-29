use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::actions::{dispatch, Action, ActionContext};
use crate::app::{ActivePane, App, FilterPickerState, InputMode, LogTimeField, LogTimeForm};
use crate::models::AppEvent;

/// Handles a mouse event and updates the application state accordingly.
/// Time entries for a newly selected row are fetched by `App::ensure_time_entries`.
pub fn handle_mouse_event(mouse: MouseEvent, app: &mut App) {
    // Hit-test against the pane areas recorded by the last render, so the result
    // always matches what is on screen (layout, input bar height, cockpit…).
    let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
    let in_tracker_pane = app
        .layout
        .tracker_area
        .is_some_and(|area| area.contains(pos));
    let in_inspector_pane = app.layout.inspector_area.contains(pos);

    match mouse.kind {
        // Update focus based on where the cursor is.
        MouseEventKind::Moved | MouseEventKind::Drag(_) => {
            app.active_pane = if in_tracker_pane {
                ActivePane::Tracker
            } else if in_inspector_pane {
                ActivePane::Inspector
            } else {
                ActivePane::Dashboard
            };
        }
        // Route scroll to the pane under the cursor.
        MouseEventKind::ScrollDown => {
            if in_tracker_pane {
                app.layout.tracker.down(3);
            } else if in_inspector_pane {
                app.layout.inspector.down(3);
            } else {
                app.next_row();
            }
        }
        MouseEventKind::ScrollUp => {
            if in_tracker_pane {
                app.layout.tracker.up(3);
            } else if in_inspector_pane {
                app.layout.inspector.up(3);
            } else {
                app.prev_row();
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
    let mut ctx = ActionContext {
        api_semaphore,
        tx,
        last_known_branches,
        state_writer: None,
    };
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
                        dispatch(Action::TrackMilestone(title), app, &mut ctx).await;
                        app.input.clear();
                        app.input_mode = InputMode::Normal;
                    }
                } else {
                    let value = std::mem::take(&mut app.input);
                    dispatch(Action::SubmitInput(value), app, &mut ctx).await;
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
            let visible_filters = app.visible_filter_defs();
            let last_idx = visible_filters.len().saturating_sub(1);
            let needs_text_input = visible_filters
                .get(app.filter_picker.cursor)
                .map(|(_, def)| def.needs_text_input)
                .unwrap_or(false);

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
                        app.filter_picker.suggestion = None;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') if !needs_text_input => {
                    if app.filter_picker.cursor < last_idx {
                        app.filter_picker.cursor += 1;
                        app.filter_picker.input.clear();
                        app.filter_picker.suggestion = None;
                    }
                }
                // Allow navigating away from a text-input row with arrow keys
                // even while the input is focused (Esc is the cancel path).
                KeyCode::Up => {
                    if app.filter_picker.cursor > 0 {
                        app.filter_picker.cursor -= 1;
                        app.filter_picker.input.clear();
                        app.filter_picker.suggestion = None;
                    }
                }
                KeyCode::Down => {
                    if app.filter_picker.cursor < last_idx {
                        app.filter_picker.cursor += 1;
                        app.filter_picker.input.clear();
                        app.filter_picker.suggestion = None;
                    }
                }

                // Label autocompletion (a no-op on other rows).
                KeyCode::Tab => app.cycle_label_suggestion(true),
                KeyCode::BackTab => app.cycle_label_suggestion(false),

                KeyCode::Backspace if needs_text_input => {
                    app.filter_picker.input.pop();
                    app.filter_picker.suggestion = None;
                }

                KeyCode::Char(c) if needs_text_input => {
                    app.filter_picker.input.push(c);
                    app.filter_picker.suggestion = None;
                }

                _ => {}
            }
        }

        // ------------------------------------------------------------------
        // Column-picker mode: the popup is open.
        // Up/Down move the cursor; Space toggles; Esc closes and persists.
        // ------------------------------------------------------------------
        InputMode::ColumnPicker => {
            let visible_cols = app.visible_column_defs();
            let column_count = visible_cols.len();

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.column_picker_cursor > 0 {
                        app.column_picker_cursor -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if column_count > 0 && app.column_picker_cursor < column_count - 1 {
                        app.column_picker_cursor += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    // Toggle the column at the cursor position using its stable `id`.
                    if let Some(col) = visible_cols.get(app.column_picker_cursor) {
                        app.config.visible_columns.toggle(col.id);
                    }
                }
                KeyCode::Esc | KeyCode::Enter => {
                    // Close the popup and persist the new column visibility to projects.toml.
                    app.input_mode = InputMode::Normal;
                    dispatch(Action::SaveVisibleColumns, app, &mut ctx).await;
                }
                _ => {}
            }
        }

        // ------------------------------------------------------------------
        // Project picker: Up/Down move; Enter switches project; Esc closes.
        // ------------------------------------------------------------------
        InputMode::ProjectPicker => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                app.project_picker_cursor = app.project_picker_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let last = app.project_picker.len().saturating_sub(1);
                app.project_picker_cursor = (app.project_picker_cursor + 1).min(last);
            }
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
                if let Some(entry) = app.project_picker.get(app.project_picker_cursor).cloned() {
                    dispatch(Action::SwitchProject(Box::new(entry)), app, &mut ctx).await;
                }
            }
            KeyCode::Esc => app.input_mode = InputMode::Normal,
            _ => {}
        },

        // ------------------------------------------------------------------
        // Settings mode: the project settings popup is open.
        // Up/Down navigate; Space toggles booleans; arrows adjust numbers;
        // printable chars edit text settings; Enter saves; Esc cancels.
        // ------------------------------------------------------------------
        InputMode::Settings => match key.code {
            KeyCode::Esc => {
                app.settings_editor.cancel();
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                dispatch(Action::SaveSettings, app, &mut ctx).await;
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => app.settings_editor.move_up(),
            KeyCode::Down | KeyCode::Char('j') => app.settings_editor.move_down(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') => {
                app.settings_editor.decrement_selected()
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('+') => {
                app.settings_editor.increment_selected()
            }
            KeyCode::Char(' ') => app.settings_editor.toggle_selected(),
            KeyCode::Backspace => app.settings_editor.backspace(),
            KeyCode::Char(c) => app.settings_editor.push_char(c),
            _ => {}
        },

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
                KeyCode::Enter => match app.log_time_form.focused_field {
                    LogTimeField::Duration | LogTimeField::Activity => {
                        app.log_time_form.focused_field = match app.log_time_form.focused_field {
                            LogTimeField::Duration => LogTimeField::Activity,
                            _ => LogTimeField::Comment,
                        };
                        app.log_time_form.error = None;
                    }
                    LogTimeField::Comment => {
                        dispatch(Action::SubmitLogTime, app, &mut ctx).await;
                    }
                },

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
        // Help popup is open — any key closes it.
        InputMode::Help => {
            app.input_mode = InputMode::Normal;
        }

        // ── Stats overlay ─────────────────────────────────────────────────────
        #[cfg(feature = "stats")]
        InputMode::Stats => {
            match key.code {
                // Close the overlay.
                KeyCode::Esc | KeyCode::Char('g') | KeyCode::Char('G') => {
                    app.input_mode = InputMode::Normal;
                }
                // Scroll up/down inside the overlay.
                KeyCode::Down | KeyCode::Char('j') => {
                    app.stats_view.scroll = app.stats_view.scroll.saturating_add(1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.stats_view.scroll = app.stats_view.scroll.saturating_sub(1);
                }
                KeyCode::PageDown => {
                    app.stats_view.scroll = app.stats_view.scroll.saturating_add(10);
                }
                KeyCode::PageUp => {
                    app.stats_view.scroll = app.stats_view.scroll.saturating_sub(10);
                }
                KeyCode::Tab => {
                    app.stats_view.tab = app.stats_view.tab.next();
                    app.stats_view.scroll = 0;
                }
                KeyCode::BackTab => {
                    app.stats_view.tab = app.stats_view.tab.previous();
                    app.stats_view.scroll = 0;
                }
                // [W] cycles the time window and triggers a fresh aggregation.
                KeyCode::Char('w') | KeyCode::Char('W') => {
                    dispatch(Action::CycleStatsWindow, app, &mut ctx).await;
                }
                // [R] forces a manual stats refresh without changing the window.
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    dispatch(Action::RefreshStats, app, &mut ctx).await;
                }
                KeyCode::Char(c) => {
                    if let Some(tab) = crate::app::StatsTab::from_digit(c) {
                        app.stats_view.tab = tab;
                        app.stats_view.scroll = 0;
                    }
                }
                _ => {}
            }
        }

        InputMode::Normal => {
            // When quit confirmation is pending, only Esc/y confirm and any other key cancels.
            if app.quit_confirm {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('y') => return true,
                    _ => {
                        app.quit_confirm = false;
                        return false;
                    }
                }
            }
            match key.code {
                // First Esc: request confirmation. A second Esc or 'y' will confirm.
                KeyCode::Esc => {
                    app.quit_confirm = true;
                }

                // Open the help popup listing all registered shortcuts by section.
                KeyCode::Char('?') => {
                    app.input_mode = InputMode::Help;
                }

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
                    ActivePane::Inspector => app.layout.inspector.down(1),
                    ActivePane::Tracker => app.layout.tracker.down(1),
                    ActivePane::Dashboard => {
                        app.next_row();
                    }
                },
                KeyCode::Up | KeyCode::Char('k') => match app.active_pane {
                    ActivePane::Inspector => app.layout.inspector.up(1),
                    ActivePane::Tracker => app.layout.tracker.up(1),
                    ActivePane::Dashboard => {
                        app.prev_row();
                    }
                },

                // Keys that map to an action; the rest are handled below.
                code => {
                    if let Some(action) = normal_mode_action(code, app) {
                        dispatch(action, app, &mut ctx).await;
                    } else {
                        match code {
                            // [P] cycles the current pane's view:
                            //   - Inspector pane: MrInfo ↔ Pipelines
                            //   - Tracker pane:   TicketInfo ↔ TimeLog (fetches entries on enter)
                            //   - Dashboard: no-op
                            KeyCode::Char('p') | KeyCode::Char('P') => match app.active_pane {
                                ActivePane::Tracker => {
                                    app.tracker_view = app.tracker_view.next();
                                    app.layout.tracker.reset();
                                }
                                _ => {
                                    // Inspector pane (or Dashboard): cycle MrInfo ↔ Pipelines.
                                    app.inspector_view = app.inspector_view.next();
                                    app.layout.inspector.reset();
                                }
                            },

                            // [F] opens the filter picker popup.
                            KeyCode::Char('f') | KeyCode::Char('F') => app.open_filter_picker(),

                            // [C] opens the column-picker popup.
                            KeyCode::Char('c') | KeyCode::Char('C') => {
                                app.column_picker_cursor = 0;
                                app.input_mode = InputMode::ColumnPicker;
                            }

                            // [T] moves focus to the Tracker pane (if a ticket is linked),
                            // or opens the ticket URL when the pane already has focus.
                            KeyCode::Char('t') | KeyCode::Char('T') => {
                                if app.active_pane == ActivePane::Tracker {
                                    dispatch(Action::OpenSelectedTicket, app, &mut ctx).await;
                                } else if app.has_tracker_ticket() {
                                    app.active_pane = ActivePane::Tracker;
                                }
                            }

                            _ => {}
                        }
                    }
                }
            }
        }
    }

    false
}

/// The action bound to `code` in Normal mode, if any.
fn normal_mode_action(code: KeyCode, app: &App) -> Option<Action> {
    let action = match code {
        KeyCode::Char('o') | KeyCode::Char('O') => Action::OpenSelectedMr,
        // [Y]ank — copy the git clone command for the MR source branch to clipboard.
        KeyCode::Char('y') | KeyCode::Char('Y') => Action::YankCloneCommand,
        // [E]xport — copy the visible MRs as a Markdown release checklist.
        KeyCode::Char('e') | KeyCode::Char('E') => Action::CopyChecklist,
        // Force a full refresh of all MRs (GitLab + tracker tickets).
        KeyCode::Char('r') | KeyCode::Char('R') => Action::Refresh,
        // [L] opens the Log Time popup for the selected MR's linked tracker ticket.
        KeyCode::Char('l') | KeyCode::Char('L') => Action::OpenLogTime,
        // [G] opens the Stats fullscreen overlay (only when the `stats` feature is on).
        #[cfg(feature = "stats")]
        KeyCode::Char('g') | KeyCode::Char('G') => Action::OpenStats,
        KeyCode::Char('s') => Action::CycleSortColumn,
        KeyCode::Char('S') => Action::ToggleSortOrder,
        // Space toggles the flagged state of the selected MR and persists immediately.
        KeyCode::Char(' ') => Action::ToggleFlagSelected,
        // [M] hides / shows merged MRs (they stay tracked); saved per project.
        KeyCode::Char('m') | KeyCode::Char('M') => Action::ToggleHideMerged,
        // [W] opens the project picker (switching needs the network: not in demo).
        KeyCode::Char('w') | KeyCode::Char('W') if !app.read_only => Action::OpenProjectPicker,
        // [,] opens the project settings popup.
        KeyCode::Char(',') => Action::OpenSettings,
        // Delete: remove the selected MR from the list.
        KeyCode::Delete => Action::RemoveSelected,
        _ => return None,
    };
    Some(action)
}
