mod app;
mod config;
mod gitlab;
mod models;
mod storage;
mod ui;
mod utils;

use app::{App, TrackedMrExt, REFRESH_INTERVAL_SECS};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use gitlab::{spawn_mr_fetch, CachedMrData, FetchContext, MAX_CONCURRENT_REQUESTS};
use models::{AppEvent, MrStatus, TrackedMr};
use std::collections::HashSet;
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
        let mut app = App::new(
            "demo-token".into(),
            "123456".into(),
            "https://gitlab.com".into(),
            config,
        );

        app.branches = vec!["main".into(), "staging".into(), "production".into()];
        app.mrs = vec![
            TrackedMr {
                id: "101".into(),
                title: "feat(auth): Add OAuth2 PKCE flow for mobile clients".into(),
                status: MrStatus::MergedIn(["main".into(), "staging".into()].into_iter().collect()),
                sha: Some("a1b2c3d4".into()),
                description: "Implemented PKCE challenge and verification flow.".into(),
                author: "alex_dev".into(),
                assignee: "sarah_code".into(),
                milestone: "v2.4.0".into(),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/101".into(),
                labels: vec![
                    "deploy::staging".into(),
                    "review::approved".into(),
                    "feature".into(),
                ],
            },
            TrackedMr {
                id: "102".into(),
                title: "fix(db): Resolve connection pool deadlocks under heavy load".into(),
                status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
                sha: Some("e5f6g7h8".into()),
                description: "Adjusted max pool size and statement timeout.".into(),
                author: "thomas_db".into(),
                assignee: "alex_dev".into(),
                milestone: "v2.4.0".into(),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/102".into(),
                labels: vec!["bug".into(), "deploy::prod_pending".into()],
            },
            TrackedMr {
                id: "103".into(),
                title: "refactor(ui): Optimize Ratatui render loop with double buffering".into(),
                status: MrStatus::Loading,
                sha: None,
                description: "Reducing CPU usage during high-frequency ticks.".into(),
                author: "julien_m".into(),
                assignee: "julien_m".into(),
                milestone: "v2.5.0".into(),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/103".into(),
                labels: vec!["performance".into(), "review::needs_work".into()],
            },
        ];

        app.table_state.select(Some(0));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
        let tx_timer = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                let _ = tx_timer.send(AppEvent::Tick);
            }
        });

        let mut terminal = ratatui::init();

        loop {
            while let Ok(event) = rx.try_recv() {
                if let AppEvent::Tick = event {
                    if app.time_left > 0 {
                        app.time_left -= 1;
                    } else {
                        app.time_left = REFRESH_INTERVAL_SECS;
                    }
                }
            }

            terminal.draw(|f| ui::render_ui(f, &mut app))?;

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat {
                        match key.code {
                            KeyCode::Esc => break,
                            KeyCode::Down | KeyCode::Char('j') => app.next_row(),
                            KeyCode::Up | KeyCode::Char('k') => app.prev_row(),
                            KeyCode::Char('s') => app.cycle_sort_column(),
                            KeyCode::Char('S') => app.toggle_sort_order(),
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                app.time_left = REFRESH_INTERVAL_SECS;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        ratatui::restore();
        return Ok(());
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

    let mut terminal = ratatui::init();

    let (saved_mrs, saved_branches) = load_state_async().await;
    let mut app = App::new(token, project_id, base_url, config);

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

    let mut is_bootstrapped = false;

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
                        if is_bootstrapped {
                            let old_branches = match &mr.status {
                                MrStatus::MergedIn(set) => set.clone(),
                                _ => HashSet::new(),
                            };
                            for b in &data.branches {
                                if !old_branches.contains(b) {
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
                        }

                        mr.title = data.title;
                        mr.sha = data.sha;
                        mr.status = MrStatus::MergedIn(data.branches);
                        mr.description = data.description;
                        mr.author = data.author;
                        mr.assignee = data.assignee;
                        mr.milestone = data.milestone;
                        mr.web_url = data.web_url;
                        mr.labels = data.labels;

                        app.sort_mrs();
                        save_state_async(&app.mrs, &app.branches).await;
                    }
                }
                AppEvent::MrFailed { id, error } => {
                    if let Some(mr) = app.mrs.find_mut(&id) {
                        mr.title = format!("⚠️ ERROR: {}", error);
                        mr.status = MrStatus::Error;
                        save_state_async(&app.mrs, &app.branches).await;
                    }
                }
                AppEvent::Tick => {
                    is_bootstrapped = true;

                    if app.time_left > 0 {
                        app.time_left -= 1;
                    } else {
                        app.time_left = REFRESH_INTERVAL_SECS;

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
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => break,
                        KeyCode::Down => app.next_row(),
                        KeyCode::Up => app.prev_row(),
                        KeyCode::Char('j') if app.input.is_empty() => app.next_row(),
                        KeyCode::Char('k') if app.input.is_empty() => app.prev_row(),

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
                            app.time_left = REFRESH_INTERVAL_SECS;
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
                                    save_state_async(&app.mrs, &app.branches).await;
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
                                    save_state_async(&app.mrs, &app.branches).await;
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
                                    save_state_async(&app.mrs, &app.branches).await;
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
                                        });
                                        app.table_state.select(Some(app.mrs.len() - 1));
                                        save_state_async(&app.mrs, &app.branches).await;

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
                                        save_state_async(&app.mrs, &app.branches).await;

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
            }
        }
    }

    ratatui::restore();
    Ok(())
}
