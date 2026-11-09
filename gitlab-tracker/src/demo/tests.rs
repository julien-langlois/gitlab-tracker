//! The demo shows every case the app knows, and its pages agree.

use super::*;
use crate::actions::{dispatch, Action, ActionContext};
use crate::app::TimeLogState;
use crate::models::{
    BranchPresence, Effort, GitlabMrState, MergeabilityStatus, MrStatus, PipelineState,
};
use crate::status::StatusKind;
use std::collections::{HashMap, HashSet};

async fn both() -> Vec<App> {
    vec![
        demo_app(AppConfig::default()).await,
        mobile_app(AppConfig::default()).await,
    ]
}

fn time(at: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(at)
        .unwrap_or_else(|e| panic!("{at}: {e}"))
        .with_timezone(&chrono::Utc)
}

#[tokio::test]
async fn every_mr_tells_a_consistent_story() {
    let now = clock::clock().now;
    for app in both().await {
        for mr in &app.mrs {
            if mr.author == "Loading" {
                assert!(matches!(mr.status, MrStatus::Loading));
                continue;
            }
            let id = &mr.id;
            let created = time(mr.created_at.as_deref().unwrap());
            let updated = time(mr.updated_at.as_deref().unwrap());
            assert!(created <= updated && updated <= now, "!{id} dates");
            // The unresolved threads are the count the list shows.
            let unresolved = mr.threads.iter().filter(|t| !t.resolved).count() as u32;
            assert_eq!(mr.user_notes_count, unresolved, "!{id} threads");
            // Newest pipeline first, each after the MR opened.
            let starts: Vec<_> = mr
                .pipelines
                .iter()
                .map(|p| time(p.created_at.as_deref().unwrap()))
                .collect();
            assert!(starts.windows(2).all(|w| w[0] >= w[1]), "!{id} pipelines");
            assert!(
                starts.iter().all(|s| *s >= created),
                "!{id} pipeline before open"
            );
            // The approvals named are approvals given.
            assert_eq!(mr.approved_by.len(), mr.approvers.len(), "!{id}");
            match mr.state {
                GitlabMrState::Merged => {
                    let merged = time(mr.merged_at.as_deref().unwrap());
                    assert!(created < merged && merged <= now, "!{id} merge");
                    assert_eq!(mr.mergeability, MergeabilityStatus::NotOpen);
                    if app.branches.contains(&mr.target_branch) {
                        let MrStatus::MergedIn(found) = &mr.status else {
                            panic!("!{id} status");
                        };
                        assert!(found.contains(&mr.target_branch), "!{id} on its target");
                    }
                }
                GitlabMrState::Opened | GitlabMrState::Closed => {
                    assert!(mr.merged_at.is_none() && mr.merged_by.is_none(), "!{id}");
                    if let MrStatus::MergedIn(found) = &mr.status {
                        assert!(found.is_empty(), "!{id} is on no branch");
                    }
                }
            }
            // A branch reached beyond the target came after the merge.
            for ((mr_id, branch), at) in &app.branch_arrivals {
                if mr_id == id {
                    let merged = time(mr.merged_at.as_deref().unwrap());
                    assert!(*at >= merged, "!{id} on {branch} before its merge");
                }
            }
        }
        // iids follow the opening order.
        let mut by_iid: Vec<(u32, String)> = app
            .mrs
            .iter()
            .filter(|mr| mr.author != "Loading")
            .map(|mr| (mr.id.parse().unwrap(), mr.created_at.clone().unwrap()))
            .collect();
        by_iid.sort();
        assert!(by_iid.windows(2).all(|w| time(&w[0].1) <= time(&w[1].1)));
    }
}

