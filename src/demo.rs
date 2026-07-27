use crate::app::App;
use crate::config::AppConfig;
use crate::models::{AppEvent, MrStatus, TrackedMr};
use crate::ui;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::time::Duration;

/// Runs the application in demo mode with pre-populated mock data.
/// This mode is intended for screenshots, testing, and demonstrations.
pub async fn run_demo_mode(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new(
        "demo-token".into(),
        "123456".into(),
        "https://gitlab.com".into(),
        900,
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
            updated_at: Some("2024-05-01T10:00:00.000Z".into()),
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
            updated_at: Some("2024-05-02T15:30:00.000Z".into()),
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
            updated_at: Some("2024-05-03T08:45:00.000Z".into()),
        },
        TrackedMr {
            id: "104".into(),
            title: "feat(api): Introduce GraphQL endpoint for MR metadata".into(),
            status: MrStatus::MergedIn(
                ["main".into(), "staging".into(), "production".into()]
                    .into_iter()
                    .collect(),
            ),
            sha: Some("c9d0e1f2".into()),
            description: "Expose MR data via a typed GraphQL schema using async-graphql.".into(),
            author: "marina_gql".into(),
            assignee: "thomas_db".into(),
            milestone: "v2.4.0".into(),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/104".into(),
            labels: vec![
                "feature".into(),
                "deploy::production".into(),
                "review::approved".into(),
                "size::L".into(),
            ],
            updated_at: Some("2024-05-04T09:15:00.000Z".into()),
        },
        TrackedMr {
            id: "105".into(),
            title: "fix(ci): Repair flaky integration tests in pipeline stage 3".into(),
            status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
            sha: Some("3a4b5c6d".into()),
            description: "Isolated timing-dependent assertions and added retry logic.".into(),
            author: "sarah_code".into(),
            assignee: "alex_dev".into(),
            milestone: "v2.4.0".into(),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/105".into(),
            labels: vec!["bug".into(), "review::approved".into(), "size::S".into()],
            updated_at: Some("2024-04-28T14:00:00.000Z".into()),
        },
        TrackedMr {
            id: "106".into(),
            title: "chore(deps): Bump tokio to 1.37 and update async ecosystem".into(),
            status: MrStatus::Error,
            sha: Some("7e8f9a0b".into()),
            description: "Routine dependency upgrade; resolves two CVEs in hyper transitive deps."
                .into(),
            author: "bot_renovate".into(),
            assignee: "julien_m".into(),
            milestone: "v2.5.0".into(),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/106".into(),
            labels: vec!["dependencies".into(), "review::needs_work".into()],
            updated_at: Some("2024-04-15T07:30:00.000Z".into()),
        },
        TrackedMr {
            id: "107".into(),
            title: "feat(notif): Add desktop notifications on branch status change".into(),
            status: MrStatus::Loading,
            sha: None,
            description: "Uses notify-rust to surface MR merge events as OS notifications.".into(),
            author: "julien_m".into(),
            assignee: "marina_gql".into(),
            milestone: "v2.5.0".into(),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/107".into(),
            labels: vec![
                "feature".into(),
                "review::needs_work".into(),
                "size::M".into(),
            ],
            updated_at: Some("2024-05-05T11:20:00.000Z".into()),
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
        // Drain the event queue before rendering
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::Tick = event {
                if app.time_left > 0 {
                    app.time_left -= 1;
                } else {
                    app.time_left = app.refresh_interval_secs;
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
                            app.time_left = app.refresh_interval_secs;
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
