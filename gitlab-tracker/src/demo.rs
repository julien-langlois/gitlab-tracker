use crate::app::App;
use crate::app::AppInit;
use crate::config::{AppConfig, VisibleColumns};
use crate::events::{handle_key_event, handle_mouse_event};
use crate::models::{
    AppEvent, GitlabMrState, MergeabilityStatus, MrStatus, Pipeline, PipelineJob, PipelineState,
    TrackedMr,
};
use crate::ui;
use crossterm::event::{self, Event, KeyEventKind};
use gitlab_tracker_core::{LinkedTicket, LINKED_TICKET_SCHEMA_VERSION};
use std::time::Duration;

// ── Demo stats seeding ────────────────────────────────────────────────────────

#[cfg(feature = "stats")]
/// Opens an in-memory SQLite DB and seeds it with realistic MR snapshots so the
/// Stats overlay renders meaningful data during demo mode (demo.tape / screenshots).
///
/// The dataset covers ~90 days, 4 authors, 2 milestones, and enough merged MRs
/// for Spearman correlations and P50/P90 percentiles to be computed.
pub(crate) async fn seed_demo_stats_db(
    project_id: &str,
) -> Option<std::sync::Arc<gitlab_tracker_stats::SqliteStatsDb>> {
    use gitlab_tracker_stats::snapshot::{MrStatsSnapshot, SnapshotTrigger};
    use gitlab_tracker_stats::SqliteStatsDb;
    use std::sync::Arc;

    let db = SqliteStatsDb::open(":memory:").await.ok()?;

    // Helper: build a merged snapshot recorded `days_ago` days in the past.
    // `cycle_days` controls the simulated review/merge cycle time.
    struct DemoMr<'a> {
        id: &'a str,
        title: &'a str,
        author: &'a str,
        assignee: Option<&'a str>,
        reviewers: Vec<&'a str>,
        milestone: &'a str,
        labels: Vec<&'a str>,
        files_changed: u32,
        additions: u32,
        deletions: u32,
        commits: u32,
        notes: u32,
        pipeline_count: u32,
        pipeline_failures: u32,
        /// Days ago the MR was merged (controls recorded_at).
        merged_days_ago: i64,
        /// Simulated cycle time in hours (created_at → merged_at proxy).
        cycle_hours: f64,
    }

    // Realistic dataset: mix of fast (hotfix) and slow (feature) MRs across two milestones.
    let dataset: &[DemoMr] = &[
        DemoMr {
            id: "201",
            title: "feat(api): GraphQL endpoint for MR metadata",
            author: "marina_gql",
            assignee: Some("thomas_db"),
            reviewers: vec!["thomas_db", "alex_dev"],
            milestone: "v2.4.0",
            labels: vec!["feature", "size::L"],
            files_changed: 12,
            additions: 487,
            deletions: 53,
            commits: 8,
            notes: 5,
            pipeline_count: 2,
            pipeline_failures: 1,
            merged_days_ago: 5,
            cycle_hours: 72.0,
        },
        DemoMr {
            id: "202",
            title: "feat(auth): OAuth2 PKCE flow for mobile clients",
            author: "alex_dev",
            assignee: Some("sarah_code"),
            reviewers: vec!["sarah_code"],
            milestone: "v2.4.0",
            labels: vec!["feature", "size::M"],
            files_changed: 4,
            additions: 89,
            deletions: 12,
            commits: 3,
            notes: 2,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 10,
            cycle_hours: 48.0,
        },
        DemoMr {
            id: "203",
            title: "fix(db): Connection pool deadlocks under heavy load",
            author: "thomas_db",
            assignee: Some("alex_dev"),
            reviewers: vec!["sarah_code"],
            milestone: "v2.4.0",
            labels: vec!["bug", "size::M"],
            files_changed: 6,
            additions: 231,
            deletions: 18,
            commits: 5,
            notes: 8,
            pipeline_count: 3,
            pipeline_failures: 1,
            merged_days_ago: 15,
            cycle_hours: 96.0,
        },
        DemoMr {
            id: "204",
            title: "fix(ci): Repair flaky integration tests",
            author: "sarah_code",
            assignee: Some("alex_dev"),
            reviewers: vec![],
            milestone: "v2.4.0",
            labels: vec!["bug", "size::S"],
            files_changed: 2,
            additions: 34,
            deletions: 8,
            commits: 1,
            notes: 6,
            pipeline_count: 4,
            pipeline_failures: 2,
            merged_days_ago: 20,
            cycle_hours: 24.0,
        },
        DemoMr {
            id: "205",
            title: "chore(deps): Bump tokio to 1.37",
            author: "bot_renovate",
            assignee: Some("julien_m"),
            reviewers: vec![],
            milestone: "v2.4.0",
            labels: vec!["deps", "size::S"],
            files_changed: 1,
            additions: 12,
            deletions: 12,
            commits: 2,
            notes: 0,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 22,
            cycle_hours: 12.0,
        },
        DemoMr {
            id: "206",
            title: "feat(notif): Desktop notifications on branch change",
            author: "julien_m",
            assignee: Some("marina_gql"),
            reviewers: vec!["thomas_db"],
            milestone: "v2.4.0",
            labels: vec!["feature", "size::M"],
            files_changed: 7,
            additions: 312,
            deletions: 41,
            commits: 6,
            notes: 3,
            pipeline_count: 2,
            pipeline_failures: 0,
            merged_days_ago: 25,
            cycle_hours: 120.0,
        },
        DemoMr {
            id: "207",
            title: "refactor(ui): Double buffering in render loop",
            author: "julien_m",
            assignee: Some("marina_gql"),
            reviewers: vec!["alex_dev"],
            milestone: "v2.4.0",
            labels: vec!["perf", "size::L"],
            files_changed: 9,
            additions: 198,
            deletions: 87,
            commits: 4,
            notes: 4,
            pipeline_count: 2,
            pipeline_failures: 1,
            merged_days_ago: 28,
            cycle_hours: 56.0,
        },
        DemoMr {
            id: "208",
            title: "fix(auth): Token refresh race condition",
            author: "alex_dev",
            assignee: Some("thomas_db"),
            reviewers: vec!["marina_gql"],
            milestone: "v2.4.0",
            labels: vec!["bug", "size::S"],
            files_changed: 3,
            additions: 67,
            deletions: 9,
            commits: 2,
            notes: 1,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 30,
            cycle_hours: 18.0,
        },
        DemoMr {
            id: "209",
            title: "feat(stats): Spearman correlation engine",
            author: "marina_gql",
            assignee: Some("julien_m"),
            reviewers: vec!["thomas_db", "alex_dev"],
            milestone: "v2.5.0",
            labels: vec!["feature", "size::XL"],
            files_changed: 18,
            additions: 820,
            deletions: 120,
            commits: 12,
            notes: 11,
            pipeline_count: 3,
            pipeline_failures: 1,
            merged_days_ago: 35,
            cycle_hours: 168.0,
        },
        DemoMr {
            id: "210",
            title: "fix(pipeline): Skip deploy on draft MRs",
            author: "thomas_db",
            assignee: Some("sarah_code"),
            reviewers: vec![],
            milestone: "v2.5.0",
            labels: vec!["bug", "size::S"],
            files_changed: 1,
            additions: 8,
            deletions: 2,
            commits: 1,
            notes: 0,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 38,
            cycle_hours: 6.0,
        },
        DemoMr {
            id: "211",
            title: "feat(tracker): Redmine ticket auto-link",
            author: "julien_m",
            assignee: Some("thomas_db"),
            reviewers: vec!["sarah_code"],
            milestone: "v2.5.0",
            labels: vec!["feature", "size::L"],
            files_changed: 10,
            additions: 390,
            deletions: 55,
            commits: 7,
            notes: 7,
            pipeline_count: 2,
            pipeline_failures: 0,
            merged_days_ago: 42,
            cycle_hours: 88.0,
        },
        DemoMr {
            id: "212",
            title: "chore(lint): Enforce clippy::pedantic workspace-wide",
            author: "sarah_code",
            assignee: Some("julien_m"),
            reviewers: vec![],
            milestone: "v2.5.0",
            labels: vec!["chore", "size::S"],
            files_changed: 5,
            additions: 43,
            deletions: 38,
            commits: 2,
            notes: 1,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 45,
            cycle_hours: 14.0,
        },
        DemoMr {
            id: "213",
            title: "feat(api): Rate-limit middleware with token bucket",
            author: "alex_dev",
            assignee: Some("marina_gql"),
            reviewers: vec!["thomas_db"],
            milestone: "v2.5.0",
            labels: vec!["feature", "size::M"],
            files_changed: 8,
            additions: 276,
            deletions: 32,
            commits: 5,
            notes: 3,
            pipeline_count: 2,
            pipeline_failures: 0,
            merged_days_ago: 50,
            cycle_hours: 60.0,
        },
        DemoMr {
            id: "214",
            title: "fix(ui): Colour mismatch on dark themes",
            author: "marina_gql",
            assignee: Some("sarah_code"),
            reviewers: vec![],
            milestone: "v2.5.0",
            labels: vec!["bug", "size::XS"],
            files_changed: 1,
            additions: 4,
            deletions: 4,
            commits: 1,
            notes: 0,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 52,
            cycle_hours: 4.0,
        },
        DemoMr {
            id: "215",
            title: "feat(export): JSON and CSV report generation",
            author: "thomas_db",
            assignee: Some("julien_m"),
            reviewers: vec!["marina_gql", "alex_dev"],
            milestone: "v2.5.0",
            labels: vec!["feature", "size::L"],
            files_changed: 14,
            additions: 560,
            deletions: 88,
            commits: 9,
            notes: 6,
            pipeline_count: 2,
            pipeline_failures: 1,
            merged_days_ago: 58,
            cycle_hours: 104.0,
        },
        DemoMr {
            id: "216",
            title: "fix(db): NULL handling in migration v3",
            author: "julien_m",
            assignee: Some("alex_dev"),
            reviewers: vec![],
            milestone: "v2.5.0",
            labels: vec!["bug", "size::S"],
            files_changed: 2,
            additions: 18,
            deletions: 6,
            commits: 1,
            notes: 2,
            pipeline_count: 2,
            pipeline_failures: 1,
            merged_days_ago: 62,
            cycle_hours: 10.0,
        },
        DemoMr {
            id: "217",
            title: "perf(cache): LRU eviction for diff stats cache",
            author: "sarah_code",
            assignee: Some("marina_gql"),
            reviewers: vec!["thomas_db"],
            milestone: "v2.5.0",
            labels: vec!["perf", "size::M"],
            files_changed: 6,
            additions: 144,
            deletions: 29,
            commits: 4,
            notes: 4,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 68,
            cycle_hours: 42.0,
        },
        DemoMr {
            id: "218",
            title: "chore(release): Bump version to v2.4.4",
            author: "bot_renovate",
            assignee: Some("julien_m"),
            reviewers: vec![],
            milestone: "v2.4.0",
            labels: vec!["chore", "size::XS"],
            files_changed: 2,
            additions: 6,
            deletions: 6,
            commits: 1,
            notes: 0,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 72,
            cycle_hours: 2.0,
        },
        DemoMr {
            id: "219",
            title: "feat(auth): Session invalidation on password change",
            author: "alex_dev",
            assignee: Some("thomas_db"),
            reviewers: vec!["sarah_code"],
            milestone: "v2.5.0",
            labels: vec!["feature", "size::M"],
            files_changed: 5,
            additions: 178,
            deletions: 22,
            commits: 4,
            notes: 3,
            pipeline_count: 2,
            pipeline_failures: 0,
            merged_days_ago: 75,
            cycle_hours: 54.0,
        },
        DemoMr {
            id: "220",
            title: "fix(api): Pagination off-by-one on large datasets",
            author: "marina_gql",
            assignee: Some("alex_dev"),
            reviewers: vec![],
            milestone: "v2.5.0",
            labels: vec!["bug", "size::S"],
            files_changed: 1,
            additions: 11,
            deletions: 3,
            commits: 1,
            notes: 1,
            pipeline_count: 1,
            pipeline_failures: 0,
            merged_days_ago: 80,
            cycle_hours: 8.0,
        },
    ];

    // Compute a RFC3339 timestamp `days_ago` days before now.
    let ts_days_ago = |days: i64| -> String {
        let dt = chrono::Utc::now() - chrono::Duration::days(days);
        dt.to_rfc3339()
    };

    for mr in dataset {
        let recorded_at = ts_days_ago(mr.merged_days_ago);
        // Approximate created_at by subtracting the cycle time from merged_at.
        let created_at = {
            let merged = chrono::Utc::now() - chrono::Duration::days(mr.merged_days_ago);
            let created = merged - chrono::Duration::hours(mr.cycle_hours as i64);
            Some(created.to_rfc3339())
        };

        let snap = MrStatsSnapshot {
            mr_id: mr.id.to_string(),
            project_id: project_id.to_string(),
            title: mr.title.to_string(),
            trigger: SnapshotTrigger::OnMerge,
            author: mr.author.to_string(),
            assignee: mr.assignee.map(str::to_string),
            reviewers: mr.reviewers.iter().map(|s| s.to_string()).collect(),
            merged_by: mr.assignee.map(str::to_string),
            milestone: Some(mr.milestone.to_string()),
            labels: mr.labels.iter().map(|s| s.to_string()).collect(),
            target_branch: "main".to_string(),
            state: "merged".to_string(),
            created_at,
            merged_at: Some(recorded_at.clone()),
            updated_at: Some(recorded_at.clone()),
            files_changed: mr.files_changed,
            additions: mr.additions,
            deletions: mr.deletions,
            commits_count: mr.commits,
            diff_difficulty: Some(
                (mr.files_changed as f64 * 0.4
                    + (mr.additions + mr.deletions) as f64 * 0.005
                    + mr.commits as f64 * 0.1)
                    .min(10.0),
            ),
            user_notes_count: mr.notes,
            pipeline_count: mr.pipeline_count,
            pipeline_failure_count: mr.pipeline_failures,
        };

        let _ = db.upsert_snapshot_at(&snap, &recorded_at).await;
    }

    Some(Arc::new(db))
}