#[tokio::test]
async fn every_status_shows_somewhere() {
    let mut seen = HashSet::new();
    let mut loading = false;
    let mut failed = false;
    for app in both().await {
        for mr in &app.mrs {
            match &mr.status {
                MrStatus::Loading => loading = true,
                MrStatus::Error(_) => failed = true,
                MrStatus::MergedIn(_) => {}
            }
            seen.insert(StatusKind::of(mr.state, &mr.mergeability));
        }
    }
    assert!(loading && failed);
    // `NotOpen` on an open MR is a moment between two refreshes.
    for kind in StatusKind::ALL
        .into_iter()
        .filter(|k| *k != StatusKind::NotOpen)
    {
        assert!(seen.contains(&kind), "{kind:?} is not in the demo");
    }
}

#[tokio::test]
async fn every_way_to_reach_a_branch_shows() {
    let mut seen = HashSet::new();
    for app in both().await {
        for mr in &app.mrs {
            for branch in &app.branches {
                if let Some(p) = app.branch_presence(mr, branch) {
                    seen.insert(format!("{p:?}"));
                }
            }
        }
    }
    for p in [
        BranchPresence::Present,
        BranchPresence::CherryPicked,
        BranchPresence::Copied,
        BranchPresence::Partial,
        BranchPresence::Absent,
    ] {
        assert!(seen.contains(&format!("{p:?}")), "{p:?} is not in the demo");
    }
}

#[tokio::test]
async fn every_effort_and_pipeline_state_shows() {
    let app = demo_app(AppConfig::default()).await;
    let efforts: HashSet<String> = app
        .mrs
        .iter()
        .filter_map(|mr| mr.diff_stats.as_ref())
        .map(|d| {
            format!(
                "{:?}",
                Effort::from_score(d.difficulty(&app.config.complexity_profile))
            )
        })
        .collect();
    assert_eq!(efforts.len(), 3, "{efforts:?}");
    let mut states = HashSet::new();
    let mut jobs = HashSet::new();
    for app in both().await {
        for p in app.mrs.iter().flat_map(|mr| &mr.pipelines) {
            states.insert(format!("{:?}", p.status));
            jobs.extend(p.jobs.iter().map(|j| j.status.clone()));
        }
    }
    for state in [
        PipelineState::Success,
        PipelineState::Failed,
        PipelineState::Running,
        PipelineState::Pending,
        PipelineState::Canceled,
    ] {
        assert!(states.contains(&format!("{state:?}")), "{state:?}");
    }
    for job in [
        "success", "failed", "skipped", "running", "pending", "created", "canceled",
    ] {
        assert!(jobs.contains(job), "no {job} job");
    }
}

#[tokio::test]
async fn the_merged_mrs_fill_every_size_of_the_stats() {
    // The stats' buckets: under 200 changed lines, 800, 2 000, and more.
    let app = demo_app(AppConfig::default()).await;
    let mut buckets = [0; 4];
    for mr in app
        .mrs
        .iter()
        .filter(|mr| mr.state == GitlabMrState::Merged)
    {
        let d = mr.diff_stats.as_ref().unwrap();
        let size = d.additions + d.deletions;
        buckets[match size {
            0..200 => 0,
            200..800 => 1,
            800..2000 => 2,
            _ => 3,
        }] += 1;
    }
    assert!(buckets.iter().all(|n| *n > 0), "{buckets:?}");
}

#[tokio::test]
async fn every_board_column_has_a_ticket() {
    let mut statuses = HashSet::new();
    let mut over_estimate = false;
    for app in both().await {
        for ticket in app.mrs.iter().filter_map(|mr| mr.linked_ticket.as_ref()) {
            statuses.insert(ticket.status.clone());
            over_estimate |= ticket.time_spent > ticket.time_estimate;
        }
    }
    for status in tracker::STATUSES {
        assert!(statuses.contains(status), "no ticket {status}");
    }
    assert!(over_estimate);
}

#[tokio::test]
async fn demo_tickets_come_with_their_tracker_and_time_log() {
    for app in both().await {
        assert_eq!(app.tracker.as_ref().map(|t| t.name()), Some("Redmine"));
        assert!(!app.activities.is_empty());
        for ticket in app.mrs.iter().filter_map(|mr| mr.linked_ticket.as_ref()) {
            let Some(TimeLogState::Loaded(entries)) = app.time_entries.get(&ticket.id) else {
                panic!("no time log for ticket #{}", ticket.id);
            };
            let logged: f32 = entries.iter().map(|e| e.hours).sum();
            let spent = ticket.time_spent.unwrap_or(0) as f32 / 3600.0;
            assert!((logged - spent).abs() < 0.01, "ticket #{}", ticket.id);
        }
    }
}

