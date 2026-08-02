mod app;
mod config;
mod demo;
mod events;
mod gitlab;
mod models;
mod storage;
mod ui;
mod utils;

use gitlab_tracker_notify as notify;

use app::{App, TrackedMrExt, RECENT_UPDATE_FADE_TICKS};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use events::{handle_key_event, handle_mouse_event};
use gitlab::{
    spawn_milestones_fetch, spawn_mr_fetch, CachedMrData, FetchContext, MAX_CONCURRENT_REQUESTS,
};
use models::{AppEvent, MrStatus, TrackedMr};
use std::sync::Arc;
use std::time::Duration;
use storage::{
    ensure_gitlab_config, get_or_prompt_token, load_or_create_config_async, load_state_async,
    migrate_legacy_keyring_entry, save_state_async,
};
use tokio::sync::Semaphore;

/// A fast terminal TUI dashboard for tracking GitLab Merge Requests across branches
#[derive(Parser, Debug)]
#[command(
    name = "gitlab-tracker",
    author,
    version,
    about,
    long_about = None
)]
struct Args {
    /// Launch in Demo Mode with mock data (for screenshots & testing)
    #[arg(long)]
    demo: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        default_panic(info);
    }));

    // Load .env before anything else so that all std::env::var() calls below
    // (including inside load_or_create_config_async) already see the env vars.
    if dotenvy::dotenv().is_err() {
        if let Some(config_dir) = storage::get_save_dir() {
            let global_env = config_dir.join(".env");
            let _ = dotenvy::from_path(global_env);
        }
    }

    let mut config = load_or_create_config_async().await;

    if args.demo {
        return demo::run_demo_mode(config).await;
    }

    // Ensure gitlab_url and project_id are available (env var > config > prompt).
    // ensure_gitlab_config is idempotent: it only prompts when a value is truly missing.
    ensure_gitlab_config(&mut config).await;

    let project_id = std::env::var("GITLAB_PROJECT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config.project_id.clone())
        .filter(|id| !id.trim().is_empty())
        .expect("GITLAB_PROJECT_ID must be set");

    let base_url = std::env::var("GITLAB_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| config.gitlab_url.clone())
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| "https://gitlab.com".to_string());
    let base_url = base_url.trim_end_matches('/').to_string();

    // One-time silent migration: move any token stored under the legacy keyring
    // service name ("gitlab_tracker") to the canonical one ("gitlab-tracker"),
    // then delete the orphaned legacy entry.
    migrate_legacy_keyring_entry();

    // `get_or_prompt_token` returns a `Zeroizing<String>` that wipes the secret
    // from memory when dropped. We extract the inner `String` here so the rest
    // of the program is unaffected; the Zeroizing wrapper is immediately dropped.
    let token = get_or_prompt_token().to_string();

    // Initialise file-based logging before ratatui takes over the terminal.
    // Writes to ~/.config/gitlab-tracker/gitlab-tracker.log (rolling daily).
    // Level is controlled by the RUST_LOG env var (default: warn).
    // Using a non-blocking writer so log I/O never stalls the event loop.
    let _log_guard = if let Some(log_dir) = storage::get_save_dir() {
        std::fs::create_dir_all(&log_dir).ok();
        let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("gitlab-tracker")
            .filename_suffix("log")
            .max_log_files(10)
            .build(&log_dir)
            .expect("Failed to initialise log file appender");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_writer(non_blocking)
            .with_ansi(false)
            .init();
        Some(guard)
    } else {
        None
    };

    // Enable mouse capture so we can detect hover and scroll events per pane.
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let mut terminal = ratatui::init();

    let (saved_mrs, saved_branches, mut last_known_branches) = load_state_async().await;
    let refresh_interval_secs = std::env::var("GITLAB_REFRESH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .or(config.refresh_interval_secs)
        .unwrap_or(900);
    let mut app = App::new(token, project_id, base_url, refresh_interval_secs, config);

    if saved_branches.is_empty() {
        app.branches = app.config.default_branches.clone();
    } else {
        app.branches = saved_branches;
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let api_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    let fetch_ctx = FetchContext {
        base_url: app.base_url.clone(),
        token: app.token.clone(),
        project_id: app.project_id.clone(),
        branches: app.branches.clone(),
    };

    for saved in saved_mrs {
        let initial_status = if !saved.found_branches.is_empty()
            && app
                .branches
                .iter()
                .all(|b| saved.found_branches.contains(b))
        {
            MrStatus::MergedIn(saved.found_branches.clone())
        } else {
            MrStatus::Loading
        };

        app.mrs.push(TrackedMr {
            id: saved.id.clone(),
            title: saved.title.clone(),
            status: initial_status.clone(),
            sha: saved.sha.clone(),
            description: saved
                .description
                .clone()
                .unwrap_or_else(|| "No description cached.".to_string()),
            author: saved
                .author
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            assignee: saved.assignee.clone().unwrap_or_else(|| "None".to_string()),
            reviewers: saved.reviewers.clone(),
            milestone: saved
                .milestone
                .clone()
                .unwrap_or_else(|| "None".to_string()),
            milestone_due_date: saved.milestone_due_date.clone(),
            web_url: saved.web_url.clone().unwrap_or_default(),
            labels: saved.labels.clone().unwrap_or_default(),
            updated_at: saved.updated_at.clone(),
            source_branch: saved
                .source_branch
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            target_branch: saved
                .target_branch
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            state: saved.state.clone(),
            merged_by: saved.merged_by.clone(),
            merged_at: saved.merged_at.clone(),
            // Mergeability is not persisted — reset to Unknown on restart and re-fetched live.
            mergeability: models::MergeabilityStatus::Unknown,
            // Restore persisted pipelines — refreshed on each MR fetch.
            pipelines: saved.pipelines.clone(),
            // On startup, no MR is considered recently updated.
            recently_updated: false,
            // Restore persisted notes count — refreshed on each MR fetch.
            user_notes_count: saved.user_notes_count,
            // Restore persisted flagged state.
            flagged: saved.flagged,
        });
        if initial_status == MrStatus::Loading {
            let cached = CachedMrData {
                title: Some(saved.title),
                sha: saved.sha,
                description: saved.description,
                author: saved.author,
                assignee: saved.assignee,
                web_url: saved.web_url,
                labels: saved.labels,
                updated_at: saved.updated_at,
                pipelines: saved.pipelines,
            };

            // Count each pending fetch so we can suppress change notifications
            // until the initial sync is complete (avoids spurious toasts on launch).
            app.pending_initial_fetches += 1;

            spawn_mr_fetch(
                fetch_ctx.clone(),
                saved.id,
                cached,
                api_semaphore.clone(),
                tx.clone(),
            );
        }
    }

    if !app.mrs.is_empty() {
        app.table_state.select(Some(0));
    }

    // Fetch active milestones on startup so the autocomplete is ready immediately.
    spawn_milestones_fetch(fetch_ctx.clone(), tx.clone());

    let tx_timer = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let _ = tx_timer.send(AppEvent::Tick);
        }
    });

    loop {
        while let Ok(event) = rx.try_recv() {
            match event {
                // 💡 Adaptation pour la structure Box<MrLoadedData>
                AppEvent::MrLoaded(data) => {
                    if let Some(mr) = app.mrs.find_mut(&data.id) {
                        // Compare new branches against the last persisted state to avoid
                        // re-notifying on restart or in-memory state that hasn't changed on disk.
                        let previously_known = last_known_branches
                            .get(&data.id)
                            .cloned()
                            .unwrap_or_default();

                        for b in &data.branches {
                            if !previously_known.contains(b) {
                                notify::mr_on_new_branch(&data.id, &data.title, b);
                            }
                        }

                        // Update the persisted reference so subsequent refreshes won't re-notify.
                        last_known_branches.insert(data.id.clone(), data.branches.clone());

                        // Decrement the startup fence: notifications are suppressed until
                        // all MRs from the saved state have received their first API response.
                        let notify_allowed = app.pending_initial_fetches == 0;
                        if app.pending_initial_fetches > 0 {
                            app.pending_initial_fetches -= 1;
                        }

                        // Detect whether this MR was actually updated since the last refresh.
                        // We compare the old `updated_at` before overwriting it.
                        let was_updated =
                            mr.updated_at.is_some() && mr.updated_at != data.updated_at;

                        // Trace field-level changes so they are visible in the log file.
                        // All comparisons happen before the fields are overwritten below.
                        if was_updated {
                            tracing::info!(
                                mr_id = %data.id,
                                old = %mr.updated_at.as_deref().unwrap_or("none"),
                                new = %data.updated_at.as_deref().unwrap_or("none"),
                                "MR updated_at changed",
                            );
                            if notify_allowed {
                                notify::mr_updated(
                                    &data.id,
                                    &data.title,
                                    data.updated_at.as_deref(),
                                );
                            }
                        }
                        if mr.mergeability != data.mergeability {
                            tracing::info!(
                                mr_id = %data.id,
                                old = ?mr.mergeability,
                                new = ?data.mergeability,
                                "MR mergeability changed",
                            );
                            if notify_allowed {
                                notify::mr_mergeability_changed(
                                    &data.id,
                                    &data.title,
                                    &format!("{:?}", mr.mergeability),
                                    &format!("{:?}", data.mergeability),
                                );
                            }
                        }
                        if mr.milestone != data.milestone {
                            tracing::info!(
                                mr_id = %data.id,
                                old = %mr.milestone,
                                new = %data.milestone,
                                "MR milestone changed",
                            );
                            if notify_allowed {
                                notify::mr_milestone_changed(
                                    &data.id,
                                    &data.title,
                                    &mr.milestone,
                                    &data.milestone,
                                );
                            }
                        }

                        mr.title = data.title;
                        mr.sha = data.sha;
                        mr.status = MrStatus::MergedIn(data.branches);
                        mr.description = data.description;
                        mr.author = data.author;
                        mr.assignee = data.assignee;
                        mr.reviewers = data.reviewers;
                        mr.milestone = data.milestone;
                        mr.milestone_due_date = data.milestone_due_date;
                        mr.web_url = data.web_url;
                        mr.labels = data.labels;
                        mr.updated_at = data.updated_at;
                        mr.source_branch = data.source_branch;
                        mr.target_branch = data.target_branch;
                        mr.state = data.state;
                        mr.merged_by = data.merged_by;
                        mr.merged_at = data.merged_at;
                        mr.mergeability = data.mergeability;
                        mr.pipelines = data.pipelines;
                        mr.recently_updated = was_updated;
                        mr.user_notes_count = data.user_notes_count;

                        // Arm (or re-arm) the global fade countdown.
                        if was_updated {
                            app.update_highlight_ticks = RECENT_UPDATE_FADE_TICKS;
                        }

                        app.sort_mrs();
                        save_state_async(&app.mrs, &app.branches, &last_known_branches).await;
                    }
                }
                AppEvent::MrFailed { id, error } => {
                    if let Some(mr) = app.mrs.find_mut(&id) {
                        mr.title = format!("⚠️ ERROR: {}", error);
                        mr.status = MrStatus::Error;
                        save_state_async(&app.mrs, &app.branches, &last_known_branches).await;
                    }
                }
                AppEvent::MilestonesLoaded(milestones) => {
                    app.milestones = milestones;
                }
                AppEvent::MilestoneMrsLoaded {
                    milestone_title,
                    mr_ids,
                } => {
                    let ctx = FetchContext {
                        base_url: app.base_url.clone(),
                        token: app.token.clone(),
                        project_id: app.project_id.clone(),
                        branches: app.branches.clone(),
                    };
                    let mut added = 0u32;
                    for mr_id in mr_ids {
                        // Skip MRs already tracked to avoid duplicates.
                        if app.mrs.iter().any(|m| m.id == mr_id) {
                            continue;
                        }
                        app.mrs.push(TrackedMr {
                            id: mr_id.clone(),
                            title: format!("Loading… ({})", milestone_title),
                            status: MrStatus::Loading,
                            state: models::GitlabMrState::Opened,
                            mergeability: models::MergeabilityStatus::Unknown,
                            sha: None,
                            description: String::new(),
                            author: "Loading".to_string(),
                            assignee: "Loading".to_string(),
                            reviewers: vec![],
                            milestone: milestone_title.clone(),
                            milestone_due_date: None,
                            web_url: String::new(),
                            labels: vec![],
                            updated_at: None,
                            source_branch: "unknown".to_string(),
                            target_branch: "unknown".to_string(),
                            merged_by: None,
                            merged_at: None,
                            pipelines: vec![],
                            recently_updated: false,
                            user_notes_count: 0,
                            // New MRs start unflagged.
                            flagged: false,
                        });
                        spawn_mr_fetch(
                            ctx.clone(),
                            mr_id,
                            CachedMrData::default(),
                            api_semaphore.clone(),
                            tx.clone(),
                        );
                        added += 1;
                    }
                    if added > 0 {
                        app.table_state.select(Some(0));
                        save_state_async(&app.mrs, &app.branches, &last_known_branches).await;
                    }
                }
                AppEvent::Tick => {
                    // Decrement the highlight fade countdown and clear flags when expired.
                    if app.update_highlight_ticks > 0 {
                        app.update_highlight_ticks -= 1;
                        if app.update_highlight_ticks == 0 {
                            for mr in &mut app.mrs {
                                mr.recently_updated = false;
                            }
                        }
                    }
                    if app.time_left > 0 {
                        app.time_left -= 1;
                    } else {
                        app.time_left = app.refresh_interval_secs;

                        let ctx = FetchContext {
                            base_url: app.base_url.clone(),
                            token: app.token.clone(),
                            project_id: app.project_id.clone(),
                            branches: app.branches.clone(),
                        };

                        for mr in &mut app.mrs {
                            if let MrStatus::MergedIn(ref found) = mr.status {
                                // Skip refresh for MRs that are fully merged into all branches —
                                // state != Opened ensures we don't skip still-open MRs.
                                if app.branches.iter().all(|b| found.contains(b))
                                    && mr.sha.is_some()
                                    && mr.state != crate::models::GitlabMrState::Opened
                                {
                                    continue;
                                }
                            }

                            mr.status = MrStatus::Loading;
                            let cached = CachedMrData {
                                title: Some(mr.title.clone()),
                                sha: mr.sha.clone(),
                                description: Some(mr.description.clone()),
                                author: Some(mr.author.clone()),
                                assignee: Some(mr.assignee.clone()),
                                web_url: Some(mr.web_url.clone()),
                                labels: Some(mr.labels.clone()),
                                updated_at: mr.updated_at.clone(),
                                pipelines: mr.pipelines.clone(),
                            };

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
        }

        terminal.draw(|f| ui::render_ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Mouse(mouse) => {
                    let term_width = terminal.size()?.width;
                    handle_mouse_event(mouse, term_width, &mut app);
                }
                Event::Key(key)
                    if key.kind == KeyEventKind::Press
                        && handle_key_event(
                            key,
                            &mut app,
                            &api_semaphore,
                            &tx,
                            &mut last_known_branches,
                        )
                        .await =>
                {
                    break;
                }
                _ => {}
            }
        }
    }

    // Disable mouse capture before restoring the terminal.
    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}
