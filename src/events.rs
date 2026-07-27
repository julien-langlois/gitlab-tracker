use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::app::{ActivePane, App};
use crate::gitlab::{spawn_mr_fetch, CachedMrData, FetchContext};
use crate::models::{AppEvent, MrStatus, TrackedMr};
use crate::storage::save_state_async;

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
    match key.code {
        KeyCode::Esc => return true,

        // Tab cycles focus between panes without consuming arrow keys.
        KeyCode::Tab if app.input.is_empty() => {
            app.active_pane = app.active_pane.next();
        }

        // Arrow keys and j/k are routed based on the active pane.
        KeyCode::Down | KeyCode::Char('j') if app.input.is_empty() => match app.active_pane {
            ActivePane::Inspector => app.inspector_scroll_down(1),
            ActivePane::Dashboard => app.next_row(),
        },
        KeyCode::Up | KeyCode::Char('k') if app.input.is_empty() => match app.active_pane {
            ActivePane::Inspector => app.inspector_scroll_up(1),
            ActivePane::Dashboard => app.prev_row(),
        },

        // Open the MR URL in the default browser.
        KeyCode::Char('o') | KeyCode::Char('O') if app.input.is_empty() => {
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
        KeyCode::Char('r') | KeyCode::Char('R') if app.input.is_empty() => {
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

        KeyCode::Char('s') if app.input.is_empty() => {
            app.cycle_sort_column();
        }

        KeyCode::Char('S') if app.input.is_empty() => {
            app.toggle_sort_order();
        }

        // Delete or 'x': remove the selected MR from the list.
        KeyCode::Delete | KeyCode::Char('x')
            if key.code == KeyCode::Delete || app.input.is_empty() =>
        {
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

        KeyCode::Char(c) => app.input.push(c),

        KeyCode::Backspace => {
            app.input.pop();
        }

        KeyCode::Enter => {
            handle_enter(app, api_semaphore, tx, last_known_branches).await;
        }

        _ => {}
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
                sha: None,
                description: String::new(),
                author: "Loading".to_string(),
                assignee: "Loading".to_string(),
                milestone: "Loading".to_string(),
                web_url: String::new(),
                labels: vec![],
                updated_at: None,
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
    }
}
