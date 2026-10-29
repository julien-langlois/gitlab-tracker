//! What the user can ask the app to do, independent of how they asked.
//!
//! The TUI maps keys to an [`Action`] in `events.rs`; another frontend maps its own
//! input (clicks, menus) to the same actions. [`dispatch`] holds the behaviour once,
//! so both frontends stay in step. Moving a cursor inside a popup or editing a text
//! field is input handling, not an action: each frontend keeps that to itself.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{mpsc::UnboundedSender, Semaphore};

use crate::app::{App, InputMode, LogTimeField, LogTimeForm};
use crate::gitlab::{spawn_milestone_mrs_fetch, spawn_mr_fetch, CachePolicy, CachedMrData};
use crate::models::{AppEvent, MrStatus};
use crate::storage::{
    save_branches_async, save_hide_merged_async, save_project_settings_async,
    save_visible_columns_async, ProjectEntry, StateFile, StateWriter,
};
use crate::utils::parse_duration_to_hours;

/// A request from the user, as understood by the app.
#[derive(Debug, Clone)]
pub enum Action {
    /// Parses the text typed in the input field: `123` tracks an MR, `-123` stops
    /// tracking it, `name` tracks a branch and `-name` stops tracking it.
    SubmitInput(String),
    /// Tracks every MR of a milestone (by title).
    TrackMilestone(String),
    /// Refetches every tracked MR and its tracker ticket, bypassing the caches.
    Refresh,
    /// Opens the selected MR in the default browser.
    OpenSelectedMr,
    /// Opens the selected MR's tracker ticket in the default browser.
    OpenSelectedTicket,
    /// Copies the git clone command for the selected MR's source branch.
    YankCloneCommand,
    /// Copies the visible MRs as a Markdown release checklist.
    CopyChecklist,
    /// Flags or unflags the selected MR, and saves the state.
    ToggleFlagSelected,
    /// Hides or shows merged MRs, and saves the choice for the project.
    ToggleHideMerged,
    /// Shows only the MRs of the filter at this index in `App::filter_defs`
    /// (`query`: the text of the filters that take one).
    // The TUI applies its filter picker through `App::apply_filter_picker`.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    SetFilter {
        index: usize,
        query: String,
    },
    CycleSortColumn,
    /// Sorts by this column, or reverses the order when it is the current one.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    SortBy(crate::app::SortColumn),
    ToggleSortOrder,
    /// Stops tracking the selected MR.
    RemoveSelected,
    /// Opens the Log Time form for the selected MR's ticket, loading the
    /// activities on first use. Does nothing when the MR has no linked ticket.
    OpenLogTime,
    /// Validates the Log Time form and sends the entry to the tracker.
    SubmitLogTime,
    /// Saves the visible columns to `projects.toml`.
    SaveVisibleColumns,
    /// Opens the project settings form.
    OpenSettings,
    /// Saves the project settings form to `projects.toml` and applies it.
    SaveSettings,
    /// Loads `projects.toml` and opens the project picker on the current project.
    OpenProjectPicker,
    /// Switches to another project (ignored when it is the current one).
    SwitchProject(Box<ProjectEntry>),
    /// Opens the stats view and computes a fresh report.
    #[cfg(feature = "stats")]
    OpenStats,
    /// Recomputes the stats report.
    #[cfg(feature = "stats")]
    RefreshStats,
    /// Moves to the next stats time window and recomputes the report.
    #[cfg(feature = "stats")]
    CycleStatsWindow,
}

/// What [`dispatch`] needs beyond the app state: the API semaphore, the event
/// bus and the branches known at the last save.
pub struct ActionContext<'a> {
    pub api_semaphore: &'a Arc<Semaphore>,
    pub tx: &'a UnboundedSender<AppEvent>,
    pub last_known_branches: &'a mut HashMap<String, HashSet<String>>,
    /// Saves the state in the background (the window, which must not wait on the
    /// disk); `None` writes it before returning (the TUI).
    pub state_writer: Option<&'a StateWriter>,
}

