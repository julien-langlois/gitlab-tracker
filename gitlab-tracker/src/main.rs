mod app;
mod cli;
mod columns_core;
mod config;
mod demo;
mod events;
mod filters_core;
mod gitlab;
mod models;
mod settings;
mod settings_core;
mod shortcuts_core;
mod storage;
mod ui;
mod utils;

use app::{App, AppInit};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use events::{handle_key_event, handle_mouse_event};
use gitlab::MAX_CONCURRENT_REQUESTS;
use models::AppEvent;
use std::sync::Arc;
use std::time::Duration;
use storage::{
    get_or_prompt_token, load_or_create_config_async, load_state_async,
    migrate_legacy_keyring_entry, save_state_async, ProjectEntry,
};
use tokio::sync::Semaphore;

/// Converts a provider's raw `LabelColorMaps` (String pairs) into the ratatui-typed
/// `TrackerLabelColors` used by the Inspector renderer.
///
/// This is the only place in `gitlab-tracker` that bridges the provider contract
/// (colour-as-String, no ratatui dependency) with the UI layer (ratatui::Color).
/// Every future provider plugin follows the same path — no additional glue needed.
///
/// Compiled only when at least one tracker feature is enabled — the function is
/// unreachable in a vanilla build and would produce a dead-code warning otherwise.
// Extend to `#[cfg(any(feature = "redmine", feature = "jira"))]` when adding a new tracker.
#[cfg(feature = "redmine")]
fn build_tracker_colors(
    provider: &dyn gitlab_tracker_core::TrackerProvider,
) -> ui::tracker::TrackerLabelColors {
    use config::parse_color;
    use ui::tracker::TrackerLabelColors;

    let maps = provider.label_colors();

    let convert = |source: std::collections::HashMap<String, (String, String)>| {
        source
            .into_iter()
            .map(|(k, (bg, fg))| (k, (parse_color(&bg), parse_color(&fg))))
            .collect()
    };

    TrackerLabelColors {
        tracker_type: convert(maps.tracker_type),
        priority: convert(maps.priority),
    }
}

/// Initialises file-based logging (rolling daily, non-blocking).
///
/// Writes to `~/.config/gitlab-tracker/gitlab-tracker.log`.
/// Log level is controlled by the `RUST_LOG` env var (default: `warn`).
/// Returns the `WorkerGuard` that must be kept alive for the duration of the
/// program — dropping it flushes and closes the log file.
fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = storage::get_save_dir()?;
    std::fs::create_dir_all(&log_dir).ok()?;

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("gitlab-tracker")
        .filename_suffix("log")
        .max_log_files(10)
        .build(&log_dir)
        .ok()?;

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
}

