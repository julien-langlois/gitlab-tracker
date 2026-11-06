use crate::app::AppInit;
use crate::app::{App, TimeLogState};
use crate::config::{AppConfig, VisibleColumns};
use crate::events::{handle_key_event, handle_mouse_event};
use crate::models::{
    AppEvent, GitlabMrState, MergeabilityStatus, MrStatus, Pipeline, PipelineJob, PipelineState,
    TrackedMr,
};
use crate::ui;
use crossterm::event::{self, Event, KeyEventKind};
use gitlab_tracker_core::{
    Activity, LinkedTicket, TimeEntry, TrackerError, TrackerProvider, LINKED_TICKET_SCHEMA_VERSION,
};
use std::collections::HashMap;
use std::time::Duration;

// ── Demo tracker ──────────────────────────────────────────────────────────────

/// A Redmine stand-in for the demo: it names the tracker and builds ticket URLs,
/// so the tracker pane, the ticket filter and the log-time dialog show up. It
/// never fetches anything: the demo's tickets, activities and time entries are
/// already in the app (see [`demo_app`]), and the demo ignores fetch results.
pub(crate) struct DemoTracker;

#[async_trait::async_trait]
impl TrackerProvider for DemoTracker {
    fn name(&self) -> &'static str {
        "Redmine"
    }

    fn detect_ticket_id(&self, _title: &str, _description: &str) -> Option<String> {
        None
    }

    async fn fetch_ticket(&self, ticket_id: &str) -> Result<LinkedTicket, TrackerError> {
        Err(TrackerError::NotFound(ticket_id.to_string()))
    }

    fn ticket_url(&self, ticket_id: &str) -> String {
        format!("https://redmine.example.com/issues/{ticket_id}")
    }
}

/// The demo's statuses, so the window's board shows its columns. Moving a
/// ticket is refused: the demo is read-only (the window checks first).
#[cfg(feature = "gui")]
#[async_trait::async_trait]
impl gitlab_tracker_core::TicketTransitionProvider for DemoTracker {
    async fn fetch_transition_targets(
        &self,
    ) -> Result<Vec<gitlab_tracker_core::TicketTransitionTarget>, TrackerError> {
        Ok([
            (1, "New"),
            (2, "In Progress"),
            (3, "Assigned"),
            (4, "Feedback"),
            (5, "Resolved"),
        ]
        .into_iter()
        .map(|(id, label)| gitlab_tracker_core::TicketTransitionTarget {
            id: id.to_string(),
            label: label.into(),
        })
        .collect())
    }

    async fn transition_ticket_status(
        &self,
        ticket_id: &str,
        _target_id: &str,
    ) -> Result<(), TrackerError> {
        Err(TrackerError::NotFound(ticket_id.to_string()))
    }
}

/// The demo tracker's time-tracking activities.
fn demo_activities() -> Vec<Activity> {
    [(9, "Development"), (10, "Review"), (11, "Testing")]
        .into_iter()
        .map(|(id, name)| Activity {
            id,
            name: name.into(),
        })
        .collect()
}

