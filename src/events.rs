use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::app::{ActivePane, App, InputMode, InspectorView};
use crate::gitlab::{spawn_mr_fetch, CachedMrData, FetchContext};
use crate::models::{AppEvent, MrStatus, TrackedMr};
use crate::storage::{save_config_async, save_state_async};

/// Handles a mouse event and updates the application state accordingly.
///
/// Returns nothing — all mutations are applied directly on `app`.
pub fn handle_mouse_event(mouse: MouseEvent, term_width: u16, app: &mut App) {
    // The inspector occupies the right 35% of the terminal width.
    let inspector_start_col = term_width * 65 / 100;

    match mouse.kind {
        // Update focus based on where the cursor is.
        MouseEventKind::Moved | MouseEventKind::Drag(_) => {
            if mouse.column >= inspector_start_col {
                app.active_pane = ActivePane::Inspector;
            } else {
                app.active_pane = ActivePane::Dashboard;
            }
        }
        // Route scroll to the pane under the cursor.
        MouseEventKind::ScrollDown => {
            if mouse.column >= inspector_start_col {
                app.inspector_scroll_down(3);
            } else {
                app.next_row();
            }
        }
        MouseEventKind::ScrollUp => {
            if mouse.column >= inspector_start_col {
                app.inspector_scroll_up(3);
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
    match app.input_mode {
        // ------------------------------------------------------------------
        // Editing mode: the input field has exclusive focus.
        // Only Esc, Enter, Backspace and printable chars are handled here.
        // ------------------------------------------------------------------
        InputMode::Editing => match key.code {
            // Esc leaves editing mode (clears input so it feels like a cancel).
            KeyCode::Esc => {
                app.input.clear();
                app.input_mode = InputMode::Normal;
            }

            KeyCode::Enter => {
                handle_enter(app, api_semaphore, tx, last_known_branches).await;
                // Return to Normal after submitting so shortcuts are available again.
                app.input_mode = InputMode::Normal;
            }

            KeyCode::Backspace => {
                app.input.pop();
            }

            KeyCode::Char(c) => app.input.push(c),

            _ => {}
        },

        // ------------------------------------------------------------------
        // Column-picker mode: the popup is open.
        // Up/Down move the cursor; Space toggles; Esc closes and persists.
        // ------------------------------------------------------------------
        InputMode::ColumnPicker => {
            const COLUMN_COUNT: usize = 4;
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
                KeyCode::Char(' ') => {
                    // Toggle the column at the current cursor position.
                    // Order must mirror `VisibleColumns` field declaration order.
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
                app.active_pane = app.active_pane.next();
            }

            // Arrow keys and j/k are routed based on the active pane.
            KeyCode::Down | KeyCode::Char('j') => match app.active_pane {
                ActivePane::Inspector => app.inspector_scroll_down(1),
                ActivePane::Dashboard => app.next_row(),
            },
            KeyCode::Up | KeyCode::Char('k') => match app.active_pane {
                ActivePane::Inspector => app.inspector_scroll_up(1),
                ActivePane::Dashboard => app.prev_row(),
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

            // Force a full refresh of all MRs.
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
                }
            }

            // [P] toggles the Inspector between MR info and Pipeline view.
            KeyCode::Char('p') | KeyCode::Char('P') => {
                app.inspector_view = match app.inspector_view {
                    InspectorView::MrInfo => InspectorView::Pipelines,
                    InspectorView::Pipelines => InspectorView::MrInfo,
                };
                app.reset_inspector_scroll();
            }

            KeyCode::Char('s') => app.cycle_sort_column(),
            KeyCode::Char('S') => app.toggle_sort_order(),

            // [C] opens the column-picker popup.
            KeyCode::Char('c') | KeyCode::Char('C') => {
                app.column_picker_cursor = 0;
                app.input_mode = InputMode::ColumnPicker;
            }

            // Delete or 'x': remove the selected MR from the list.
            KeyCode::Delete | KeyCode::Char('x') => {
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
                milestone: "Loading".to_string(),
                web_url: String::new(),
                labels: vec![],
                updated_at: None,
                // target_branch is unknown until the first API response.
                target_branch: "unknown".to_string(),
                // Pipelines are fetched on demand when the user presses [P].
                pipelines: vec![],
                // New MRs are not highlighted on first load.
                recently_updated: false,
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
    // Column-picker popup intercepts all keys when open.
    if app.input_mode == InputMode::ColumnPicker {
        const COLUMN_COUNT: usize = 4;
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
            app.active_pane = app.active_pane.next();
        }

        KeyCode::Down | KeyCode::Char('j') => match app.active_pane {
            ActivePane::Inspector => app.inspector_scroll_down(1),
            ActivePane::Dashboard => app.next_row(),
        },
        KeyCode::Up | KeyCode::Char('k') => match app.active_pane {
            ActivePane::Inspector => app.inspector_scroll_up(1),
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

        // Reset the refresh timer display only (no actual network fetch in demo mode).
        KeyCode::Char('r') | KeyCode::Char('R') => {
            app.time_left = app.refresh_interval_secs;
        }

        KeyCode::Char('s') => app.cycle_sort_column(),
        KeyCode::Char('S') => app.toggle_sort_order(),

        // [P] toggles the Inspector between MR info and Pipeline view (no fetch in demo mode).
        KeyCode::Char('p') | KeyCode::Char('P') => {
            app.inspector_view = match app.inspector_view {
                InspectorView::MrInfo => InspectorView::Pipelines,
                InspectorView::Pipelines => InspectorView::MrInfo,
            };
            app.reset_inspector_scroll();
        }

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
        milestone: Some(mr.milestone.clone()),
        web_url: Some(mr.web_url.clone()),
        labels: Some(mr.labels.clone()),
        updated_at: mr.updated_at.clone(),
        pipelines: mr.pipelines.clone(),
    }
}
