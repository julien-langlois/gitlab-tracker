mod app;
mod config;
mod demo;
mod events;
mod gitlab;
mod models;
mod storage;
mod ui;
mod utils;

use app::App;
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use events::{handle_key_event, handle_mouse_event};
use gitlab::MAX_CONCURRENT_REQUESTS;
use models::AppEvent;
use std::sync::Arc;
use std::time::Duration;
use storage::{
    get_or_prompt_token, load_or_create_config_async, load_state_async,
    migrate_legacy_keyring_entry, resolve_active_project, save_state_async, ProjectEntry,
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
    if let Some(cols) = &project.visible_columns {
        config.visible_columns = cols.clone();
    }
    if let Some(colors) = &project.label_colors {
        config.label_colors = colors.clone();
    }
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

    // Resolve the active project from projects.toml (env vars > active entry > prompt).
    // Project-scoped settings override the global config.json values when present.
    let project = resolve_active_project().await;
    apply_project_overrides(&mut config, &project);

    // Extract tracked_branches before moving project fields.
    let project_tracked_branches = project.tracked_branches.clone();
    let base_url = project.gitlab_url;
    let project_id = project.project_id;
    let refresh_interval_secs = resolve_refresh_interval(&config);

    // One-time silent migration: move any token stored under the legacy keyring
    // service name ("gitlab_tracker") to the canonical one ("gitlab-tracker"),
    // then delete the orphaned legacy entry.
    migrate_legacy_keyring_entry();

    // `get_or_prompt_token` returns a `Zeroizing<String>` that wipes the secret
    // from memory when dropped. We extract the inner `String` here so the rest
    // of the program is unaffected; the Zeroizing wrapper is immediately dropped.
    let token = get_or_prompt_token().to_string();

    // ── Optional Redmine integration ─────────────────────────────────────────
    // Loads config + prompts for the API token only when the `redmine` feature
    // is compiled in. The provider is `None` when the user skips the token prompt,
    // keeping the integration fully inactive without affecting the rest of the app.
    #[cfg(feature = "redmine")]
    let redmine_provider: Option<app::TrackerHandle> = {
        use std::sync::Arc;
        let mut redmine_cfg = gitlab_tracker_redmine::config::load_or_create_config().await;
        // Prompt for the Redmine URL if it is missing (first run or unconfigured).
        gitlab_tracker_redmine::config::ensure_redmine_config(&mut redmine_cfg).await;
        // Skip integration silently if no Redmine URL is configured yet.
        if redmine_cfg.redmine_url.trim().is_empty() {
            tracing::info!("Redmine URL not configured — integration disabled");
            None
        } else {
            gitlab_tracker_redmine::keyring::get_or_prompt_token().map(|tok| {
                let provider =
                    gitlab_tracker_redmine::RedmineProvider::new(redmine_cfg, tok.to_string());
                Arc::new(provider) as Arc<dyn gitlab_tracker_core::TrackerProvider>
            })
        }
    };

    // Initialise logging before ratatui takes over the terminal.
    // The guard must stay alive for the duration of the program.
    let _log_guard = init_logging();

    // Enable mouse capture so we can detect hover and scroll events per pane.
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

    let mut terminal = ratatui::init();

    let (saved_mrs, migrated_branches, mut last_known_branches) = load_state_async().await;
    let mut app = App::new(token, project_id, base_url, refresh_interval_secs, config);

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

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let api_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    // Inject the tracker provider and derive colour maps from it.
    // Each provider owns its config and exposes `label_colors()` — main.rs only
    // converts the raw (String, String) pairs to ratatui::Color via `parse_color`.
    // This block is generic: any future provider (Jira, Linear, …) gets wired here
    // under its own feature flag without touching the logic below.
    #[cfg(feature = "redmine")]
    if let Some(ref provider) = redmine_provider {
        app.tracker_colors = build_tracker_colors(provider.as_ref());
        app.tracker = redmine_provider;
    }

    // Restore previously tracked MRs from disk, spawning background fetches as needed.
    app.restore_from_saved(saved_mrs, api_semaphore.clone(), tx.clone());

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
    loop {
        // Drain all pending async events before rendering.
        while let Ok(event) = rx.try_recv() {
            let needs_save = app
                .apply_event(event, api_semaphore.clone(), &tx, &mut last_known_branches)
                .await;
            if needs_save {
                save_state_async(&app.mrs, &last_known_branches).await;
            }
        }

        // Update the terminal window title with live stats.
        // Uses the OSC 0 escape sequence supported by all modern terminal emulators.
        let mode_label = match app.input_mode {
            app::InputMode::Editing => "✏️  Editing",
            app::InputMode::ColumnPicker => "⚙️  Columns",
            app::InputMode::FilterPicker => "🔍 Filter",
            app::InputMode::LogTime => "⏱️  Log Time",
            app::InputMode::Normal => "Normal",
        };
        let filter_label = app.filter_mode.label();
        let window_title = format!(
            "GitLab Tracker │ {} MRs │ {} │ {}",
            app.mrs.len(),
            mode_label,
            filter_label,
        );
        crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::SetTitle(&window_title)
        )?;

        terminal.draw(|f| ui::render_ui(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
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