/// Time entries of the demo's linked tickets, adding up to their `time_spent`.
fn demo_time_entries() -> HashMap<String, TimeLogState> {
    let activity = |i: usize| demo_activities().swap_remove(i);
    let entry = |id, hours, a, comment: &str, user: &str, spent_on: &str| TimeEntry {
        id,
        hours,
        activity: activity(a),
        comment: comment.into(),
        user: user.into(),
        spent_on: spent_on.into(),
    };
    HashMap::from([
        (
            "4271".to_string(),
            TimeLogState::Loaded(vec![
                entry(
                    501,
                    3.0,
                    0,
                    "GraphQL schema and resolvers",
                    "Marina Graphetti",
                    "2024-04-16",
                ),
                entry(502, 1.5, 0, "Pagination", "Marina Graphetti", "2024-04-18"),
                entry(503, 0.5, 1, "", "Thomas Dubosc", "2024-04-19"),
            ]),
        ),
        (
            "3987".to_string(),
            TimeLogState::Loaded(vec![entry(
                504,
                1.5,
                2,
                "Reproduced under load",
                "Alex Devries",
                "2024-04-29",
            )]),
        ),
    ])
}

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

    // Promotion lead time: every MR reaches staging a few hours after its merge
    // on main, the ones older than a week reach production too; one in three is
    // a copy (cherry-pick) rather than a merge.
    for (i, mr) in dataset.iter().enumerate() {
        use gitlab_tracker_stats::{ArrivalKind, BranchArrival};
        let merged = chrono::Utc::now() - chrono::Duration::days(mr.merged_days_ago);
        let staging_delay = chrono::Duration::hours(2 + (i as i64 * 7) % 30);
        let production_delay = chrono::Duration::hours(30 + (i as i64 * 13) % 120);
        let mut arrivals = vec![("staging", staging_delay)];
        if mr.merged_days_ago > 7 {
            arrivals.push(("production", production_delay));
        }
        for (branch, delay) in arrivals {
            let arrival = BranchArrival {
                project_id: project_id.to_string(),
                mr_id: mr.id.to_string(),
                branch: branch.to_string(),
                arrived_at: merged + delay,
                how: if i % 3 == 0 {
                    ArrivalKind::Copy
                } else {
                    ArrivalKind::Merge
                },
                merged_at: Some(merged.to_rfc3339()),
            };
            let _ = db.record_branch_arrival(&arrival).await;
        }
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

/// Review timelines, unresolved threads and branch arrivals for the demo MRs,
/// dated from now: what the discussions and the stats DB give a real project.
fn demo_review_activity(app: &mut App) {
    use crate::models::{MrThread, ReviewActivity};
    let now = chrono::Utc::now();
    let at = |days: f64| {
        (now - chrono::Duration::minutes((days * 1440.0) as i64))
            .format("%Y-%m-%dT%H:%M:%S.000Z")
            .to_string()
    };
    let thread =
        |id: u64, author: &str, excerpt: &str, opened: f64, last: f64, notes: u32| MrThread {
            note_id: id,
            author: author.into(),
            excerpt: excerpt.into(),
            created_at: at(opened),
            last_note_at: at(last),
            notes,
            resolved: false,
        };
    for mr in &mut app.mrs {
        match mr.id.as_str() {
            "104" => {
                mr.review = Some(ReviewActivity {
                    first_review_at: Some("2024-04-20T20:00:00Z".into()),
                    approved_at: Some("2024-05-03T10:00:00Z".into()),
                    review_requested_at: [
                        ("thomas_db".to_string(), "2024-04-20T09:00:00Z".to_string()),
                        ("alex_dev".to_string(), "2024-04-20T09:00:00Z".to_string()),
                    ]
                    .into(),
                    last_push_at: Some("2024-05-02T16:00:00Z".into()),
                    last_activity_by: [
                        ("thomas_db".to_string(), "2024-05-03T10:00:00Z".to_string()),
                        ("alex_dev".to_string(), "2024-04-22T11:00:00Z".to_string()),
                    ]
                    .into(),
                    ..Default::default()
                });
            }
            "102" => {
                mr.threads = vec![
                    thread(
                        9_102_001,
                        "alex_dev",
                        "Shouldn't the timeout be configurable? 30 s will still block the worker under load.",
                        16.0,
                        3.0,
                        4,
                    ),
                    thread(
                        9_102_002,
                        "sarah_code",
                        "This index is created without CONCURRENTLY: it will lock the table on production.",
                        12.0,
                        12.0,
                        2,
                    ),
                    thread(
                        9_102_003,
                        "sarah_code",
                        "Nit: the pool size could come from the settings.",
                        12.0,
                        11.0,
                        3,
                    ),
                ];
                mr.review = Some(ReviewActivity {
                    first_review_at: Some(at(16.0)),
                    approved_at: None,
                    review_requested_at: [("sarah_code".to_string(), at(17.0))].into(),
                    last_push_at: Some(at(13.0)),
                    last_activity_by: [
                        ("alex_dev".to_string(), at(3.0)),
                        ("sarah_code".to_string(), at(11.0)),
                        ("thomas_db".to_string(), at(11.0)),
                    ]
                    .into(),
                    ..Default::default()
                });
                mr.created_at = Some(at(20.0));
            }
            "107" => {
                mr.threads = vec![thread(
                    9_107_001,
                    "thomas_db",
                    "The notification fires twice when two branches change in the same refresh.",
                    6.0,
                    4.0,
                    2,
                )];
                mr.review = Some(ReviewActivity {
                    first_review_at: Some(at(6.0)),
                    approved_at: None,
                    review_requested_at: [("thomas_db".to_string(), at(7.0))].into(),
                    last_push_at: Some(at(2.0)),
                    last_activity_by: [
                        ("thomas_db".to_string(), at(4.0)),
                        ("julien_m".to_string(), at(2.0)),
                    ]
                    .into(),
                    ..Default::default()
                });
                mr.created_at = Some(at(8.0));
            }
            _ => {}
        }
    }
    // What the last startup found changed since the session before.
    app.last_changes = vec![
        crate::app::SessionChange {
            id: "101".into(),
            title: "feat(auth): Add OAuth2 PKCE flow for mobile clients".into(),
            kind: crate::app::ChangeKind::Arrived("staging".into()),
        },
        crate::app::SessionChange {
            id: "106".into(),
            title: "chore(deps): Bump tokio to 1.37 and update async ecosystem".into(),
            kind: crate::app::ChangeKind::CiFailing,
        },
    ];
    // !101 went through review and merge last week, and sits on staging.
    if let Some(mr) = app.mrs.iter_mut().find(|m| m.id == "101") {
        mr.created_at = Some(at(11.0));
        mr.merged_at = Some(at(6.0));
        mr.review = Some(ReviewActivity {
            first_review_at: Some(at(10.2)),
            approved_at: Some(at(7.0)),
            ..Default::default()
        });
    }
    app.branch_arrivals.insert(
        ("101".into(), "staging".into()),
        now - chrono::Duration::days(4),
    );
    let utc = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or(now)
    };
    app.branch_arrivals.insert(
        ("104".into(), "staging".into()),
        utc("2024-05-08T09:00:00Z"),
    );
    app.branch_arrivals.insert(
        ("104".into(), "production".into()),
        utc("2024-05-12T09:00:00Z"),
    );
}

