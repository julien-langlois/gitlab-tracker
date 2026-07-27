mod app;
mod config;
mod demo;
mod gitlab;
mod models;
mod storage;
mod ui;
mod utils;

use app::{ActivePane, App, TrackedMrExt};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
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
                                if app.branches.iter().all(|b| found.contains(b))
                                    && mr.sha.is_some()
                                    && !mr.title.starts_with("[Open]")
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
                // --- Mouse events: switch active pane on hover, route scroll to focused pane ---
                Event::Mouse(mouse) => {
                    // The inspector occupies the right 35% of the terminal width.
                    let term_width = terminal.size()?.width;
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

                // --- Keyboard events ---
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match key.code {
                        KeyCode::Esc => break,

                        // Tab cycles focus between panes without consuming arrow keys.
                        KeyCode::Tab if app.input.is_empty() => {
                            app.active_pane = app.active_pane.next();
                        }

                        // Arrow keys and j/k are routed based on the active pane.
                        KeyCode::Down | KeyCode::Char('j') if app.input.is_empty() => {
                            match app.active_pane {
                                ActivePane::Inspector => app.inspector_scroll_down(1),
                                ActivePane::Dashboard => app.next_row(),
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') if app.input.is_empty() => {
                            match app.active_pane {
                                ActivePane::Inspector => app.inspector_scroll_up(1),
                                ActivePane::Dashboard => app.prev_row(),
                            }
                        }

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

                        KeyCode::Char('r') | KeyCode::Char('R') if app.input.is_empty() => {
                            app.time_left = app.refresh_interval_secs;
                            let ctx = FetchContext {
                                base_url: app.base_url.clone(),
                                token: app.token.clone(),
                                project_id: app.project_id.clone(),
                                branches: app.branches.clone(),
                            };

                            for mr in &mut app.mrs {
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

                        KeyCode::Char('s') if app.input.is_empty() => {
                            app.cycle_sort_column();
                        }

                        KeyCode::Char('S') if app.input.is_empty() => {
                            app.toggle_sort_order();
                        }

                        KeyCode::Delete => {
                            if let Some(selected) = app.table_state.selected() {
                                if selected < app.mrs.len() {
                                    app.mrs.remove(selected);
                                    if app.mrs.is_empty() {
                                        app.table_state.select(None);
                                    } else if selected >= app.mrs.len() {
                                        app.table_state.select(Some(app.mrs.len() - 1));
                                    }
                                    save_state_async(&app.mrs, &app.branches, &last_known_branches)
                                        .await;
                                }
                            }
                        }

                        KeyCode::Char('x') if app.input.is_empty() => {
                            if let Some(selected) = app.table_state.selected() {
                                if selected < app.mrs.len() {
                                    app.mrs.remove(selected);
                                    if app.mrs.is_empty() {
                                        app.table_state.select(None);
                                    } else if selected >= app.mrs.len() {
                                        app.table_state.select(Some(app.mrs.len() - 1));
                                    }
                                    save_state_async(&app.mrs, &app.branches, &last_known_branches)
                                        .await;
                                }
                            }
                        }

                        KeyCode::Char(c) => app.input.push(c),
                        KeyCode::Backspace => {
                            app.input.pop();
                        }

                        KeyCode::Enter => {
                            let value = app.input.trim().to_string();
                            if !value.is_empty() {
                                if value.starts_with('-') {
                                    let to_remove = value.trim_start_matches('-').to_string();
                                    if to_remove.chars().all(|c| c.is_numeric()) {
                                        app.mrs.retain(|m| m.id != to_remove);
                                    } else {
                                        app.branches.retain(|b| b != &to_remove);
                                    }
                                    save_state_async(&app.mrs, &app.branches, &last_known_branches)
                                        .await;
                                    if app.mrs.is_empty() {
                                        app.table_state.select(None);
                                    }
                                } else if value.chars().all(|c| c.is_numeric()) {
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
                                        save_state_async(
                                            &app.mrs,
                                            &app.branches,
                                            &last_known_branches,
                                        )
                                        .await;

                                        let ctx = FetchContext {
                                            base_url: app.base_url.clone(),
                                            token: app.token.clone(),
                                            project_id: app.project_id.clone(),
                                            branches: app.branches.clone(),
                                        };

                                        spawn_mr_fetch(
                                            ctx,
                                            value,
                                            CachedMrData::default(),
                                            api_semaphore.clone(),
                                            tx.clone(),
                                        );
                                    }
                                } else {
                                    if !app.branches.contains(&value) {
                                        app.branches.push(value.clone());
                                        save_state_async(
                                            &app.mrs,
                                            &app.branches,
                                            &last_known_branches,
                                        )
                                        .await;

                                        let ctx = FetchContext {
                                            base_url: app.base_url.clone(),
                                            token: app.token.clone(),
                                            project_id: app.project_id.clone(),
                                            branches: app.branches.clone(),
                                        };

                                        for mr in &mut app.mrs {
                                            if mr.status != MrStatus::Loading {
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
                                app.input.clear();
                            }
                        }

                        _ => {}
                    }
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
