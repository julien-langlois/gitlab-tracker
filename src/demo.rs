use crate::app::App;
use crate::config::AppConfig;
use crate::events::{handle_key_event_demo, handle_mouse_event};
use crate::models::{AppEvent, GitlabMrState, MrStatus, TrackedMr};
use crate::ui;
use crossterm::event::{self, Event, KeyEventKind};
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
        // MR 104 is placed first so it is selected on startup for the Inspector scroll demo.
        // The sort demo in demo.tape will reorder the list afterwards.
        TrackedMr {
            id: "104".into(),
            title: "feat(api): Introduce GraphQL endpoint for MR metadata".into(),
            status: MrStatus::MergedIn(
                ["main".into(), "staging".into(), "production".into()]
                    .into_iter()
                    .collect(),
            ),
            state: GitlabMrState::Merged,
            sha: Some("c9d0e1f2".into()),
            description: concat!(
                "This MR introduces a fully typed GraphQL endpoint for Merge Request metadata,\n",
                "built with **async-graphql** and exposed via **Axum**.\n",
                "\n",
                "## Motivation\n",
                "\n",
                "REST endpoints were becoming unwieldy for clients needing partial data.\n",
                "GraphQL allows consumers to request only the fields they need, reducing\n",
                "payload size and round-trips significantly.\n",
                "\n",
                "The existing `/api/v1/mrs` endpoint returns the full MR object (~4 KB) even\n",
                "when the consumer only needs the `id` and `status` fields. At scale, this\n",
                "adds up to hundreds of megabytes of unnecessary data transfer per day.\n",
                "\n",
                "## Implementation\n",
                "\n",
                "- Defined `MergeRequestType` and `BranchType` as GraphQL objects via `#[Object]`.\n",
                "- Integrated `async-graphql-axum` for the HTTP transport layer.\n",
                "- Added a `/graphql` route behind the existing JWT middleware.\n",
                "- Exposed a `mergeRequests(ids: [ID!])` query for batch fetching.\n",
                "- Added `mrById(id: ID!)` for single-item lookups.\n",
                "- Wrote a custom scalar for ISO 8601 timestamps.\n",
                "- Documented all fields with `#[graphql(description = \"...\")]`.\n",
                "- Configured `introspection` disabled in production for security.\n",
                "- Added `depth_limit` and `complexity_limit` guards to prevent abuse.\n",
                "\n",
                "## Schema (excerpt)\n",
                "\n",
                "- `type MergeRequest { id, title, status, author, assignee, labels }`\n",
                "- `type Branch { name, protected, default }`\n",
                "- `type Query { mrById(id: ID!): MergeRequest }`\n",
                "- `type Query { mergeRequests(ids: [ID!]!): [MergeRequest!]! }`\n",
                "\n",
                "## Breaking Changes\n",
                "\n",
                "None. The REST API remains fully intact and is not deprecated.\n",
                "Both endpoints coexist and share the same service layer.\n",
                "\n",
                "## Testing\n",
                "\n",
                "- Unit tests: 47 added, all passing on CI.\n",
                "- Integration tests: 12 added against a live test Postgres instance.\n",
                "- Fuzz tests: 3 added for input validation on scalar deserialisation.\n",
                "- Manual QA: performed with GraphiQL playground against staging environment.\n",
                "- Load test: 500 rps sustained for 60 s with p99 < 40 ms on staging.\n",
                "\n",
                "## Follow-up\n",
                "\n",
                "- Subscription support (live updates over WebSocket) is out of scope here\n",
                "  and will be tracked in epic #88.\n",
                "- Federation with the Notifications service is planned for v2.6.0.\n",
                "- Persisted queries support will be evaluated once adoption grows.\n",
            )
            .into(),
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
            id: "101".into(),
            title: "feat(auth): Add OAuth2 PKCE flow for mobile clients".into(),
            status: MrStatus::MergedIn(["main".into(), "staging".into()].into_iter().collect()),
            state: GitlabMrState::Merged,
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
            state: GitlabMrState::Opened,
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
            state: GitlabMrState::Opened,
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
            id: "105".into(),
            title: "fix(ci): Repair flaky integration tests in pipeline stage 3".into(),
            status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
            state: GitlabMrState::Closed,
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
            state: GitlabMrState::Opened,
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
            state: GitlabMrState::Opened,
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

    // Enable mouse capture so VHS scroll simulation works in demo mode.
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

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
            match event::read()? {
                Event::Mouse(mouse) => {
                    let term_width = terminal.size()?.width;
                    handle_mouse_event(mouse, term_width, &mut app);
                }
                // Accept both Press and Repeat so held keys scroll smoothly in demo mode.
                Event::Key(key)
                    if (key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat)
                        && handle_key_event_demo(key, &mut app) =>
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