/// Resolves `refresh_interval_secs` from env var, config file, or default.
fn resolve_refresh_interval(config: &config::AppConfig) -> u64 {
    std::env::var("GITLAB_REFRESH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .or(config.refresh_interval_secs)
        .unwrap_or(900)
}

/// Applies project-scoped overrides from `projects.toml` onto the global `AppConfig`.
///
/// Priority: `projects.toml` entry > `config.json` value > compiled-in default.
/// Only fields explicitly set in the `ProjectEntry` override the config — `None`
/// means "use whatever config.json / the default says".
fn apply_project_overrides(config: &mut config::AppConfig, project: &ProjectEntry) {
    if let Some(branches) = &project.default_branches {
        config.default_branches = branches.clone();
    }
    if let Some(prefixes) = &project.table_label_prefixes {
        config.table_label_prefixes = prefixes.clone();
    }
    if let Some(profile) = &project.complexity_profile {
        config.complexity_profile = profile.clone();
    }
    if let Some(secs) = project.refresh_interval_secs {
        config.refresh_interval_secs = Some(secs);
    }
    if let Some(days) = project.activity_stale_days {
        config.activity_stale_days = days;
    }
    if let Some(days) = project.activity_recent_days {
        config.activity_recent_days = days;
    }
    if let Some(show_cockpit) = project.show_cockpit {
        config.show_cockpit = show_cockpit;
    }
    if let Some(thresholds) = &project.cockpit_thresholds {
        config.cockpit_thresholds = thresholds.clone();
    }
    if let Some(cols) = &project.visible_columns {
        config.visible_columns = cols.clone();
    }
    if let Some(colors) = &project.label_colors {
        config.label_colors = colors.clone();
    }
    // Propagate the per-project GitLab username — drives the "Assigned to me" and
    // "Reviewer: me" filter visibility in the picker popup.
    config.gitlab_username = project.gitlab_username.clone();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Args::parse();

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

    // Resolve the selected project from CLI flags, projects.toml, env vars, or prompt.
    // Project-scoped settings override the global config.json values when present.
    let project = cli::resolve_project(&args).await;
    apply_project_overrides(&mut config, &project);

    if cli::run_command(args.command.as_ref(), &project).await? {
        return Ok(());
    }

    let project_settings = project.clone();

    // Extract tracked_branches before moving project fields.
    let project_tracked_branches = project.tracked_branches.clone();
    // Extract stats settings before the project fields are partially moved below.
    #[cfg(feature = "stats")]
    let stats_retention_days = project
        .stats
        .as_ref()
        .and_then(|s| s.retention_days)
        .unwrap_or(365);
    #[cfg(feature = "stats")]
    let stats_sprint_weeks = project
        .stats
        .as_ref()
        .and_then(|s| s.sprint_weeks)
        .unwrap_or(2);
    // Discovery lives at the project level — independent of the stats feature.
    // Any reviewer can opt in to automatic MR population without enabling analytics.
    let discover_new_mrs = project.discover_new_mrs.unwrap_or(false);
    let base_url = project.gitlab_url;
    let project_name = project.name.clone();
    let project_id = project.project_id;
    let refresh_interval_secs = resolve_refresh_interval(&config);

    // One-time silent migration: move any token stored under legacy keyring keys
    // (service rename + flat account → per-instance URL account) to the current
    // multi-tenant key, then delete orphaned legacy entries.
    migrate_legacy_keyring_entry(&base_url);

    // `get_or_prompt_token` returns a `Zeroizing<String>` that wipes the secret
    // from memory when dropped. We extract the inner `String` here so the rest
    // of the program is unaffected; the Zeroizing wrapper is immediately dropped.
    let token = get_or_prompt_token(&base_url).to_string();

    // ── Optional tracker integration ──────────────────────────────────────────
    // The tracker config lives inside the active `ProjectEntry` under the generic
    // `[project.tracker]` key. The `provider` field selects the plugin at runtime;
    // all provider-specific fields are forwarded via `extra: toml::Table`.
    //
    // This block is the only place that maps a provider name to a concrete plugin
    // crate — adding a new provider (Jira, Linear, …) means adding one `else if`
    // branch here and a new feature-gated crate, without touching any other file.
    #[cfg(feature = "redmine")]
    let redmine_provider: Option<(app::TrackerHandle, app::TicketTransitionHandle)> = {
        use std::sync::Arc;

        // Extract the generic tracker config from the active project entry.
        // If absent or pointing to a different provider, the Redmine integration
        // stays inactive without any error.
        let tracker_cfg = project
            .tracker
            .as_ref()
            .filter(|t| t.provider.eq_ignore_ascii_case("redmine"));

        match tracker_cfg {
            None => {
                // No [project.tracker] section configured — integration is opt-in,
                // so we stay silent and inactive. The user enables Redmine by adding
                // the section manually to projects.toml (or via a future setup command).
                // We never prompt here to avoid asking on every startup.
                tracing::info!("No [project.tracker] section found — tracker integration disabled");
                None
            }
            Some(cfg) => {
                // Deserialise the provider-specific fields from the opaque `extra`
                // table — only the Redmine plugin knows which keys it expects.
                let mut redmine_cfg: gitlab_tracker_redmine::config::RedmineConfig =
                    cfg.extra.clone().try_into().unwrap_or_default();
                redmine_cfg.url = cfg.url.clone();

                // Apply REDMINE_URL env override if set.
                redmine_cfg.apply_env_override();

                if !redmine_cfg.is_active() {
                    tracing::warn!(
                        "[project.tracker] found but url is empty — integration disabled"
                    );
                    None
                } else {
                    tracing::info!(url = %redmine_cfg.url, "Redmine integration active (from projects.toml)");
                    // Token is keyed by URL — each tenant instance is independent.
                    gitlab_tracker_redmine::keyring::get_or_prompt_token(&redmine_cfg.url).map(
                        |tok| {
                            let provider = Arc::new(gitlab_tracker_redmine::RedmineProvider::new(
                                redmine_cfg,
                                tok.to_string(),
                            ));
                            (
                                Arc::clone(&provider)
                                    as Arc<dyn gitlab_tracker_core::TrackerProvider>,
                                provider as Arc<dyn gitlab_tracker_core::TicketTransitionProvider>,
                            )
                        },
                    )
                }
            }
        }
    };

    // Initialise logging before ratatui takes over the terminal.
    // The guard must stay alive for the duration of the program.
    let _log_guard = init_logging();

    // Detect the terminal colour scheme BEFORE ratatui::init() takes ownership of
    // the terminal (raw mode). terminal-colorsaurus sends an OSC 11 query and reads
    // back the background colour; it must run while the terminal is still in cooked
    // mode. Falls back to Dark when the terminal does not respond (TTY, tmux, etc.).
    let theme_mode = {
        use terminal_colorsaurus::{theme_mode, QueryOptions};
        match theme_mode(QueryOptions::default()) {
            Ok(terminal_colorsaurus::ThemeMode::Light) => crate::ui::theme::ThemeMode::Light,
            _ => crate::ui::theme::ThemeMode::Dark,
        }
    };
    let palette = crate::ui::theme::Palette::for_mode(theme_mode);
    tracing::info!(mode = ?theme_mode, "Terminal theme detected");

    // Enable mouse capture so we can detect hover and scroll events per pane.
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let mut terminal = ratatui::init();

    let (
        saved_mrs,
        migrated_branches,
        mut last_known_branches,
        saved_discovery_started_at,
        dismissed_mr_ids,
    ) = load_state_async(&base_url, &project_id).await;
    // Clone complexity_profile before the move into App::new so the backfill
    // closure below can still reference it after `config` is consumed.
    #[cfg(feature = "stats")]
    let complexity_profile = config.complexity_profile.clone();

    let mut app = App::new(AppInit {
        token,
        project_id: project_id.clone(),
        base_url: base_url.clone(),
        project_name,
        refresh_interval_secs,
        config,
        theme: palette,
        project_settings,
    });

    // Branch resolution priority:
    //   1. tracked_branches in projects.toml (canonical source after migration)
    //   2. branches from tracker_state.json (legacy — one-shot migration done in load_state_async)
    //   3. default_branches from config (first run)
    app.branches = if let Some(ref tb) = project_tracked_branches {
        if tb.is_empty() {
            app.config.default_branches.clone()
        } else {
            tb.clone()
        }
    } else if !migrated_branches.is_empty() {
        migrated_branches
    } else {
        app.config.default_branches.clone()
    };

    // Inject the discovery flag unconditionally — independent of the stats feature.
    app.discovery_enabled = discover_new_mrs;
    // Restore the discovery anchor timestamp from the state file so newly created
    // MRs are filtered correctly across restarts.
    app.discovery_started_at = saved_discovery_started_at;
    // Restore manually dismissed MRs so discovery does not re-add them after restart.
    app.dismissed_mr_ids = dismissed_mr_ids;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let api_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    // Inject the tracker provider and derive colour maps from it.
    // Each provider owns its config and exposes `label_colors()` — main.rs only
    // converts the raw (String, String) pairs to ratatui::Color via `parse_color`.
    // This block is generic: any future provider (Jira, Linear, …) gets wired here
    // under its own feature flag without touching the logic below.
    #[cfg(feature = "redmine")]
    if let Some((provider, transitioner)) = redmine_provider {
        app.tracker_colors = build_tracker_colors(provider.as_ref());
        app.tracker = Some(provider);
        app.ticket_transitioner = Some(transitioner);
    }

    // Collect all shortcut blocks registered via inventory::submit! across every
    // linked crate (Core, Redmine if feature-enabled, any future plugin).
    // No explicit mention of any provider crate is needed here — adding a new
    // plugin only requires linking it (i.e. enabling its feature in Cargo.toml).
    app.shortcut_providers = gitlab_tracker_core::collect_all_blocks();

    // ── Stats DB initialisation ───────────────────────────────────────────────
    // Opened before `restore_from_saved` so that the very first MrLoaded events
    // can already record snapshots. The DB file lives alongside tracker_state.json
    // in the XDG config directory.
    #[cfg(feature = "stats")]
    {
        use gitlab_tracker_stats::StatsDb as _;
        use std::sync::Arc;

        // sqlx SqliteConnectOptions accepts either a plain path or a `sqlite:<path>` URI.
        // We pass the plain absolute path — `create_if_missing(true)` handles file creation.
        let db_path = storage::get_save_dir()
            .map(|d| d.join("stats.db"))
            .and_then(|p| p.to_str().map(|s| s.to_string()));

        if let Some(path) = db_path {
            match gitlab_tracker_stats::SqliteStatsDb::open(&path).await {
                Ok(db) => {
                    let db = Arc::new(db);

                    // Purge snapshots older than the configured retention threshold.
                    let retention_days = stats_retention_days;
                    match db.purge_old_snapshots(&project_id, retention_days).await {
                        Ok(n) if n > 0 => {
                            tracing::info!(rows = n, retention_days, "Purged old stats snapshots")
                        }
                        Err(e) => tracing::warn!(error = %e, "Stats purge failed"),
                        _ => {}
                    }

                    // Remove cross-day duplicates produced by repeated backfills:
                    // the UNIQUE constraint prevents same-day dupes but not cross-day ones.
                    match db.deduplicate_snapshots(&project_id).await {
                        Ok(n) if n > 0 => {
                            tracing::info!(rows = n, "Deduplicated stats snapshots")
                        }
                        Err(e) => tracing::warn!(error = %e, "Stats deduplication failed"),
                        _ => {}
                    }

                    // ── Backfill from tracker_state.json ─────────────────────
                    // On first run (or after a gap), seed the DB with the MRs
                    // already persisted on disk so stats are immediately useful
                    // without waiting for live merge/close events.
                    //
                    // Strategy:
                    //   • Merged/closed MRs  → OnMerge / OnClose snapshot
                    //     (recorded_at = merged_at / updated_at as best proxy)
                    //   • Open MRs           → OnRefresh snapshot (today)
                    //
                    // All upserts are idempotent: the UNIQUE constraint silently
                    // ignores duplicates if the DB already has data for today.
                    {
                        use crate::models::GitlabMrState;
                        use gitlab_tracker_stats::db::StatsDb as _;
                        use gitlab_tracker_stats::snapshot::{MrStatsSnapshot, SnapshotTrigger};

                        let today = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

                        let mut backfill = Vec::with_capacity(saved_mrs.len());
                        for mr in &saved_mrs {
                            let (trigger, recorded_at) = match &mr.state {
                                GitlabMrState::Merged => (
                                    SnapshotTrigger::OnMerge,
                                    mr.merged_at.clone().unwrap_or_else(|| today.clone()),
                                ),
                                GitlabMrState::Closed => (
                                    SnapshotTrigger::OnClose,
                                    mr.updated_at.clone().unwrap_or_else(|| today.clone()),
                                ),
                                GitlabMrState::Opened => {
                                    (SnapshotTrigger::OnRefresh, today.clone())
                                }
                            };

                            let diff = mr.diff_stats.as_ref();
                            let pipeline_count = mr.pipelines.len() as u32;
                            let pipeline_failure_count =
                                mr.pipelines
                                    .iter()
                                    .filter(|p| p.status == crate::models::PipelineState::Failed)
                                    .count() as u32;

                            let snap = MrStatsSnapshot {
                                mr_id: mr.id.clone(),
                                project_id: project_id.clone(),
                                title: mr.title.clone(),
                                trigger,
                                author: mr.author.clone().unwrap_or_default(),
                                assignee: mr.assignee.clone().filter(|s| !s.is_empty()),
                                reviewers: mr.reviewers.clone(),
                                merged_by: mr.merged_by.clone(),
                                milestone: mr.milestone.clone().filter(|s| !s.is_empty()),
                                labels: mr.labels.clone().unwrap_or_default(),
                                target_branch: mr.target_branch.clone().unwrap_or_default(),
                                state: format!("{:?}", mr.state).to_lowercase(),
                                created_at: mr.created_at.clone(),
                                merged_at: mr.merged_at.clone(),
                                updated_at: mr.updated_at.clone(),
                                files_changed: diff.map(|d| d.files_changed).unwrap_or(0),
                                additions: diff.map(|d| d.additions).unwrap_or(0),
                                deletions: diff.map(|d| d.deletions).unwrap_or(0),
                                commits_count: diff.map(|d| d.commits_count).unwrap_or(0),
                                diff_difficulty: diff.map(|d| d.difficulty(&complexity_profile)),
                                user_notes_count: mr.user_notes_count,
                                pipeline_count,
                                pipeline_failure_count,
                            };

                            // `recorded_at` is explicit so merged MRs appear at their real
                            // merge date rather than today; open MRs land at today —
                            // correct behaviour for OnRefresh.
                            backfill.push((snap, recorded_at));
                        }

                        // Single transaction: one fsync for the whole backfill instead of
                        // one per snapshot / label / reviewer. It also patches `created_at`
                        // on rows inserted before that field was tracked.
                        match db.backfill_snapshots(&backfill).await {
                            Ok(()) => tracing::info!(
                                count = backfill.len(),
                                "Stats backfill from tracker_state.json complete"
                            ),
                            Err(e) => tracing::warn!(error = %e, "Stats backfill failed"),
                        }
                    }

                    app.stats_view.sprint_weeks = stats_sprint_weeks;
                    app.stats_db = Some(db);
                }
                Err(e) => {
                    tracing::warn!(error = %e, path, "Failed to open stats DB — stats disabled");
                }
            }
        }
    }

    // Restore previously tracked MRs from disk, spawning background fetches as needed.
    app.restore_from_saved(saved_mrs, api_semaphore.clone(), tx.clone());

    // Warm the shared stats report on startup so cockpit release data is reconciled
    // before the user opens the fullscreen stats overlay.
    #[cfg(feature = "stats")]
    crate::ui::stats::trigger_background_stats_refresh(&mut app, &tx);

    // Fetch active milestones on startup so the autocomplete is ready immediately.
    gitlab::spawn_milestones_fetch(app.fetch_context(), tx.clone());

    // Fetch project label colours on startup so chip badges reflect GitLab colours
    // for labels not overridden in config.json.
    gitlab::spawn_gitlab_labels_fetch(app.fetch_context(), tx.clone());

    // Spawn the 1-second tick timer that drives the refresh countdown.
    let tx_timer = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let _ = tx_timer.send(AppEvent::Tick);
        }
    });

    // ── Main event loop ───────────────────────────────────────────────────────
    // Redraw only when something changed (async event, terminal input/resize) or
    // while the loading spinner is visible — an idle tracker costs ~0 CPU.
    let mut dirty = true;
    let mut last_title = String::new();
    loop {
        // Drain all pending async events before rendering. State is persisted and
        // the MR list re-sorted once per drain rather than once per event: at
        // startup N `MrLoaded` events would otherwise mean N full JSON rewrites.
        let mut needs_save = false;
        while let Ok(event) = rx.try_recv() {
            dirty = true;
            needs_save |= app
                .apply_event(event, api_semaphore.clone(), &tx, &mut last_known_branches)
                .await;
        }
        app.flush_pending_sort();
        if needs_save {
            save_state_async(
                &app.mrs,
                &last_known_branches,
                app.discovery_started_at.as_deref(),
                &app.dismissed_mr_ids,
                &base_url,
                &project_id,
            )
            .await;
        }

        let spinner_visible =
            !app.pending_initial_fetches.is_empty() || !app.pending_refresh_fetches.is_empty();
        if dirty || spinner_visible {
            // Update the terminal window title with live stats (OSC 0), only when
            // it actually changes.
            let mode_label = match app.input_mode {
                app::InputMode::Editing => "✏️  Editing",
                app::InputMode::ColumnPicker => "⚙️  Columns",
                app::InputMode::Settings => "⚙️  Settings",
                app::InputMode::FilterPicker => "🔍 Filter",
                app::InputMode::LogTime => "⏱️  Log Time",
                app::InputMode::Help => "❓ Help",
                app::InputMode::Normal => "Normal",
                #[cfg(feature = "stats")]
                app::InputMode::Stats => "📊 Stats",
            };
            let filter_label = app.active_filter.label(&app.filter_defs);
            let window_title = format!(
                "GitLab Tracker │ {} MRs │ {} │ {}",
                app.mrs.len(),
                mode_label,
                filter_label,
            );
            if window_title != last_title {
                crossterm::execute!(
                    std::io::stdout(),
                    crossterm::terminal::SetTitle(&window_title)
                )?;
                last_title = window_title;
            }

            terminal.draw(|f| ui::render_ui(f, &mut app))?;
            dirty = false;
        }

        if event::poll(Duration::from_millis(50))? {
            dirty = true;
            match event::read()? {
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    handle_mouse_event(mouse, size.width, size.height, &mut app, &tx);
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
