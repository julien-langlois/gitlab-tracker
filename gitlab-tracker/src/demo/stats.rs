//! The stats database of a demo project, recorded as the app records it: a
//! snapshot when an MR merges or closes, one a day while it is open, and each
//! arrival on a branch with how it got there; the deployments it read; and
//! the months before the demo's MRs, for the DORA trend.

use super::engine::{How, World};
use crate::models::{GitlabMrState, TrackedMr};
use chrono::{Datelike, Duration, TimeZone, Utc};
use gitlab_tracker_stats::snapshot::MrStatsSnapshot;
use gitlab_tracker_stats::snapshot::SnapshotTrigger;
use gitlab_tracker_stats::{ArrivalKind, BranchArrival, Deployment, SqliteStatsDb};
use std::sync::Arc;

pub async fn seed(
    world: &World,
    mrs: &[TrackedMr],
    project_id: &str,
    profile: &crate::models::DifficultyProfile,
) -> Option<Arc<SqliteStatsDb>> {
    let db = SqliteStatsDb::open(":memory:").await.ok()?;
    for (built, mr) in world.mrs.iter().zip(mrs) {
        if mr.title.is_empty() || mr.author == "Loading" {
            continue;
        }
        let record = |trigger, at: chrono::DateTime<chrono::Utc>| {
            let snapshot = mr.stats_snapshot(trigger, project_id, profile);
            let at = at.to_rfc3339();
            let db = &db;
            async move {
                let _ = db.upsert_snapshot_at(&snapshot, &at).await;
            }
        };
        match mr.state {
            GitlabMrState::Merged => {
                if let Some((at, _)) = built.merged {
                    record(SnapshotTrigger::OnMerge, at).await;
                }
            }
            GitlabMrState::Closed => {
                if let Some((at, _)) = built.closed {
                    record(SnapshotTrigger::OnClose, at).await;
                }
            }
            GitlabMrState::Opened => {
                // Once a day while the app ran: every morning since it opened.
                let mut day = world.now;
                while day >= built.opened {
                    record(SnapshotTrigger::OnRefresh, day).await;
                    day -= chrono::Duration::days(1);
                }
            }
        }
    }
    for arrival in &world.arrivals {
        let mr = &world.mrs[arrival.mr];
        let _ = db
            .record_branch_arrival(&BranchArrival {
                project_id: project_id.to_string(),
                mr_id: mr.iid.clone(),
                branch: arrival.branch.to_string(),
                arrived_at: arrival.at,
                how: match arrival.how {
                    How::Merge => ArrivalKind::Merge,
                    How::CherryPick => ArrivalKind::CherryPick,
                    How::Copy => ArrivalKind::Copy,
                },
                merged_at: mr.merged.map(|(at, _)| super::clock::iso(at)),
            })
            .await;
    }
    // The deployments as the app keeps them from its reads.
    let deployments: Vec<Deployment> = world
        .deployments
        .iter()
        .map(|d| Deployment {
            environment: d.environment.into(),
            reference: d.branch.into(),
            sha: d.sha.clone(),
            created_at: d.at,
            status: if d.ok { "success" } else { "failed" }.into(),
        })
        .collect();
    let _ = db.record_deployments(project_id, &deployments).await;
    history(&db, world, project_id).await;
    Some(Arc::new(db))
}

/// The months before the demo's MRs, as an app that ran then would have kept
/// them, for the Delivery page's DORA trend: oldest first, shipping more
/// often and faster month after month, with a bad month (failed deployments
/// and a revert) on the way.
async fn history(db: &SqliteStatsDb, world: &World, project_id: &str) {
    let (Some(target), Some(production)) =
        (world.spec.branches.first(), world.spec.branches.last())
    else {
        return;
    };
    let Some(environment) = world
        .spec
        .environments
        .iter()
        .find(|(b, _)| b == production)
        .map(|(_, e)| *e)
    else {
        return;
    };
    // Up to the first MR of the demo's own history.
    let Some(first) = world.mrs.iter().map(|m| m.opened).min() else {
        return;
    };
    let first_month = first.date_naive().with_day(1).unwrap_or(first.date_naive());
    const TITLES: [&str; 8] = [
        "feat(api): paginate the carriers list",
        "fix(auth): refresh tokens a minute before they expire",
        "chore(deps): bump the HTTP client",
        "feat(shipments): filter by destination country",
        "fix(db): index the tracking numbers",
        "refactor(api): one error type per route",
        "feat(webhooks): retry with backoff",
        "fix(export): escape commas in CSV fields",
    ];
    let mut iid = 9000;
    for back in (1..=9u32).rev() {
        let Some(start) = first_month.checked_sub_months(chrono::Months::new(back)) else {
            continue;
        };
        let start = Utc.from_utc_datetime(&start.and_hms_opt(9, 0, 0).unwrap_or_default());
        // Older months: fewer releases, longer leads.
        let age = back as i64;
        let releases = 2 + (9 - age) / 2;
        let lead_days = 4 + age;
        let bad_month = back == 4;
        let mut deployments = Vec::new();
        for r in 0..releases {
            let deployed = start + Duration::days(2 + r * 28 / releases.max(1));
            let failed = bad_month && r < 2;
            if failed {
                deployments.push(Deployment {
                    environment: environment.into(),
                    reference: (*production).into(),
                    sha: format!("{iid:x}f"),
                    created_at: deployed - Duration::hours(30),
                    status: "failed".into(),
                });
            }
            deployments.push(Deployment {
                environment: environment.into(),
                reference: (*production).into(),
                sha: format!("{iid:x}"),
                created_at: deployed,
                status: "success".into(),
            });
            // Two MRs a release, merged into the target, then promoted.
            for k in 0..2i64 {
                iid += 1;
                let opened = deployed - Duration::days(lead_days + k);
                let merged = deployed - Duration::days(1 + k);
                let revert = bad_month && r == releases - 1 && k == 1;
                let title = if revert {
                    format!("Revert \"{}\"", TITLES[0])
                } else {
                    TITLES[(iid as usize) % TITLES.len()].to_string()
                };
                let snapshot = MrStatsSnapshot {
                    mr_id: iid.to_string(),
                    project_id: project_id.into(),
                    title,
                    trigger: SnapshotTrigger::OnMerge,
                    author: "marina".into(),
                    assignee: None,
                    reviewers: vec!["alex".into()],
                    merged_by: Some("alex".into()),
                    milestone: None,
                    labels: Vec::new(),
                    target_branch: (*target).into(),
                    state: "merged".into(),
                    created_at: Some(super::clock::iso(opened)),
                    merged_at: Some(super::clock::iso(merged)),
                    updated_at: Some(super::clock::iso(merged)),
                    ready_at: Some(super::clock::iso(opened + Duration::hours(6))),
                    files_changed: 4,
                    additions: 60,
                    deletions: 12,
                    commits_count: 3,
                    diff_difficulty: None,
                    user_notes_count: 3,
                    pipeline_count: 2,
                    pipeline_failure_count: 0,
                };
                let _ = db.upsert_snapshot_at(&snapshot, &merged.to_rfc3339()).await;
                let _ = db
                    .record_branch_arrival(&BranchArrival {
                        project_id: project_id.into(),
                        mr_id: iid.to_string(),
                        branch: (*production).into(),
                        arrived_at: deployed,
                        how: ArrivalKind::Merge,
                        merged_at: Some(super::clock::iso(merged)),
                    })
                    .await;
            }
        }
        let _ = db.record_deployments(project_id, &deployments).await;
    }
}