#[tokio::test]
async fn the_last_session_left_changes_of_every_kind() {
    use crate::app::ChangeKind;
    let app = demo_app(AppConfig::default()).await;
    let kinds: HashSet<String> = app
        .last_changes
        .iter()
        .map(|c| match &c.kind {
            ChangeKind::Arrived(_) => "arrived".to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    for kind in ["Merged", "Closed", "arrived", "CiFailing"] {
        assert!(kinds.contains(kind), "no {kind} in {kinds:?}");
    }
    assert!(app.last_digest.is_some());
}

#[tokio::test]
async fn the_mobile_hotfix_has_to_come_down() {
    let app = mobile_app(AppConfig::default()).await;
    let behind: Vec<&str> = app
        .behind_on("develop")
        .iter()
        .map(|mr| mr.id.as_str())
        .collect();
    assert_eq!(behind, [iid("push-wrong-parcel").as_str()]);
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

#[cfg(feature = "gui")]
#[tokio::test]
async fn the_pages_read_from_the_same_history() {
    let app = demo_app(AppConfig::default()).await;
    for mr in &app.mrs {
        // The patches add up to the diff stats.
        let files = demo_file_diffs(&app, &mr.id);
        let stats = mr.diff_stats.as_ref().unwrap();
        assert_eq!(files.len() as u32, stats.files_changed, "!{}", mr.id);
        // Each job has a log.
        for job in mr.pipelines.iter().flat_map(|p| &p.jobs) {
            assert!(!demo_job_log(&app, job.id).contains("no longer kept"));
        }
        let page = demo_page_data(mr);
        let threads = page.threads.as_ref().unwrap();
        assert_eq!(threads.len(), mr.threads.len(), "!{}", mr.id);
        // GitLab's count of the commits behind is the list of them.
        if mr.state == GitlabMrState::Opened
            && !matches!(
                mr.mergeability,
                MergeabilityStatus::Mergeable
                    | MergeabilityStatus::SyncFailed
                    | MergeabilityStatus::Retrying
                    | MergeabilityStatus::Unknown
            )
        {
            let behind = page.behind.as_ref().map_or(0, Vec::len) as u32;
            assert_eq!(stats.commits_behind, Some(behind), "!{}", mr.id);
        }
        // An MR GitLab says is mergeable meets its approval rules.
        if mr.mergeability == MergeabilityStatus::Mergeable {
            assert!(page.rules.unwrap().iter().all(|r| r.approved), "!{}", mr.id);
        }
    }
    // The conflict is on a file main changed since.
    let conflict = app
        .mrs
        .iter()
        .find(|mr| mr.mergeability == MergeabilityStatus::Conflict)
        .unwrap();
    let files = demo_page_data(conflict).conflicts.unwrap();
    assert!(files.iter().any(|f| f.clash), "{files:?}");
    // Production runs what was deployed last, and the staging deploy failed.
    let overview = demo_project_overview(&app);
    let staging = overview
        .pipelines
        .iter()
        .find(|p| p.branch == "staging")
        .unwrap();
    assert_eq!(staging.status, PipelineState::Failed);
    let main = overview
        .pipelines
        .iter()
        .find(|p| p.branch == "main")
        .unwrap();
    assert_eq!(main.status, PipelineState::Running);
    assert!(overview.deployments.iter().any(|d| d.status == "failed"));
    // The bell has notifications, some seen already.
    let notices = demo_notifications(&app);
    assert!(notices.iter().any(|n| n.4) && notices.iter().any(|n| !n.4));
    // Release notes since the last release.
    let (tag, titles) = demo_changelog(&app);
    assert_eq!(tag, "v2.3.0");
    assert!(!titles.is_empty());
}