/// Performs `action` on `app`.
pub async fn dispatch(action: Action, app: &mut App, ctx: &mut ActionContext<'_>) {
    let tx = ctx.tx;
    match action {
        Action::SubmitInput(value) => submit_input(app, &value, ctx.api_semaphore, tx).await,

        Action::TrackMilestone(title) => {
            // Filter by milestone title — the GitLab MRs API uses the title,
            // not the numeric milestone ID, for the `milestone` query parameter.
            if !app.read_only {
                spawn_milestone_mrs_fetch(app.fetch_context(), title, tx.clone());
            }
        }

        Action::Refresh => refresh(app, ctx.api_semaphore, tx),

        Action::OpenSelectedMr => {
            if let Some(mr) = app.selected_mr() {
                let target_url = if !mr.web_url.is_empty() {
                    mr.web_url.clone()
                } else {
                    format!(
                        "{}/projects/{}/merge_requests/{}",
                        app.base_url, app.project_id, mr.id
                    )
                };
                let _ = open::that_detached(target_url);
            }
        }

        Action::OpenSelectedTicket => {
            if let Some(ticket) = app.selected_mr().and_then(|mr| mr.linked_ticket.as_ref()) {
                let _ = open::that_detached(&ticket.url);
            }
        }

        Action::YankCloneCommand => app.yank_clone_command(),
        Action::CopyChecklist => app.yank_markdown_checklist(),

        Action::ToggleFlagSelected => {
            if app.toggle_flag_selected().is_some() && !app.read_only {
                let file = StateFile::new(
                    &app.mrs,
                    ctx.last_known_branches,
                    app.discovery_started_at.as_deref(),
                    &app.dismissed_mr_ids,
                    &app.base_url,
                    &app.project_id,
                );
                match (file, ctx.state_writer) {
                    (Some(file), Some(writer)) => writer.save(file),
                    (Some(file), None) => file.write().await,
                    (None, _) => {}
                }
            }
        }

        Action::ToggleHideMerged => {
            let hide = !app.config.hide_merged;
            app.config.hide_merged = hide;
            app.project_settings.hide_merged = Some(hide);
            app.clamp_selection();
            if !app.read_only {
                save_hide_merged_async(hide, &app.base_url, &app.project_id).await;
            }
        }

        Action::SetFilter { index, query } => app.set_filter(index, query),
        Action::CycleSortColumn => app.cycle_sort_column(),
        Action::SortBy(column) => app.sort_by(column),
        Action::ToggleSortOrder => app.toggle_sort_order(),

        Action::RemoveSelected => {
            // The selection indexes the visible (filtered) list: resolve the id.
            if let Some(id) = app.selected_mr().map(|mr| mr.id.clone()) {
                let _ = tx.send(AppEvent::MrRemovedById(id));
            }
        }

        Action::OpenLogTime => {
            let has_ticket = app
                .selected_mr()
                .and_then(|mr| mr.linked_ticket.as_ref())
                .is_some();
            if has_ticket {
                app.log_time_form = LogTimeForm::default();
                app.input_mode = InputMode::LogTime;

                // Fetch activities lazily if not yet loaded.
                if app.activities.is_empty() {
                    if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
                        app.activities_error = None;
                        let tx2 = tx.clone();
                        tokio::spawn(async move {
                            let activities = provider.fetch_activities().await;
                            let _ = tx2.send(AppEvent::ActivitiesLoaded(activities));
                        });
                    }
                }
            }
        }

        Action::SubmitLogTime => submit_log_time(app, tx),

        Action::SaveVisibleColumns => {
            if !app.read_only {
                save_visible_columns_async(
                    &app.config.visible_columns,
                    &app.base_url,
                    &app.project_id,
                )
                .await;
            }
        }

        Action::OpenSettings => {
            app.settings_editor =
                crate::settings::SettingsEditorState::from_project_entry(&app.project_settings);
            app.settings_editor.reset_cursor();
            app.input_mode = InputMode::Settings;
        }

        Action::SaveSettings => {
            let project_table = app.settings_editor.to_project_table();
            if app.read_only {
                app.settings_editor.cancel();
            } else if let Some(project) =
                save_project_settings_async(&project_table, &app.base_url, &app.project_id).await
            {
                app.project_settings = project;
                let editor = std::mem::take(&mut app.settings_editor);
                editor.apply_to_app(app);
                app.settings_editor =
                    crate::settings::SettingsEditorState::from_project_entry(&app.project_settings);
            }
        }

        Action::OpenProjectPicker => {
            // Switching needs the network: not in demo.
            if !app.read_only {
                app.project_picker = crate::storage::load_projects_toml().await.projects;
                app.project_picker_cursor = app
                    .project_picker
                    .iter()
                    .position(|p| {
                        crate::storage::is_same_project(p, &app.base_url, &app.project_id)
                    })
                    .unwrap_or(0);
                app.input_mode = InputMode::ProjectPicker;
            }
        }

        Action::SwitchProject(entry) => {
            if !crate::storage::is_same_project(&entry, &app.base_url, &app.project_id) {
                app.switch_to = Some(*entry);
            }
        }

        #[cfg(feature = "stats")]
        Action::OpenStats => {
            app.input_mode = InputMode::Stats;
            crate::ui::stats::trigger_stats_refresh(app, tx);
        }
        #[cfg(feature = "stats")]
        Action::RefreshStats => crate::ui::stats::trigger_stats_refresh(app, tx),
        #[cfg(feature = "stats")]
        Action::CycleStatsWindow => {
            app.stats_view.window = app.stats_view.window.next();
            crate::ui::stats::trigger_stats_refresh(app, tx);
        }
    }
}