/// Returns an ISO 8601 UTC timestamp offset by `days_ago` days from now.
/// Used to produce realistic, relative `updated_at` values in demo mode
/// so that the Activity badge (🟢 / 🟡 / 🔴) reflects the configured thresholds.
fn demo_updated_at(days_ago: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::days(days_ago))
        .format("%Y-%m-%dT%H:%M:%S.000Z")
        .to_string()
}

/// Runs the application in demo mode with pre-populated mock data.
/// This mode is intended for screenshots, testing, and demonstrations.
pub async fn run_demo_mode(config: AppConfig) -> anyhow::Result<()> {
    use anyhow::Context;
    // In demo mode optional columns start hidden — the demo.tape scenario uses [C]
    // to open the column picker and enable them live, showcasing the feature.
    let demo_config = AppConfig {
        visible_columns: {
            let mut vc = VisibleColumns::default();
            vc.set_visible("activity", true);
            vc.set_visible("milestone", true);
            vc.set_visible("notes", true);
            vc
        },
        ..config
    };

    // Demo mode always uses the dark palette — no terminal query needed.
    let demo_palette = crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark);
    let demo_project = crate::storage::ProjectEntry {
        name: Some("Demo".into()),
        gitlab_url: "https://gitlab.com".into(),
        project_id: "123456".into(),
        active: true,
        refresh_interval_secs: Some(900),
        activity_stale_days: Some(7),
        activity_recent_days: Some(2),
        show_cockpit: Some(true),
        ..Default::default()
    };

    let mut app = App::new(AppInit {
        token: std::sync::Arc::new("demo-token".to_string().into()),
        project_id: "123456".into(),
        base_url: "https://gitlab.com".into(),
        project_name: None,
        refresh_interval_secs: 900,
        config: demo_config,
        theme: demo_palette,
        project_settings: demo_project,
    });

    app.branches = vec!["main".into(), "staging".into(), "production".into()];
    app.mrs = vec![
        // MR 104 is placed first so it is selected on startup for the Inspector scroll demo.
        // The sort demo in demo.tape will reorder the list afterwards.
        TrackedMr {
            id: "104".into(),
            status: MrStatus::MergedIn(
                ["main".into(), "staging".into(), "production".into()]
                    .into_iter()
                    .collect(),
            ),
            mergeability: MergeabilityStatus::NotApproved,
            recently_updated: false,
            flagged: true,
            // Demo: simulate a linked Redmine ticket with full metadata for the Tracker pane.
            linked_ticket: Some(LinkedTicket {
                schema_version: LINKED_TICKET_SCHEMA_VERSION,
                id: "4271".into(),
                subject: "Expose MR metadata via GraphQL — server-side implementation".into(),
                status: "In Progress".into(),
                url: "https://redmine.example.com/issues/4271".into(),
                author: Some("Marina Graphetti".into()),
                assignee: Some("Thomas Dubosc".into()),
                tracker_type: Some("Evolution".into()),
                priority: Some("High".into()),
                version: Some("v2.4.0".into()),
                start_date: Some("2024-04-15".into()),
                done_ratio: Some(65),
                time_estimate: Some(28800),   // 8 h
                time_spent: Some(18000),       // 5 h
                time_remaining: Some(10800),   // 3 h
            }),
            data: crate::models::MrData {
                title: "feat(api): Introduce GraphQL endpoint for MR metadata".into(),
                state: GitlabMrState::Merged,
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
                assignee: Some("Thomas Dubosc (@thomas_db)".into()),
                reviewers: vec![
                    "Thomas Dubosc (@thomas_db)".into(),
                    "Alex Devries (@alex_dev)".into(),
                ],
                milestone: Some("v2.4.0".into()),
                milestone_due_date: Some("2024-05-31".into()),
                milestone_description: Some("Demo milestone for the v2.4.0 release train.".into()),
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
                // Demo: simulate a MR with several comments awaiting review.
                user_notes_count: 5,
                diff_stats: Some(crate::models::DiffStats { files_changed: 12, additions: 487, deletions: 53, commits_count: 8, commits_behind: Some(3) }),
                created_at: Some("2024-04-20T08:00:00Z".into()),
            },
        },
        TrackedMr {
            id: "101".into(),
            status: MrStatus::MergedIn(["main".into(), "staging".into()].into_iter().collect()),
            mergeability: MergeabilityStatus::Unknown,
            recently_updated: false,
            flagged: false,
            linked_ticket: None,
            data: crate::models::MrData {
                title: "feat(auth): Add OAuth2 PKCE flow for mobile clients".into(),
                state: GitlabMrState::Merged,
                sha: Some("a1b2c3d4".into()),
                description: "Implemented PKCE challenge and verification flow.".into(),
                author: "Alex Devries (@alex_dev)".into(),
                assignee: Some("Sarah Codewyn (@sarah_code)".into()),
                reviewers: vec![],
                milestone: Some("v2.4.0".into()),
                milestone_due_date: Some("2024-05-31".into()),
                milestone_description: Some("Demo milestone for the v2.4.0 release train.".into()),
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
                user_notes_count: 0,
                diff_stats: Some(crate::models::DiffStats { files_changed: 4, additions: 89, deletions: 12, commits_count: 3, commits_behind: None }),
                created_at: Some("2024-04-25T10:00:00Z".into()),
            },
        },
        TrackedMr {
            id: "102".into(),
            status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
            // Demo: simulate a MR with merge conflicts.
            mergeability: MergeabilityStatus::Conflict,
            recently_updated: false,
            flagged: false,
            // Demo: simulate a linked Redmine bug ticket with partial time tracking.
            linked_ticket: Some(LinkedTicket {
                schema_version: LINKED_TICKET_SCHEMA_VERSION,
                id: "3987".into(),
                subject: "Connection pool deadlocks under sustained high load".into(),
                status: "Assigned".into(),
                url: "https://redmine.example.com/issues/3987".into(),
                author: Some("Thomas Dubosc".into()),
                assignee: Some("Alex Devries".into()),
                tracker_type: Some("Bug".into()),
                priority: Some("Low".into()),
                version: Some("v2.4.0".into()),
                start_date: Some("2024-04-28".into()),
                done_ratio: Some(30),
                time_estimate: Some(14400),  // 4 h
                time_spent: Some(5400),       // 1 h 30
                time_remaining: Some(9000),   // 2 h 30
            }),
            data: crate::models::MrData {
                title: "fix(db): Resolve connection pool deadlocks under heavy load".into(),
                state: GitlabMrState::Opened,
                sha: Some("e5f6g7h8".into()),
                description: "Adjusted max pool size and statement timeout.".into(),
                author: "Thomas Dubosc (@thomas_db)".into(),
                assignee: Some("Alex Devries (@alex_dev)".into()),
                reviewers: vec!["Sarah Codewyn (@sarah_code)".into()],
                milestone: Some("v2.4.0".into()),
                milestone_due_date: Some("2024-05-31".into()),
                milestone_description: Some("Demo milestone for the v2.4.0 release train.".into()),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/102".into(),
                labels: vec!["bug".into(), "deploy::prod_pending".into()],
                updated_at: Some(demo_updated_at(14)),
                source_branch: "fix/db-pool-deadlock".into(),
                target_branch: "main".into(),
                merged_by: None,
                merged_at: None,
                pipelines: vec![],
                // Demo: simulate a MR with unread comments from reviewers.
                user_notes_count: 3,
                diff_stats: Some(crate::models::DiffStats { files_changed: 6, additions: 231, deletions: 18, commits_count: 5, commits_behind: Some(7) }),
                created_at: Some("2024-04-28T14:30:00Z".into()),
            },
        },
        TrackedMr {
            id: "103".into(),
            status: MrStatus::Loading,
            // Demo: simulate a MR that needs a rebase.
            mergeability: MergeabilityStatus::NeedsRebase,
            recently_updated: false,
            flagged: false,
            linked_ticket: None,
            data: crate::models::MrData {
                title: "refactor(ui): Optimize Ratatui render loop with double buffering".into(),
                state: GitlabMrState::Opened,
                sha: None,
                description: "Reducing CPU usage during high-frequency ticks.".into(),
                author: "Julien Morel (@julien_m)".into(),
                assignee: Some("Julien Morel (@julien_m)".into()),
                reviewers: vec![],
                milestone: Some("v2.5.0".into()),
                milestone_due_date: None,
                milestone_description: Some("Demo milestone for the v2.5.0 release train.".into()),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/103".into(),
                labels: vec!["performance".into(), "review::needs_work".into()],
                updated_at: Some(demo_updated_at(1)),
                source_branch: "refactor/ui-double-buffer".into(),
                target_branch: "develop".into(),
                merged_by: None,
                merged_at: None,
                pipelines: vec![],
                user_notes_count: 0,
                diff_stats: None,
                created_at: None,
            },
        },
        TrackedMr {
            id: "105".into(),
            status: MrStatus::MergedIn(["main".into()].into_iter().collect()),
            mergeability: MergeabilityStatus::DiscussionsNotResolved,
            recently_updated: false,
            flagged: false,
            linked_ticket: None,
            data: crate::models::MrData {
                title: "fix(ci): Repair flaky integration tests in pipeline stage 3".into(),
                state: GitlabMrState::Closed,
                sha: Some("3a4b5c6d".into()),
                description: "Isolated timing-dependent assertions and added retry logic.".into(),
                author: "Sarah Codewyn (@sarah_code)".into(),
                assignee: Some("Alex Devries (@alex_dev)".into()),
                reviewers: vec![],
                milestone: Some("v2.4.0".into()),
                milestone_due_date: Some("2024-05-31".into()),
                milestone_description: Some("Demo milestone for the v2.4.0 release train.".into()),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/105".into(),
                labels: vec!["bug".into(), "review::approved".into(), "size::S".into()],
                updated_at: Some(demo_updated_at(30)),
                source_branch: "fix/ci-flaky-tests".into(),
                target_branch: "main".into(),
                merged_by: None,
                merged_at: None,
                pipelines: vec![],
                user_notes_count: 6,
                diff_stats: Some(crate::models::DiffStats { files_changed: 2, additions: 34, deletions: 8, commits_count: 1, commits_behind: Some(0) }),
                created_at: Some("2024-04-10T09:15:00Z".into()),
            },
        },
        TrackedMr {
            id: "106".into(),
            status: MrStatus::Error("GitLab API returned 502 Bad Gateway".into()),
            // Demo: simulate a cleanly mergeable MR.
            mergeability: MergeabilityStatus::Mergeable,
            recently_updated: false,
            flagged: false,
            linked_ticket: None,
            data: crate::models::MrData {
                title: "chore(deps): Bump tokio to 1.37 and update async ecosystem".into(),
                state: GitlabMrState::Opened,
                sha: Some("7e8f9a0b".into()),
                description: "Routine dependency upgrade; resolves two CVEs in hyper transitive deps."
                    .into(),
                author: "Bot Renovate (@bot_renovate)".into(),
                assignee: Some("Julien Morel (@julien_m)".into()),
                reviewers: vec![],
                milestone: Some("v2.5.0".into()),
                milestone_due_date: None,
                milestone_description: Some("Demo milestone for the v2.5.0 release train.".into()),
                web_url: "https://gitlab.com/demo/project/-/merge_requests/106".into(),
                labels: vec!["dependencies".into(), "review::needs_work".into()],
                updated_at: Some(demo_updated_at(5)),
                source_branch: "chore/bump-tokio-1.37".into(),
                target_branch: "develop".into(),
                merged_by: None,
                merged_at: None,
                pipelines: vec![],
                user_notes_count: 0,
                diff_stats: Some(crate::models::DiffStats { files_changed: 1, additions: 12, deletions: 12, commits_count: 2, commits_behind: Some(2) }),
                created_at: Some("2024-04-22T11:00:00Z".into()),
            },
        },
        TrackedMr {
            id: "107".into(),
            status: MrStatus::Loading,
            // Demo: simulate a cleanly mergeable MR.
            mergeability: MergeabilityStatus::RequestedChanges,
            recently_updated: false,
            flagged: true,
            linked_ticket: None,
            data: crate::models::MrData {
                title: "feat(notif): Add desktop notifications on branch status change".into(),
                state: GitlabMrState::Opened,
                sha: None,
                description: "Uses notify-rust to surface MR merge events as OS notifications.".into(),
                author: "Julien Morel (@julien_m)".into(),
                assignee: Some("Marina Graphetti (@marina_gql)".into()),
                reviewers: vec!["Thomas Dubosc (@thomas_db)".into()],
                milestone: Some("v2.5.0".into()),
                milestone_due_date: None,
                milestone_description: Some("Demo milestone for the v2.5.0 release train.".into()),
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
                // Demo: simulate a MR with one comment to address.
                user_notes_count: 1,
                diff_stats: None,
                created_at: Some("2024-04-30T16:45:00Z".into()),
            },
        },
    ];

    // Seed an in-memory SQLite stats DB so the Stats overlay renders real data
    // in demo mode instead of "Stats DB not available".
    #[cfg(feature = "stats")]
    {
        app.stats_db = seed_demo_stats_db("123456").await;
    }

    // Apply the default sort (UpdatedAt Descending) so the table order at startup
    // matches exactly what the user sees — MR 104 is given the most recent timestamp
    // so it lands at row 0 after sorting.
    app.sort_mrs();
    app.table_state.select(Some(0));
    // The demo runs the real key handler; this flag turns off its disk writes and
    // network requests.
    app.read_only = true;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    // Required by the key handler's signature; unused in read-only mode.
    let api_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let mut last_known_branches = std::collections::HashMap::new();
    let tx_timer = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let _ = tx_timer.send(AppEvent::Tick);
        }
    });

    // Enable mouse capture so VHS scroll simulation works in demo mode.
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
        .context("Failed to enable mouse capture")?;

    let mut terminal = ratatui::init();
    let _terminal_guard = crate::utils::TerminalGuard;

    loop {
        // Drain the event queue before rendering
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::Tick => {
                    if app.time_left > 0 {
                        app.time_left -= 1;
                    } else {
                        app.time_left = app.refresh_interval_secs;
                    }
                }
                // Route stats results into app state so the overlay re-renders.
                #[cfg(feature = "stats")]
                AppEvent::StatsReportLoaded { generation, result } => {
                    app.stats_view.apply_result(generation, result);
                }
                // Everything else (add / remove MR…) would mutate or fetch: the demo
                // is read-only, so those events are dropped.
                _ => {}
            }
        }

        terminal
            .draw(|f| ui::render_ui(f, &mut app))
            .context("Failed to draw the terminal")?;

        if event::poll(Duration::from_millis(50)).context("Failed to poll terminal input")? {
            match event::read().context("Failed to read terminal input")? {
                Event::Mouse(mouse) => {
                    handle_mouse_event(mouse, &mut app);
                }
                // Repeat too, so held keys scroll smoothly in recordings.
                Event::Key(key)
                    if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                {
                    let quit = handle_key_event(
                        key,
                        &mut app,
                        &api_semaphore,
                        &tx,
                        &mut last_known_branches,
                    )
                    .await;
                    if quit {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    // `_terminal_guard` disables mouse capture and restores the terminal on drop.
    Ok(())
}
