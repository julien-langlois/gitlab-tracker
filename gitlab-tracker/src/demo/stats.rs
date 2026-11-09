//! The stats database of a demo project, recorded as the app records it: a
//! snapshot when an MR merges or closes, one a day while it is open, and each
//! arrival on a branch with how it got there.

use super::engine::{How, World};
use crate::models::{GitlabMrState, TrackedMr};
use gitlab_tracker_stats::snapshot::SnapshotTrigger;
use gitlab_tracker_stats::{ArrivalKind, BranchArrival, SqliteStatsDb};
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
    Some(Arc::new(db))
}