fn refresh(app: &mut App, api_semaphore: &Arc<Semaphore>, tx: &UnboundedSender<AppEvent>) {
    app.time_left = app.refresh_interval_secs;
    if app.read_only {
        return; // demo: only the countdown restarts
    }
    if app.refresh_in_flight() {
        app.flash("Refresh already running — wait for it to finish", false);
        return;
    }
    app.time_entries.clear();
    let ctx = app.fetch_context();

    // Run the discovery poller on manual refresh as well, so a manual refresh
    // behaves like the automatic refresh cycle when auto-polling is enabled.
    if app.discovery_enabled {
        app.spawn_discovery(ctx.clone(), tx);
    }

    for mr in &mut app.mrs {
        mr.status = MrStatus::Loading;
        let mut cached = CachedMrData::from(&*mr);
        // Force a full re-sync on manual refresh: bypass all cache guards
        // so the user always gets an up-to-date snapshot, even for
        // already-merged MRs whose caches would otherwise be permanent.
        cached.cache_policy = CachePolicy::ForceAll;
        spawn_mr_fetch(
            ctx.clone(),
            mr.id.clone(),
            cached,
            api_semaphore.clone(),
            tx.clone(),
        );
        // Track pending fetches to drive the spinner in the table title.
        app.pending_refresh_fetches.insert(mr.id.clone());

        // Re-fetch the tracker ticket so that spent_hours and time entries
        // reflect any change logged since the last refresh, regardless of
        // whether the GitLab MR itself was updated.
        if let Some(provider) = app.tracker.as_ref().map(Arc::clone) {
            if let Some(ticket_id) = mr.linked_ticket.as_ref().map(|t| t.id.clone()) {
                crate::app::spawn_ticket_fetch(provider, ticket_id, mr.id.clone(), tx);
            }
        }
    }
    app.start_refresh_cycle("manual");
}

