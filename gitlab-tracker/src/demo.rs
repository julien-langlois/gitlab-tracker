use crate::app::App;
use crate::config::{AppConfig, VisibleColumns};
use crate::events::{handle_key_event_demo, handle_mouse_event};
use crate::models::{
    AppEvent, GitlabMrState, MergeabilityStatus, MrStatus, Pipeline, PipelineJob, PipelineState,
    TrackedMr,
};
use crate::ui;
use crossterm::event::{self, Event, KeyEventKind};
use std::time::Duration;

/// Returns an ISO 8601 UTC timestamp offset by `days_ago` days from now.
/// Used to produce realistic, relative `updated_at` values in demo mode
/// so that the Activity badge (🟢 / 🟡 / 🔴) reflects the configured thresholds.
fn demo_updated_at(days_ago: i64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let ts = now - days_ago * 86_400;
    // Format as a minimal ISO 8601 UTC string understood by the activity_badge parser.
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    let days_since_epoch = ts / 86_400;
    // Compute calendar date from days since Unix epoch (1970-01-01).
    let (year, month, day) = days_since_epoch_to_ymd(days_since_epoch);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        year, month, day, hours, mins, secs
    )
}

/// Converts days since Unix epoch to a (year, month, day) tuple.
fn days_since_epoch_to_ymd(mut days: i64) -> (i64, u8, u8) {
    let mut year = 1970i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let months = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u8;
    for &m in &months {
        if days < m {
            break;
        }
        days -= m;
        month += 1;
    }
    (year, month, (days + 1) as u8)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Runs the application in demo mode with pre-populated mock data.
/// This mode is intended for screenshots, testing, and demonstrations.
pub async fn run_demo_mode(config: AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    // In demo mode optional columns start hidden — the demo.tape scenario uses [C]
    // to open the column picker and enable them live, showcasing the feature.
    let demo_config = AppConfig {
        visible_columns: VisibleColumns {
            activity: true,
            target_branch: false,
            labels: false,
            milestone: true,
            notes: true,
            tracker_ticket: false,
        },
        ..config
    };

    let mut app = App::new(
        "demo-token".into(),
        "123456".into(),
        "https://gitlab.com".into(),
        900,
        demo_config,
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
            mergeability: MergeabilityStatus::NotApproved,
            sha: Some("c9d0e1f2".into()),
            // Pre-populate mock pipelines for the demo so [P] shows data instantly.
            pipelines: vec![
                Pipeline {
                    id: 9981,
                    status: PipelineState::Success,
                    created_at: Some("2024-11-14T10:23:05Z".into()),
                    jobs: vec![
                        PipelineJob { name: "lint".into(),           stage: "test".into(),   status: "success".into(), duration: Some(18.0) },
                        PipelineJob { name: "unit-tests".into(),     stage: "test".into(),   status: "success".into(), duration: Some(74.0) },
                        PipelineJob { name: "build".into(),          stage: "build".into(),  status: "success".into(), duration: Some(42.0) },
                        PipelineJob { name: "deploy-staging".into(), stage: "deploy".into(), status: "success".into(), duration: Some(31.0) },
                    ],
                },
                Pipeline {
                    id: 9942,
                    status: PipelineState::Failed,
                    created_at: Some("2024-11-13T17:08:42Z".into()),
                    jobs: vec![
                        PipelineJob { name: "lint".into(),           stage: "test".into(),   status: "success".into(), duration: Some(17.0) },
                        PipelineJob { name: "unit-tests".into(),     stage: "test".into(),   status: "failed".into(),  duration: Some(61.0) },
                        PipelineJob { name: "build".into(),          stage: "build".into(),  status: "skipped".into(), duration: None },
                        PipelineJob { name: "deploy-staging".into(), stage: "deploy".into(), status: "skipped".into(), duration: None },
                    ],
                },
            ],
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
            author: "Marina Graphetti (@marina_gql)".into(),
            assignee: "Thomas Dubosc (@thomas_db)".into(),
            reviewers: vec![
                "Thomas Dubosc (@thomas_db)".into(),
                "Alex Devries (@alex_dev)".into(),
            ],
            milestone: "v2.4.0".into(),
            milestone_due_date: Some("2024-05-31".into()),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/104".into(),
            labels: vec![
                "feature".into(),
                "deploy::production".into(),
                "review::approved".into(),
                "size::L".into(),
            ],
            // Most recent timestamp so MR 104 lands at row 0 after the default sort
            // (UpdatedAt Descending) — required for the pipeline demo in demo.tape.
            updated_at: Some(demo_updated_at(1)),
            source_branch: "feat/graphql-mr-metadata".into(),
            target_branch: "main".into(),
            merged_by: Some("Thomas Dubosc (@thomas_db)".into()),
            merged_at: Some("2024-05-06T09:00:00.000Z".into()),
            recently_updated: false,
            // Demo: simulate a MR with several comments awaiting review.
            user_notes_count: 5,
            flagged: true,
            linked_ticket: None,
        },
        TrackedMr {
            id: "101".into(),
            title: "feat(auth): Add OAuth2 PKCE flow for mobile clients".into(),
            status: MrStatus::MergedIn(["main".into(), "staging".into()].into_iter().collect()),
            state: GitlabMrState::Merged,
            mergeability: MergeabilityStatus::Unknown,
            sha: Some("a1b2c3d4".into()),
            description: "Implemented PKCE challenge and verification flow.".into(),
            author: "Alex Devries (@alex_dev)".into(),
            assignee: "Sarah Codewyn (@sarah_code)".into(),
            reviewers: vec![],
            milestone: "v2.4.0".into(),
            milestone_due_date: Some("2024-05-31".into()),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/101".into(),
            labels: vec![
                "deploy::staging".into(),
                "review::approved".into(),
                "feature".into(),
            ],
            updated_at: Some(demo_updated_at(4)),
            source_branch: "feat/oauth2-pkce".into(),
            target_branch: "main".into(),
            merged_by: Some("Sarah Codewyn (@sarah_code)".into()),
            merged_at: Some("2024-05-01T09:45:00.000Z".into()),
            pipelines: vec![],
            recently_updated: false,
            user_notes_count: 0,
            flagged: false,
            linked_ticket: None,
        },
        TrackedMr {
            id: "102".into(),
            title: "fix(db): Resolve connection pool deadlocks under heavy load".into(),
            status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
            state: GitlabMrState::Opened,
            // Demo: simulate a MR with merge conflicts.
            mergeability: MergeabilityStatus::Conflict,
            sha: Some("e5f6g7h8".into()),
            description: "Adjusted max pool size and statement timeout.".into(),
            author: "Thomas Dubosc (@thomas_db)".into(),
            assignee: "Alex Devries (@alex_dev)".into(),
            reviewers: vec!["Sarah Codewyn (@sarah_code)".into()],
            milestone: "v2.4.0".into(),
            milestone_due_date: Some("2024-05-31".into()),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/102".into(),
            labels: vec!["bug".into(), "deploy::prod_pending".into()],
            updated_at: Some(demo_updated_at(14)),
            source_branch: "fix/db-pool-deadlock".into(),
            target_branch: "main".into(),
            merged_by: None,
            merged_at: None,
            pipelines: vec![],
            recently_updated: false,
            // Demo: simulate a MR with unread comments from reviewers.
            user_notes_count: 3,
            flagged: false,
            linked_ticket: None,
        },
        TrackedMr {
            id: "103".into(),
            title: "refactor(ui): Optimize Ratatui render loop with double buffering".into(),
            status: MrStatus::Loading,
            state: GitlabMrState::Opened,
            // Demo: simulate a MR that needs a rebase.
            mergeability: MergeabilityStatus::NeedsRebase,
            sha: None,
            description: "Reducing CPU usage during high-frequency ticks.".into(),
            author: "Julien Morel (@julien_m)".into(),
            assignee: "Julien Morel (@julien_m)".into(),
            reviewers: vec![],
            milestone: "v2.5.0".into(),
            milestone_due_date: None,
            web_url: "https://gitlab.com/demo/project/-/merge_requests/103".into(),
            labels: vec!["performance".into(), "review::needs_work".into()],
            updated_at: Some(demo_updated_at(1)),
            source_branch: "refactor/ui-double-buffer".into(),
            target_branch: "develop".into(),
            merged_by: None,
            merged_at: None,
            pipelines: vec![],
            recently_updated: false,
            user_notes_count: 0,
            flagged: false,
            linked_ticket: None,
        },
        TrackedMr {
            id: "105".into(),
            title: "fix(ci): Repair flaky integration tests in pipeline stage 3".into(),
            status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
            state: GitlabMrState::Closed,
            mergeability: MergeabilityStatus::DiscussionsNotResolved,
            sha: Some("3a4b5c6d".into()),
            description: "Isolated timing-dependent assertions and added retry logic.".into(),
            author: "Sarah Codewyn (@sarah_code)".into(),
            assignee: "Alex Devries (@alex_dev)".into(),
            reviewers: vec![],
            milestone: "v2.4.0".into(),
            milestone_due_date: Some("2024-05-31".into()),
            web_url: "https://gitlab.com/demo/project/-/merge_requests/105".into(),
            labels: vec!["bug".into(), "review::approved".into(), "size::S".into()],
            updated_at: Some(demo_updated_at(30)),
            source_branch: "fix/ci-flaky-tests".into(),
            target_branch: "main".into(),
            merged_by: None,
            merged_at: None,
            pipelines: vec![],
            recently_updated: false,
            user_notes_count: 6,
            flagged: false,
            linked_ticket: None,
        },
        TrackedMr {
            id: "106".into(),
            title: "chore(deps): Bump tokio to 1.37 and update async ecosystem".into(),
            status: MrStatus::Error,
            state: GitlabMrState::Opened,
            // Demo: simulate a cleanly mergeable MR.
            mergeability: MergeabilityStatus::Mergeable,
            sha: Some("7e8f9a0b".into()),
            description: "Routine dependency upgrade; resolves two CVEs in hyper transitive deps."
                .into(),
            author: "Bot Renovate (@bot_renovate)".into(),
            assignee: "Julien Morel (@julien_m)".into(),
            reviewers: vec![],
            milestone: "v2.5.0".into(),
            milestone_due_date: None,
            web_url: "https://gitlab.com/demo/project/-/merge_requests/106".into(),
            labels: vec!["dependencies".into(), "review::needs_work".into()],
            updated_at: Some(demo_updated_at(5)),
            source_branch: "chore/bump-tokio-1.37".into(),
            target_branch: "develop".into(),
            merged_by: None,
            merged_at: None,
            pipelines: vec![],
            recently_updated: false,
            user_notes_count: 0,
            flagged: false,
            linked_ticket: None,
        },
        TrackedMr {
            id: "107".into(),
            title: "feat(notif): Add desktop notifications on branch status change".into(),
            status: MrStatus::Loading,
            state: GitlabMrState::Opened,
            // Demo: simulate a cleanly mergeable MR.
            mergeability: MergeabilityStatus::RequestedChanges,
            sha: None,
            description: "Uses notify-rust to surface MR merge events as OS notifications.".into(),
            author: "Julien Morel (@julien_m)".into(),
            assignee: "Marina Graphetti (@marina_gql)".into(),
            reviewers: vec!["Thomas Dubosc (@thomas_db)".into()],
            milestone: "v2.5.0".into(),
            milestone_due_date: None,
            web_url: "https://gitlab.com/demo/project/-/merge_requests/107".into(),
            labels: vec![
                "feature".into(),
                "review::needs_work".into(),
                "size::M".into(),
            ],
            updated_at: Some(demo_updated_at(18)),
            source_branch: "feat/desktop-notifications".into(),
            target_branch: "main".into(),
            merged_by: None,
            merged_at: None,
            pipelines: vec![],
            recently_updated: false,
            // Demo: simulate a MR with one comment to address.
            user_notes_count: 1,
            flagged: true,
            linked_ticket: None,
        },
    ];

    // Apply the default sort (UpdatedAt Descending) so the table order at startup
    // matches exactly what the user sees — MR 104 is given the most recent timestamp
    // so it lands at row 0 after sorting.
    app.sort_mrs();
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
                    handle_mouse_event(mouse, term_width, &mut app, &tx);
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