/// Runs the application in demo mode with pre-populated mock data.
/// This mode is intended for screenshots, testing, and demonstrations.
/// The demo project: mock MRs, branches and tickets, read-only (no disk writes,
/// no network). Shared by the terminal demo and the window demo.
pub async fn demo_app(config: AppConfig) -> App {
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: vec!["thomas_db".into(), "sarah_code".into()],
                approvers: vec![
                    "Thomas Dubosc (@thomas_db)".into(),
                    "Sarah Codewyn (@sarah_code)".into(),
                ],
                title: "feat(api): Introduce GraphQL endpoint for MR metadata".into(),
                state: GitlabMrState::Merged,
                sha: Some("c9d0e1f2".into()),
                // Pre-populate mock pipelines for the demo so [P] shows data instantly.
                pipelines: vec![
                    Pipeline {
                        id: 9981,
                        status: PipelineState::Success,
                        created_at: Some("2024-11-14T10:23:05Z".into()),
                        updated_at: Some("2024-11-14T10:35:40Z".into()),
                        sha: Some("c9d0e1f2".into()),
                        jobs: vec![
                            PipelineJob { id: 0, name: "lint".into(),           stage: "test".into(),   status: "success".into(), duration: Some(18.0), queued_duration: None },
                            PipelineJob { id: 0, name: "unit-tests".into(),     stage: "test".into(),   status: "success".into(), duration: Some(74.0), queued_duration: None },
                            PipelineJob { id: 0, name: "build".into(),          stage: "build".into(),  status: "success".into(), duration: Some(42.0), queued_duration: None },
                            PipelineJob { id: 0, name: "deploy-staging".into(), stage: "deploy".into(), status: "success".into(), duration: Some(31.0), queued_duration: None },
                        ],
                    },
                    Pipeline {
                        id: 9942,
                        status: PipelineState::Failed,
                        created_at: Some("2024-11-13T17:08:42Z".into()),
                        updated_at: Some("2024-11-13T17:19:02Z".into()),
                        sha: Some("b81f2a07".into()),
                        jobs: vec![
                            PipelineJob { id: 0, name: "lint".into(),           stage: "test".into(),   status: "success".into(), duration: Some(17.0), queued_duration: None },
                            PipelineJob { id: 0, name: "unit-tests".into(),     stage: "test".into(),   status: "failed".into(),  duration: Some(61.0), queued_duration: None },
                            PipelineJob { id: 0, name: "build".into(),          stage: "build".into(),  status: "skipped".into(), duration: None, queued_duration: None },
                            PipelineJob { id: 0, name: "deploy-staging".into(), stage: "deploy".into(), status: "skipped".into(), duration: None, queued_duration: None },
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
                diff_stats: Some(crate::models::DiffStats { files_changed: 12, additions: 487, deletions: 53, commits_count: 8, commits_behind: Some(3), ..Default::default() }),
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: Vec::new(),
                approvers: Vec::new(),
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
                diff_stats: Some(crate::models::DiffStats { files_changed: 4, additions: 89, deletions: 12, commits_count: 3, commits_behind: None, ..Default::default() }),
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: Vec::new(),
                approvers: Vec::new(),
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
                diff_stats: Some(crate::models::DiffStats { files_changed: 6, additions: 231, deletions: 18, commits_count: 5, commits_behind: Some(7), ..Default::default() }),
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: Vec::new(),
                approvers: Vec::new(),
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: Vec::new(),
                approvers: Vec::new(),
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
                diff_stats: Some(crate::models::DiffStats { files_changed: 2, additions: 34, deletions: 8, commits_count: 1, commits_behind: Some(0), ..Default::default() }),
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: Vec::new(),
                approvers: Vec::new(),
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
                diff_stats: Some(crate::models::DiffStats { files_changed: 1, additions: 12, deletions: 12, commits_count: 2, commits_behind: Some(2), ..Default::default() }),
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
                commits: Vec::new(),
                landed_fingerprint: None,
                threads: Vec::new(),
                review: None,
                approved_by: Vec::new(),
                approvers: Vec::new(),
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
    // The review timeline, threads and branch arrivals the refreshes would read,
    // so the window can tell who each MR waits on and since when.
    demo_review_activity(&mut app);

    app.sort_mrs();
    app.table_state.select(Some(0));
    // The tracker's data is loaded up front: the demo drops fetch results.
    app.tracker = Some(std::sync::Arc::new(DemoTracker));
    app.activities = demo_activities();
    app.time_entries = demo_time_entries();
    // The demo runs the real key handler; this flag turns off its disk writes and
    // network requests.
    app.read_only = true;
    app
}

/// Applies `event` to the demo app: only the clock and the stats results, since
/// every other event (add / remove MR…) would mutate or fetch.
pub fn apply_demo_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::Tick => {
            app.tick_flash();
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
        #[cfg(feature = "stats")]
        AppEvent::StatsCockpitLoaded { generation, result } => {
            app.stats_view.apply_cockpit_result(generation, result);
        }
        _ => {}
    }
}

pub async fn run_demo_mode(config: AppConfig) -> anyhow::Result<()> {
    use anyhow::Context;

    let mut app = demo_app(config).await;

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
            apply_demo_event(&mut app, event);
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

/// The changed files the demo's inspector shows for any MR (`gui` feature).
#[cfg(feature = "gui")]
pub fn demo_file_diffs() -> Vec<crate::gitlab::FileDiff> {
    let file = |path: &str, new_file: bool, diff: &str| crate::gitlab::FileDiff {
        old_path: path.into(),
        new_path: path.into(),
        new_file,
        renamed_file: false,
        deleted_file: false,
        diff: diff.into(),
    };
    vec![
        file(
            "src/api/graphql/schema.rs",
            false,
            "@@ -12,7 +12,12 @@ use crate::models::MergeRequest;\n pub struct QueryRoot;\n \n #[Object]\n impl QueryRoot {\n-    async fn merge_request(&self, iid: u64) -> Option<MergeRequest> {\n-        MergeRequest::find(iid).await\n+    /// One MR with its metadata: labels, pipelines, approvals.\n+    async fn merge_request(&self, ctx: &Context<'_>, iid: u64) -> Result<Option<MergeRequest>> {\n+        let db = ctx.data::<Db>()?;\n+        Ok(MergeRequest::find(db, iid).await?)\n+    }\n+\n+    async fn merge_requests(&self, ctx: &Context<'_>, state: Option<State>) -> Result<Vec<MergeRequest>> {\n+        Ok(MergeRequest::list(ctx.data::<Db>()?, state).await?)\n     }\n }",
        ),
        file(
            "src/api/graphql/mod.rs",
            true,
            "@@ -0,0 +1,4 @@\n+mod schema;\n+\n+pub use schema::QueryRoot;\n+pub type Schema = async_graphql::Schema<QueryRoot, EmptyMutation, EmptySubscription>;",
        ),
        file(
            "Cargo.toml",
            false,
            "@@ -18,6 +18,7 @@ axum = \"0.7\"\n serde = { version = \"1\", features = [\"derive\"] }\n tokio = { version = \"1\", features = [\"full\"] }\n+async-graphql = \"7\"\n tracing = \"0.1\"",
        ),
    ]
}

/// The end of a failed job's log, for the demo's inspector (`gui` feature).
#[cfg(feature = "gui")]
pub fn demo_job_log() -> String {
    crate::gitlab::log_tail(
        "$ cargo test --workspace\n   Compiling api v0.4.0 (/builds/shop/api)\n    Finished test profile in 41.2s\n     Running unittests src/lib.rs\n\nrunning 48 tests\ntest db::pool::tests::acquire_times_out ... ok\ntest db::pool::tests::release_under_load ... FAILED\ntest api::graphql::tests::lists_open_mrs ... ok\n\nfailures:\n\n---- db::pool::tests::release_under_load stdout ----\nthread 'db::pool::tests::release_under_load' panicked at src/db/pool.rs:212:9:\nassertion `left == right` failed: 3 connections still checked out\n  left: 3\n right: 0\n\ntest result: FAILED. 47 passed; 1 failed; 0 ignored\nerror: test failed, to rerun pass `--lib`\nERROR: Job failed: exit code 101",
        60,
    )
}

/// What the demo's environments and CI pages read "from GitLab" (`gui`
/// feature): two weeks of deployments of an environment per tracked branch, and
/// each branch's latest pipeline, dated from now.
#[cfg(feature = "gui")]
pub fn demo_project_overview(app: &App) -> crate::gitlab_detail::ProjectOverview {
    use crate::gitlab_detail::{BranchPipeline, DeploymentRecord, ProjectOverview};
    let now = chrono::Utc::now();
    let at = |hours: f64| {
        (now - chrono::Duration::minutes((hours * 60.0) as i64))
            .format("%Y-%m-%dT%H:%M:%S.000Z")
            .to_string()
    };
    let flow = app.branch_flow();
    let ordered: Vec<String> = flow.in_order().iter().map(|b| b.name.clone()).collect();
    let last = ordered.len().saturating_sub(1);
    let mut deployments = Vec::new();
    let mut pipelines = Vec::new();
    let people = [
        "Alex Devries",
        "Thomas Dubosc",
        "Sarah Codewyn",
        "GitLab CI",
    ];
    for (index, branch) in ordered.iter().enumerate() {
        let environment = match crate::branch_flow::short_name(branch).as_str() {
            "prod" => "prod".to_string(),
            "staging" => "recette".to_string(),
            "main" => "dev".to_string(),
            other => other.to_string(),
        };
        // The closer to production, the rarer: dev twice a day, staging every
        // other day, production every two or three days.
        let every = match last - index {
            0 => 60.0,
            1 => 44.0,
            _ => 11.0,
        };
        let mut hours = 0.7 + index as f64 * 6.0;
        let mut n = 0;
        while hours < 35.0 * 24.0 {
            let status = if n % 9 == 4 { "failed" } else { "success" };
            deployments.push(DeploymentRecord {
                environment: environment.clone(),
                reference: branch.clone(),
                sha: format!("{:07x}", 0x3a91c0e + (index * 977 + n * 131) as u64),
                created_at: at(hours),
                status: status.into(),
                user: people[(index + n) % people.len()].into(),
            });
            // A failed production deployment, three hours before the last one
            // that fixed it: an incident for the delivery page.
            if n == 0 && index == last && last > 0 {
                deployments.push(DeploymentRecord {
                    environment: environment.clone(),
                    reference: branch.clone(),
                    sha: "7c19f03".into(),
                    created_at: at(hours + 3.0),
                    status: "failed".into(),
                    user: people[1].into(),
                });
            }
            hours += every * (0.8 + 0.4 * ((n * 7 % 5) as f64 / 4.0));
            n += 1;
        }
        let failed = index + 1 == last && last > 0;
        pipelines.push(BranchPipeline {
            branch: branch.clone(),
            id: 9990 - index as u64 * 4,
            sha: if failed {
                "7c19f03".into()
            } else {
                format!("{:07x}", 0x3a91c0e + (index * 977) as u64)
            },
            status: if failed {
                PipelineState::Failed
            } else {
                PipelineState::Success
            },
            created_at: at(0.9 + index as f64 * 30.0),
            updated_at: at(0.7 + index as f64 * 30.0),
            web_url: String::new(),
            failed_jobs: if failed {
                vec!["deploy-staging".into()]
            } else {
                Vec::new()
            },
        });
    }
    deployments.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    ProjectOverview {
        deployments,
        pipelines,
        merges: crate::gitlab_detail::MergeHistory {
            merged: app
                .mrs
                .iter()
                .filter_map(|mr| Some((mr.id.parse().ok()?, mr.merged_at.clone()?)))
                .collect(),
            read_at: Some(now.to_rfc3339()),
        },
    }
}

/// What the window's demo adds to [`demo_app`] for its project pages (`gui`
/// feature): two weeks of pipelines per MR, with a flaky job, the files each MR
/// changes (two open MRs on the same lines, others on the same files), and
/// answered review requests. Dated from now; the TUI demo and the tests keep
/// [`demo_app`] as it is.
#[cfg(feature = "gui")]
pub fn enrich_for_window(app: &mut App) {
    use crate::models::{ChangedFile, ReviewActivity};
    let now = chrono::Utc::now();
    let at = |hours: f64| {
        (now - chrono::Duration::minutes((hours * 60.0) as i64))
            .format("%Y-%m-%dT%H:%M:%S.000Z")
            .to_string()
    };
    // name, stage, usual seconds
    const JOBS: [(&str, &str, f64); 6] = [
        ("lint", "test", 46.0),
        ("unit-tests", "test", 252.0),
        ("integration-tests", "test", 305.0),
        ("e2e-cypress", "test", 460.0),
        ("build-image", "build", 198.0),
        ("deploy-staging", "deploy", 151.0),
    ];
    // Per MR: pipelines (hours ago, failing jobs), newest first; same sha twice
    // with a failure then a pass is a flaky run.
    // (hours ago, failing jobs, revision of the source branch)
    type Run = (f64, &'static [&'static str], u32);
    let plan: &[(&str, &[Run])] = &[
        (
            "107",
            &[
                (5.0, &["unit-tests"], 7),
                (30.0, &["unit-tests"], 6),
                (52.0, &[], 5),
                (98.0, &["e2e-cypress"], 5),
                (170.0, &[], 4),
            ],
        ),
        (
            "102",
            &[
                (9.0, &[], 3),
                (11.0, &["e2e-cypress"], 3),
                (75.0, &["integration-tests"], 2),
                (77.0, &[], 2),
                (200.0, &["lint"], 1),
            ],
        ),
        (
            "103",
            &[
                (20.0, &[], 4),
                (23.0, &["e2e-cypress"], 4),
                (130.0, &[], 3),
                (220.0, &["e2e-cypress"], 2),
            ],
        ),
        (
            "106",
            &[
                (40.0, &[], 2),
                (150.0, &["deploy-staging"], 1),
                (152.0, &[], 1),
            ],
        ),
        (
            "101",
            &[
                (150.0, &[], 3),
                (190.0, &["e2e-cypress"], 2),
                (192.0, &[], 2),
                (260.0, &["unit-tests"], 1),
            ],
        ),
        (
            "105",
            &[
                (280.0, &["integration-tests"], 2),
                (300.0, &["integration-tests", "e2e-cypress"], 1),
            ],
        ),
    ];
    for mr in &mut app.mrs {
        let Some((_, runs)) = plan.iter().find(|(id, _)| *id == mr.id) else {
            continue;
        };
        if !mr.pipelines.is_empty() {
            continue;
        }
        let base: u64 = mr.id.parse().unwrap_or(1) * 97;
        mr.pipelines = runs
            .iter()
            .enumerate()
            .map(|(k, (hours, failing, rev))| {
                let jitter = |j: usize| 0.82 + ((base as usize + k * 7 + j * 3) % 9) as f64 * 0.045;
                let mut stop = false;
                let jobs: Vec<PipelineJob> = JOBS
                    .iter()
                    .enumerate()
                    .map(|(j, (name, stage, secs))| {
                        let failed = failing.contains(name);
                        let status = if stop {
                            "skipped"
                        } else if failed {
                            "failed"
                        } else {
                            "success"
                        };
                        // Nothing deploys after a failed stage.
                        if failed && *stage != "deploy" {
                            stop = *stage == "build";
                        }
                        PipelineJob {
                            id: 70_000 + base * 10 + (k * 10 + j) as u64,
                            name: (*name).into(),
                            stage: (*stage).into(),
                            status: status.into(),
                            duration: (status != "skipped").then(|| (secs * jitter(j)).round()),
                            // From a few seconds to over a minute waiting for a runner.
                            queued_duration: (status != "skipped").then(|| {
                                (8.0 + ((base as usize + k * 13 + j * 5) % 11) as f64 * 6.5).round()
                            }),
                        }
                    })
                    .collect();
                // Wall time: the jobs of a stage run side by side, some runners are slower.
                let spread = 0.38 + 0.07 * ((k * 5 + base as usize) % 7) as f64;
                let total: f64 = jobs.iter().filter_map(|j| j.duration).sum::<f64>() * spread;
                Pipeline {
                    id: 9_700 + base / 3 + (runs.len() - k) as u64 * 7,
                    status: if failing.is_empty() {
                        PipelineState::Success
                    } else {
                        PipelineState::Failed
                    },
                    created_at: Some(at(*hours)),
                    updated_at: Some(at(hours - total / 3600.0)),
                    sha: Some(format!(
                        "{:08x}",
                        0x5e1a_0000_u64 + base * 16 + u64::from(*rev)
                    )),
                    jobs,
                }
            })
            .collect();
    }

    // The files each MR changes.
    let file = |path: &str, hunks: &[(u32, u32)], added: u32, deleted: u32| ChangedFile {
        path: path.into(),
        added,
        deleted,
        hunks: hunks.to_vec(),
        ..ChangedFile::default()
    };
    for mr in &mut app.mrs {
        let files = match mr.id.as_str() {
            "103" => vec![
                file("src/ui/render.rs", &[(12, 90)], 141, 62),
                file("src/ui/table.rs", &[(300, 340)], 38, 21),
                file("src/app.rs", &[(1200, 1262)], 44, 30),
                file("src/api/graphql/schema.rs", &[(118, 131)], 9, 4),
            ],
            "107" => vec![
                file("src/notify.rs", &[(1, 212)], 188, 12),
                file("src/app.rs", &[(1240, 1290)], 52, 8),
                file("config/default.toml", &[(30, 34)], 6, 0),
                file("src/api/graphql/schema.rs", &[(124, 129)], 11, 2),
            ],
            "102" => vec![
                file("src/db/pool.rs", &[(180, 240)], 61, 39),
                file("src/app.rs", &[(400, 410)], 7, 3),
                ChangedFile {
                    new_file: true,
                    ..file("migrations/0042_pool_index.sql", &[(1, 1)], 12, 0)
                },
            ],
            "106" => vec![
                file("Cargo.toml", &[(20, 48)], 14, 14),
                file("Cargo.lock", &[(1, 900)], 310, 286),
                file("src/db/pool.rs", &[(247, 251)], 3, 3),
            ],
            "101" => vec![
                file("src/auth/oauth.rs", &[(1, 160)], 151, 20),
                file("config/default.toml", &[(12, 18)], 7, 1),
                file("src/app.rs", &[(880, 905)], 22, 4),
            ],
            "104" => vec![
                file("src/api/graphql/schema.rs", &[(1, 140)], 230, 10),
                file("src/app.rs", &[(1210, 1230)], 18, 6),
                file("src/db/pool.rs", &[(60, 75)], 9, 2),
            ],
            _ => continue,
        };
        if let Some(stats) = &mut mr.diff_stats {
            stats.files = files;
            stats.files_listed = true;
        } else {
            mr.diff_stats = Some(crate::models::DiffStats {
                files_changed: files.len() as u32,
                additions: files.iter().map(|f| f.added).sum(),
                deletions: files.iter().map(|f| f.deleted).sum(),
                commits_count: 1,
                files,
                files_listed: true,
                ..Default::default()
            });
        }
    }

    // Milestones due soon, so that the page has one to make and one to watch.
    let due = |days: i64| {
        (now + chrono::Duration::days(days))
            .format("%Y-%m-%d")
            .to_string()
    };
    app.milestones = vec![
        crate::models::GitLabMilestone {
            title: "v2.4.0".into(),
            due_date: Some(due(6)),
            description: Some("November release train.".into()),
        },
        crate::models::GitLabMilestone {
            title: "v2.5.0".into(),
            due_date: Some(due(27)),
            description: Some("Mobile clients and notifications.".into()),
        },
    ];
    for mr in &mut app.mrs {
        match mr.milestone.as_deref() {
            Some("v2.4.0") => mr.milestone_due_date = Some(due(6)),
            Some("v2.5.0") => mr.milestone_due_date = Some(due(27)),
            _ => {}
        }
    }

    // A recent arrival on production, for the milestones' pace.
    if app.mrs.iter().any(|mr| mr.id == "104") {
        if let Some(last) = app.branches.last().cloned() {
            app.branch_arrivals
                .insert(("104".into(), last), now - chrono::Duration::days(9));
        }
    }

    // Answered review requests, for the response times.
    for mr in &mut app.mrs {
        let id = mr.id.clone();
        let review = mr.review.get_or_insert_with(ReviewActivity::default);
        let mut ask = |who: &str, asked: f64, answered: Option<f64>| {
            review
                .review_requested_at
                .entry(who.to_string())
                .or_insert_with(|| at(asked));
            if let Some(answered) = answered {
                review
                    .last_activity_by
                    .entry(who.to_string())
                    .or_insert_with(|| at(answered));
            }
        };
        match id.as_str() {
            "101" => {
                ask("thomas_db", 220.0, Some(214.0));
                ask("sarah_code", 220.0, Some(196.0));
            }
            "103" => ask("marina_gql", 60.0, Some(49.0)),
            "106" => ask("julien_m", 46.0, Some(44.5)),
            "102" => ask("alex_dev", 30.0, Some(21.0)),
            _ => {}
        }
    }
    // The open MRs opened weeks ago, not years: their age means something.
    for (id, days) in [("103", 12.0), ("106", 26.0)] {
        if let Some(mr) = app.mrs.iter_mut().find(|m| m.id == id) {
            mr.data.created_at = Some(at(days * 24.0));
        }
    }
    demo_history(app, now);
    demo_stacks(app, now);
}

#[cfg(feature = "gui")]
/// The people of the demo: display string, login.
const DEMO_PEOPLE: [(&str, &str); 5] = [
    ("Thomas Dubosc (@thomas_db)", "thomas_db"),
    ("Alex Devries (@alex_dev)", "alex_dev"),
    ("Sarah Codewyn (@sarah_code)", "sarah_code"),
    ("Julien Morel (@julien_m)", "julien_m"),
    ("Marina Graphetti (@marina_gql)", "marina_gql"),
];

#[cfg(feature = "gui")]
/// A demo MR with its basics; the caller fills in the rest.
fn demo_mr(id: &str, title: &str, author: usize, source: &str, target: &str) -> TrackedMr {
    let mut mr = TrackedMr::placeholder(id.into(), title.into(), None);
    mr.data = crate::models::MrData {
        title: title.into(),
        author: DEMO_PEOPLE[author].0.into(),
        web_url: format!("https://gitlab.com/demo/project/-/merge_requests/{id}"),
        source_branch: source.into(),
        target_branch: target.into(),
        ..crate::models::MrData::default()
    };
    mr
}

/// MRs merged over the last eight weeks, most of them on production, with
/// their review timeline: a past for the delivery page, the milestones'
/// forecast and the journal. A revert and a hotfix among them, for the change
/// failure rate.
#[cfg(feature = "gui")]
fn demo_history(app: &mut App, now: chrono::DateTime<chrono::Utc>) {
    use crate::models::{CommitFingerprint, MrCommit, ReviewActivity};
    let at = |days: f64| now - chrono::Duration::minutes((days * 1440.0) as i64);
    let text = |days: f64| at(days).format("%Y-%m-%dT%H:%M:%S.000Z").to_string();
    // id, title, author, reviewer, source branch, milestone, labels,
    // (opened, review asked, first review, approved, merged, on staging,
    // on production) in days ago, review rounds.
    type Past = (
        &'static str,
        &'static str,
        usize,
        usize,
        &'static str,
        Option<&'static str>,
        &'static [&'static str],
        [Option<f64>; 7],
        u32,
    );
    let past: [Past; 13] = [
        (
            "86",
            "feat(api): Expose pipelines in the MR payload",
            2,
            0,
            "feat/api-pipelines",
            None,
            &["feature"],
            [
                Some(54.0),
                Some(53.8),
                Some(52.0),
                Some(51.5),
                Some(51.4),
                Some(50.0),
                Some(48.0),
            ],
            1,
        ),
        (
            "87",
            "fix(ui): Wrap long branch names in the inspector",
            1,
            2,
            "fix/inspector-wrap",
            None,
            &["bug"],
            [
                Some(43.0),
                Some(42.9),
                Some(42.5),
                Some(42.4),
                Some(42.3),
                Some(41.0),
                Some(40.0),
            ],
            0,
        ),
        (
            "88",
            "feat(api): Cursor pagination for the MR list",
            2,
            0,
            "feat/cursor-pagination",
            Some("v2.4.0"),
            &["feature"],
            [
                Some(33.0),
                Some(32.8),
                Some(31.5),
                Some(30.2),
                Some(30.0),
                Some(29.0),
                Some(27.5),
            ],
            1,
        ),
        (
            "89",
            "fix(ui): Keep the selection when the list refreshes",
            1,
            3,
            "fix/keep-selection",
            Some("v2.4.0"),
            &["bug"],
            [
                Some(29.0),
                Some(28.9),
                Some(28.2),
                Some(28.0),
                Some(27.9),
                Some(27.0),
                Some(25.0),
            ],
            0,
        ),
        (
            "90",
            "chore(ci): Cache the cargo registry between jobs",
            3,
            1,
            "chore/ci-cache",
            None,
            &["ci"],
            [
                Some(26.0),
                Some(25.9),
                Some(24.0),
                Some(23.8),
                Some(23.7),
                Some(23.0),
                Some(20.9),
            ],
            0,
        ),
        (
            "91",
            "feat(tracker): Log time from the MR page",
            4,
            0,
            "feat/log-time-mr-page",
            Some("v2.4.0"),
            &["feature"],
            [
                Some(24.0),
                Some(23.0),
                Some(19.5),
                Some(17.0),
                Some(16.8),
                Some(16.0),
                Some(13.9),
            ],
            2,
        ),
        (
            "92",
            "perf(cache): LRU eviction for the diff stats cache",
            0,
            2,
            "perf/lru-diff-cache",
            None,
            &["performance"],
            [
                Some(20.0),
                Some(19.8),
                Some(18.0),
                Some(17.5),
                Some(17.4),
                Some(16.0),
                Some(13.9),
            ],
            1,
        ),
        (
            "93",
            "Revert \"perf(cache): LRU eviction for the diff stats cache\"",
            1,
            0,
            "revert-lru-diff-cache",
            None,
            &[],
            [
                Some(13.5),
                Some(13.48),
                Some(13.45),
                Some(13.42),
                Some(13.4),
                Some(13.3),
                Some(13.2),
            ],
            0,
        ),
        (
            "94",
            "fix(auth): Invalidate sessions on password change",
            2,
            1,
            "fix/session-invalidation",
            Some("v2.4.0"),
            &["bug", "security"],
            [
                Some(16.0),
                Some(15.5),
                Some(12.0),
                Some(11.0),
                Some(10.8),
                Some(10.0),
                Some(6.9),
            ],
            1,
        ),
        (
            "95",
            "fix(auth): Token refresh race on expired sessions",
            3,
            0,
            "hotfix/token-refresh",
            None,
            &["hotfix"],
            [
                Some(7.2),
                Some(7.15),
                Some(7.1),
                Some(7.05),
                Some(7.0),
                Some(6.95),
                Some(6.9),
            ],
            0,
        ),
        (
            "96",
            "feat(export): CSV report of the tracked MRs",
            4,
            2,
            "feat/csv-export",
            Some("v2.4.0"),
            &["feature"],
            [
                Some(14.0),
                Some(13.0),
                Some(8.0),
                Some(5.0),
                Some(4.8),
                Some(3.9),
                None,
            ],
            3,
        ),
        (
            "97",
            "fix(db): NULL handling in migration v3",
            0,
            1,
            "fix/migration-v3-null",
            Some("v2.5.0"),
            &["bug"],
            [
                Some(9.0),
                Some(8.5),
                Some(6.0),
                Some(5.6),
                Some(5.5),
                Some(4.5),
                None,
            ],
            1,
        ),
        (
            "98",
            "docs: Explain the stacked MRs page",
            1,
            3,
            "docs/stacks-page",
            None,
            &["docs"],
            [
                Some(5.0),
                Some(4.9),
                Some(3.2),
                Some(2.9),
                Some(2.8),
                None,
                None,
            ],
            0,
        ),
    ];
    let (staging, production) = ("staging".to_string(), "production".to_string());
    for (id, title, author, reviewer, source, milestone, labels, times, rounds) in past {
        if app.mrs.iter().any(|m| m.id == id) {
            continue;
        }
        let [opened, asked, reviewed, approved, merged, on_staging, on_production] = times;
        let mut mr = demo_mr(id, title, author, source, "main");
        let (reviewer_name, login) = DEMO_PEOPLE[reviewer];
        let mut branches: std::collections::BTreeSet<String> = ["main".to_string()].into();
        if let Some(d) = on_staging {
            branches.insert(staging.clone());
            app.branch_arrivals
                .insert((id.to_string(), staging.clone()), at(d));
        }
        if let Some(d) = on_production {
            branches.insert(production.clone());
            app.branch_arrivals
                .insert((id.to_string(), production.clone()), at(d));
        }
        mr.status = MrStatus::MergedIn(branches.into_iter().collect());
        mr.mergeability = MergeabilityStatus::NotOpen;
        let data = &mut mr.data;
        data.state = GitlabMrState::Merged;
        data.labels = labels.iter().map(|l| l.to_string()).collect();
        data.milestone = milestone.map(str::to_string);
        data.reviewers = vec![reviewer_name.into()];
        data.approved_by = vec![login.into()];
        data.approvers = vec![reviewer_name.into()];
        data.created_at = opened.map(text);
        data.merged_at = merged.map(text);
        data.updated_at = merged.map(text);
        data.merged_by = Some(DEMO_PEOPLE[reviewer].0.into());
        data.sha = Some(format!(
            "{:040x}",
            0xc0ffee_u64 * 1000 + id.parse::<u64>().unwrap_or(0)
        ));
        let first_commit = opened.map(|o| o + 0.4 + (id.len() % 3) as f64 * 0.3);
        data.commits = vec![MrCommit {
            sha: format!(
                "{:040x}",
                0xbeef_u64 * 1000 + id.parse::<u64>().unwrap_or(0)
            ),
            fingerprint: CommitFingerprint {
                author_email: format!("{}@demo.dev", DEMO_PEOPLE[author].1),
                authored_at: first_commit.map_or(0, |d| at(d).timestamp()),
                title: title.into(),
            },
        }];
        data.diff_stats = Some(crate::models::DiffStats {
            files_changed: 2 + (rounds * 3),
            additions: 40 + rounds * 120,
            deletions: 12 + rounds * 30,
            commits_count: 1 + rounds,
            ..Default::default()
        });
        // A push between the first review and the approval per round.
        let last_push = match (reviewed, approved) {
            (Some(r), Some(a)) if rounds > 0 => Some(text(r - (r - a) * 0.6)),
            _ => None,
        };
        data.review = Some(ReviewActivity {
            first_review_requested_at: asked.map(text),
            review_requested_at: asked
                .map(|a| [(login.to_string(), text(a))].into())
                .unwrap_or_default(),
            first_review_at: reviewed.map(text),
            approved_at: approved.map(text),
            last_push_at: last_push,
            pushes_after_review: rounds,
            last_activity_by: approved
                .or(reviewed)
                .map(|a| [(login.to_string(), text(a))].into())
                .unwrap_or_default(),
        });
        app.mrs.push(mr);
    }
}

/// MRs opened on another MR's branch: !108 on !107, !109 on !108 (a chain of
/// three), and !110 on !101's branch while !101 is merged (to retarget).
#[cfg(feature = "gui")]
fn demo_stacks(app: &mut App, now: chrono::DateTime<chrono::Utc>) {
    use crate::models::ReviewActivity;
    let text = |days: f64| {
        (now - chrono::Duration::minutes((days * 1440.0) as i64))
            .format("%Y-%m-%dT%H:%M:%S.000Z")
            .to_string()
    };
    let plan: [(
        &str,
        &str,
        usize,
        &str,
        &str,
        MergeabilityStatus,
        f64,
        f64,
        bool,
    ); 3] = [
        (
            "108",
            "feat(notif): Notification settings panel",
            3,
            "feat/notification-settings",
            "feat/desktop-notifications",
            MergeabilityStatus::Mergeable,
            5.0,
            3.0,
            true,
        ),
        (
            "109",
            "feat(notif): Mute notifications per project",
            3,
            "feat/notification-mute",
            "feat/notification-settings",
            MergeabilityStatus::Draft,
            2.0,
            1.0,
            false,
        ),
        (
            "110",
            "feat(auth): Keep the PKCE verifier across restarts",
            2,
            "feat/pkce-verifier-store",
            "feat/oauth2-pkce",
            MergeabilityStatus::Mergeable,
            8.0,
            6.5,
            false,
        ),
    ];
    for (id, title, author, source, target, mergeability, opened, pushed, approved) in plan {
        if app.mrs.iter().any(|m| m.id == id) {
            continue;
        }
        let mut mr = demo_mr(id, title, author, source, target);
        mr.mergeability = mergeability;
        let data = &mut mr.data;
        data.state = GitlabMrState::Opened;
        data.labels = vec!["feature".into()];
        data.reviewers = vec![DEMO_PEOPLE[0].0.into()];
        if approved {
            data.approved_by = vec![DEMO_PEOPLE[0].1.into()];
            data.approvers = vec![DEMO_PEOPLE[0].0.into()];
        }
        data.created_at = Some(text(opened));
        data.updated_at = Some(text(pushed));
        data.diff_stats = Some(crate::models::DiffStats {
            files_changed: 3,
            additions: 96,
            deletions: 14,
            commits_count: 2,
            commits_behind: Some(if id == "108" { 2 } else { 0 }),
            ..Default::default()
        });
        data.review = Some(ReviewActivity {
            first_review_requested_at: Some(text(opened - 0.2)),
            review_requested_at: [(DEMO_PEOPLE[0].1.to_string(), text(opened - 0.2))].into(),
            first_review_at: approved.then(|| text(pushed + 0.5)),
            approved_at: approved.then(|| text(pushed - 0.5)),
            last_push_at: Some(text(pushed)),
            ..ReviewActivity::default()
        });
        app.mrs.push(mr);
    }
}

/// What the demo's MR page reads "from GitLab" when it opens (`gui` feature):
/// merge checks, approval rules, threads and deployments, the same for any MR.
#[cfg(feature = "gui")]
pub fn demo_page_data(mr: &crate::models::TrackedMr) -> crate::gitlab_detail::PageData {
    use crate::gitlab_detail::{Deployment, Note, Overview, PageData, Rule, Thread, User};
    let user = |name: &str, username: &str| User {
        name: name.into(),
        username: username.into(),
    };
    let note = |author: &str, username: &str, body: &str, at: &str| Note {
        author: author.into(),
        username: username.into(),
        body: body.into(),
        created_at: at.into(),
    };
    let open = mr.state == GitlabMrState::Opened;
    let conflict = mr.mergeability == MergeabilityStatus::Conflict;
    let file = demo_file_diffs()
        .first()
        .map(|f| f.new_path.clone())
        .unwrap_or_default();
    // A long list on purpose: the page must keep it short.
    let conflicts = conflict.then(|| {
        [
            ("src/db/pool.rs", true),
            ("src/db/pool/config.rs", true),
            ("src/db/pool/health.rs", true),
            ("src/db/migrations/2024_05_02_pool_settings.sql", true),
            ("src/api/handlers/merge_requests.rs", true),
            ("src/api/handlers/pipelines.rs", true),
            ("src/config/defaults.toml", true),
            ("tests/integration/pool_deadlock.rs", true),
            ("Cargo.lock", true),
            ("src/db/pool/metrics.rs", true),
            ("src/db/pool/retry.rs", true),
            ("src/api/middleware/timeout.rs", true),
            ("tests/integration/pool_saturation.rs", true),
            ("Cargo.toml", false),
            ("src/db/mod.rs", false),
            ("src/main.rs", false),
            ("docs/operations/database.md", false),
        ]
        .into_iter()
        .map(|(path, clash)| crate::gitlab_detail::ConflictFile {
            path: path.into(),
            clash,
        })
        .collect()
    });
    PageData {
        conflicts,
        overview: Some(Overview {
            detailed_merge_status: if !open {
                "not_open".into()
            } else if conflict {
                "conflict".into()
            } else {
                "not_approved".into()
            },
            has_conflicts: conflict,
            blocking_discussions_resolved: mr.user_notes_count == 0,
            draft: mr.mergeability == MergeabilityStatus::Draft,
            diverged_commits_count: Some(4),
            force_remove_source_branch: Some(true),
        }),
        rules: Some(vec![
            Rule {
                name: "Backend maintainers".into(),
                approvals_required: 1,
                approved: !open,
                approved_by: if open { Vec::new() } else { vec![user("Alex Devries", "alex_dev")] },
                eligible_approvers: vec![user("Alex Devries", "alex_dev"), user("Marina Graphetti", "marina_g")],
            },
            Rule {
                name: "DBA".into(),
                approvals_required: 1,
                approved: !open,
                approved_by: Vec::new(),
                eligible_approvers: vec![user("Sarah Coste", "sarah_c")],
            },
        ]),
        threads: Ok(vec![
            Thread {
                resolved: false,
                path: Some(file.clone()),
                line: Some(16),
                notes: vec![
                    note("Alex Devries", "alex_dev", "If the context has no `Db`, this now returns an error instead of panicking: can we cover that case with a test?", "2024-11-12T09:14:00Z"),
                    note("Marina Graphetti", "marina_g", "+1, and the error message should name the missing data.", "2024-11-13T10:02:00Z"),
                ],
            },
            Thread {
                resolved: false,
                path: None,
                line: None,
                notes: vec![note("Sarah Coste", "sarah_c", "Needs a rebase on main before I review again.", "2024-11-14T08:40:00Z")],
            },
            Thread {
                resolved: true,
                path: Some("Cargo.toml".into()),
                line: Some(21),
                notes: vec![
                    note("Alex Devries", "alex_dev", "Pin async-graphql to a minor version?", "2024-11-11T16:20:00Z"),
                    note("Thomas Dubosc", "thomas_db", "Done in the last push.", "2024-11-11T17:05:00Z"),
                ],
            },
        ]),
        deployments: vec![
            Deployment {
                environment: "staging".into(),
                reference: "staging".into(),
                sha: "5e1c0a97".into(),
                created_at: "2024-11-14T12:30:00Z".into(),
                includes: Some(!open),
            },
            Deployment {
                environment: "production".into(),
                reference: "production".into(),
                sha: "0b7d2f44".into(),
                created_at: "2024-11-12T18:00:00Z".into(),
                includes: Some(false),
            },
        ],
        // As many as `diverged_commits_count`: a tracked merge, a tracked
        // squash (known by its SHA), a merge the tracking missed, a direct push.
        behind: open.then(|| {
            let commit = |sha: &str, title: &str, author: &str, at: &str, iid: Option<&str>| {
                crate::gitlab_detail::BehindCommit {
                    sha: sha.into(),
                    title: title.into(),
                    author: author.into(),
                    created_at: at.into(),
                    web_url: format!("https://gitlab.com/demo/project/-/commit/{sha}"),
                    mr_iid: iid.map(String::from),
                }
            };
            vec![
                commit("7f3a91c0e2", "feat(api): Introduce GraphQL endpoint for MR metadata", "Marina Graphetti", "2024-11-14T09:00:00Z", Some("104")),
                commit("e41b0d2a77", "docs: Fix the setup steps in the README", "Thomas Dubosc", "2024-11-13T15:20:00Z", None),
                commit("3c9e5f1b08", "chore(ci): Cache cargo registry between jobs", "Sarah Coste", "2024-11-12T11:10:00Z", Some("98")),
                commit("a1b2c3d4", "feat(auth): Add OAuth2 PKCE flow for mobile clients", "Alex Devries", "2024-11-10T09:45:00Z", None),
            ]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{dispatch, Action, ActionContext};

    #[tokio::test]
    async fn demo_tickets_come_with_their_tracker_and_time_log() {
        let app = demo_app(AppConfig::default()).await;
        assert_eq!(app.tracker.as_ref().map(|t| t.name()), Some("Redmine"));
        assert!(!app.activities.is_empty());
        let tickets: Vec<_> = app
            .mrs
            .iter()
            .filter_map(|mr| mr.linked_ticket.as_ref())
            .collect();
        assert!(!tickets.is_empty());
        for ticket in tickets {
            let Some(TimeLogState::Loaded(entries)) = app.time_entries.get(&ticket.id) else {
                panic!("no time log for ticket #{}", ticket.id);
            };
            let logged: f32 = entries.iter().map(|e| e.hours).sum();
            let spent = ticket.time_spent.unwrap_or(0) as f32 / 3600.0;
            assert_eq!(logged, spent, "ticket #{}", ticket.id);
        }
    }

    #[tokio::test]
    async fn the_demo_never_logs_time() {
        let mut app = demo_app(AppConfig::default()).await;
        let row = app
            .visible_mrs()
            .position(|mr| mr.linked_ticket.is_some())
            .expect("a demo MR with a ticket");
        app.table_state.select(Some(row));
        app.log_time_form.duration_input = "1h".into();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = ActionContext {
            api_semaphore: &std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            tx: &tx,
            last_known_branches: &mut HashMap::new(),
            state_writer: None,
        };
        dispatch(Action::SubmitLogTime, &mut app, &mut ctx).await;
        assert!(!app.log_time_form.submitting);
        assert_eq!(
            app.log_time_form.error.as_deref(),
            Some("Demo: no time is logged")
        );
        assert!(rx.try_recv().is_err(), "nothing was sent to the tracker");
    }
}