fn submit_log_time(app: &mut App, tx: &UnboundedSender<AppEvent>) {
    // Validate duration before submitting.
    let hours = match parse_duration_to_hours(&app.log_time_form.duration_input) {
        Err(e) => {
            app.log_time_form.error = Some(e.to_string());
            app.log_time_form.focused_field = LogTimeField::Duration;
            return;
        }
        Ok(h) => h,
    };
    if app.activities.is_empty() {
        app.log_time_form.error = Some("No activities loaded".into());
        return;
    }
    if app.log_time_form.submitting {
        return; // Already in flight — ignore a double submit.
    }

    // Resolve the selected activity, the tracker ticket id, and the parent MR id
    // (needed to route the refresh).
    let activity = app
        .activities
        .get(app.log_time_form.selected_activity_idx)
        .cloned();
    let selected_mr = app.selected_mr();
    let mr_id = selected_mr.map(|mr| mr.id.clone());
    let ticket_id = selected_mr
        .and_then(|mr| mr.linked_ticket.as_ref())
        .map(|t| t.id.clone());

    let (Some(activity), Some(mr_id), Some(ticket_id)) = (activity, mr_id, ticket_id) else {
        app.log_time_form.error = Some("No linked ticket selected".into());
        return;
    };

    app.log_time_form.submitting = true;
    app.log_time_form.error = None;

    let Some(provider) = app.tracker.as_ref().map(Arc::clone) else {
        return;
    };
    let comment = app.log_time_form.comment_input.clone();
    let spent_on = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let tx = tx.clone();
    tokio::spawn(async move {
        let entry = gitlab_tracker_core::TimeEntryRequest {
            hours,
            activity_id: activity.id,
            comment,
            spent_on,
        };
        match provider.log_time(&ticket_id, entry).await {
            Ok(()) => {
                let _ = tx.send(AppEvent::TimeLogSubmitted { mr_id, ticket_id });
            }
            Err(e) => {
                let _ = tx.send(AppEvent::TimeLogFailed { error: e });
            }
        }
    });
}

/// Parses the input field and dispatches the matching change
/// (add/remove MR, add/remove branch).
async fn submit_input(
    app: &mut App,
    value: &str,
    api_semaphore: &Arc<Semaphore>,
    tx: &UnboundedSender<AppEvent>,
) {
    let value = value.trim().to_string();
    if value.is_empty() || app.read_only {
        // Demo: adding / removing MRs and branches is a mutation — ignore it.
        return;
    }

    if value.starts_with('-') {
        // Remove an MR (numeric) or a branch (text).
        let to_remove = value.trim_start_matches('-').to_string();
        if to_remove.chars().all(|c| c.is_ascii_digit()) {
            // Route through the event bus — apply_event handles the mutation,
            // recomputes the API call estimate, and persists state.
            let _ = tx.send(AppEvent::MrRemovedById(to_remove));
        } else {
            app.branches.retain(|b| b != &to_remove);
            // Branches live in projects.toml — persist there, not in tracker_state.json.
            save_branches_async(&app.branches, &app.base_url, &app.project_id).await;
        }
    } else if value.chars().all(|c| c.is_ascii_digit()) {
        // Add a new MR to track — route through the event bus so apply_event
        // handles the push, the fetch spawn, and the estimate recompute atomically.
        if !app.mrs.iter().any(|m| m.id == value) {
            let _ = tx.send(AppEvent::MrAdded(value.clone()));
        }
    } else if !app.branches.contains(&value) {
        // Add a new branch to track.
        app.branches.push(value.clone());
        // MRs already on it did not just arrive there (no toast, no arrival).
        app.just_added_branches.insert(value.clone());
        // Branches live in projects.toml — persist there, not in tracker_state.json.
        save_branches_async(&app.branches, &app.base_url, &app.project_id).await;

        let ctx = app.fetch_context();
        for mr in &mut app.mrs {
            if mr.status != MrStatus::Loading {
                mr.status = MrStatus::Loading;
                let cached = CachedMrData::from(&*mr);
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
