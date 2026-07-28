mod app;
mod config;
mod demo;
mod events;
mod gitlab;
mod models;
mod storage;
mod ui;
mod utils;

use app::{App, TrackedMrExt};
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use events::{handle_key_event, handle_mouse_event};
use gitlab::{spawn_mr_fetch, CachedMrData, FetchContext, MAX_CONCURRENT_REQUESTS};
use models::{AppEvent, MrStatus, TrackedMr};
use std::sync::Arc;
use std::time::Duration;
use storage::{
    get_or_prompt_token, load_or_create_config_async, load_state_async, save_state_async,
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

    let config = load_or_create_config_async().await;

    if args.demo {
        return demo::run_demo_mode(config).await;
    }

    if dotenvy::dotenv().is_err() {
        if let Some(config_dir) = storage::get_save_dir() {
            let global_env = config_dir.join(".env");
            let _ = dotenvy::from_path(global_env);
        }
    }

    let project_id = std::env::var("GITLAB_PROJECT_ID")
        .ok()
        .or_else(|| config.project_id.clone())
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| {
            eprintln!("❌ Error: GITLAB_PROJECT_ID is missing.");
            eprintln!("Please set it in your .env, in ~/.config/gitlab-tracker/config.json, or as an environment variable.");
            std::process::exit(1);
    });

    let base_url = std::env::var("GITLAB_URL")
        .ok()
        .or_else(|| config.gitlab_url.clone())
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| "https://gitlab.com".to_string());
    let base_url = base_url.trim_end_matches('/').to_string();

    let token = get_or_prompt_token();

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
            milestone: saved
                .milestone
                .clone()
                .unwrap_or_else(|| "None".to_string()),
            web_url: saved.web_url.clone().unwrap_or_default(),
            labels: saved.labels.clone().unwrap_or_default(),
            updated_at: saved.updated_at.clone(),
            state: saved.state.clone(),
            // Mergeability is not persisted — reset to Unknown on restart and re-fetched live.
            mergeability: models::MergeabilityStatus::Unknown,
            // Restore persisted pipelines — refreshed on each MR fetch.
            pipelines: saved.pipelines.clone(),
        });
        if initial_status == MrStatus::Loading {
            let cached = CachedMrData {
                title: Some(saved.title),
                sha: saved.sha,
                description: saved.description,
                author: saved.author,
                assignee: saved.assignee,
                milestone: saved.milestone,
                web_url: saved.web_url,
                labels: saved.labels,
                updated_at: saved.updated_at,
                pipelines: saved.pipelines,
            };

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
                                let _ = notify_rust::Notification::new()
                                    .summary("GitLab MR Tracker")
                                    .body(&format!(
                                        "MR !{} ({}) is now present on branch '{}'!",
                                        data.id, data.title, b
                                    ))
                                    .icon("dialog-information")
                                    .show();
                            }
                        }

                        // Update the persisted reference so subsequent refreshes won't re-notify.
                        last_known_branches.insert(data.id.clone(), data.branches.clone());

                        mr.title = data.title;
                        mr.sha = data.sha;
                        mr.status = MrStatus::MergedIn(data.branches);
                        mr.description = data.description;
                        mr.author = data.author;
                        mr.assignee = data.assignee;
                        mr.milestone = data.milestone;
                        mr.web_url = data.web_url;
                        mr.labels = data.labels;
                        mr.updated_at = data.updated_at;
                        mr.state = data.state;
                        mr.mergeability = data.mergeability;
                        mr.pipelines = data.pipelines;

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
                AppEvent::Tick => {
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
                                milestone: Some(mr.milestone.clone()),
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
