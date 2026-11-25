use super::*;
use gitlab_tracker_gitlab::models::{MrThread, Pipeline, PipelineState, ReviewActivity};
use std::path::PathBuf;

const URL: &str = "https://gitlab.example";

fn mr(id: &str) -> MrRow {
    MrRow {
        id: id.into(),
        flagged: false,
        snooze: None,
        linked_ticket: None,
        found_branches: Some(HashSet::new()),
        mergeability: MergeabilityStatus::Unknown,
        data: MrData {
            title: format!("MR {id}"),
            author: "ana".into(),
            web_url: format!("https://g/{id}"),
            source_branch: "feature".into(),
            target_branch: "develop".into(),
            created_at: Some("2026-11-16T08:00:00Z".into()),
            ..MrData::default()
        },
    }
}

async fn db() -> (StateDb, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "gt-state-db-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (StateDb::open(&dir.join(DB_FILE)).await.unwrap(), dir)
}

#[tokio::test]
async fn a_database_from_a_newer_version_is_left_untouched() {
    let (db, dir) = db().await;
    let newer = MIGRATIONS.len() + 1;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA user_version = {newer}"
    )))
    .execute(&db.pool)
    .await
    .unwrap();
    drop(db);
    let err = StateDb::open(&dir.join(DB_FILE))
        .await
        .err()
        .expect("refused");
    assert!(err.to_string().contains("newer version"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

async fn kinds(db: &StateDb, id: &str) -> Vec<(&'static str, Option<String>)> {
    db.events_of(URL, "1", id)
        .await
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.subject.or(e.actor)))
        .collect()
}

#[tokio::test]
async fn each_save_appends_what_changed_and_nothing_when_nothing_did() {
    let (db, dir) = db().await;
    let state = ProjectState::default();
    let mut a = mr("1");
    db.save(URL, "1", std::slice::from_ref(&a), &state, "d1")
        .await
        .unwrap();
    assert_eq!(
        kinds(&db, "1").await,
        [("tracked", None), ("opened", Some("ana".into()))]
    );

    // Nothing new: no row, no event.
    assert_eq!(
        db.save(URL, "1", std::slice::from_ref(&a), &state, "d2")
            .await
            .unwrap(),
        0
    );

    // The next day: reviewed, approved, a pipeline, a thread, a label; then merged.
    a.data.labels = vec!["bug".into()];
    a.data.approved_by = vec!["bob".into()];
    a.data.review = Some(ReviewActivity {
        approvals: [("bob".to_string(), "2026-11-17T09:00:00Z".to_string())].into(),
        ..ReviewActivity::default()
    });
    a.data.pipelines = vec![Pipeline {
        id: 7,
        status: PipelineState::Running,
        ..Pipeline::default()
    }];
    a.data.threads = vec![MrThread {
        note_id: 70,
        author: "bob".into(),
        excerpt: "why?".into(),
        created_at: "2026-11-17T08:00:00Z".into(),
        last_note_at: "2026-11-17T08:00:00Z".into(),
        notes: 1,
        resolved: false,
    }];
    a.mergeability = MergeabilityStatus::Mergeable;
    db.save(URL, "1", std::slice::from_ref(&a), &state, "d3")
        .await
        .unwrap();
    let events = db.events_of(URL, "1", "1").await.unwrap();
    let approved = events.iter().find(|e| e.kind == "approved").unwrap();
    assert_eq!(approved.at.as_deref(), Some("2026-11-17T09:00:00Z"));
    assert_eq!(
        kinds(&db, "1").await[2..],
        [
            ("label_added", Some("bug".into())),
            ("approved", Some("bob".into())),
            ("pipeline", Some("7".into())),
            ("thread_opened", Some("70".into())),
            ("mergeability", None),
        ]
    );

    a.data.state = GitlabMrState::Merged;
    a.data.merged_by = Some("bob".into());
    a.data.pipelines[0].status = PipelineState::Success;
    a.data.threads[0].resolved = true;
    a.data.pipelines.clear(); // GitLab's list moves on: the table keeps it
    a.found_branches = Some(["develop".to_string()].into());
    db.save(URL, "1", std::slice::from_ref(&a), &state, "d4")
        .await
        .unwrap();
    assert_eq!(
        kinds(&db, "1").await[7..],
        [
            ("merged", Some("bob".into())),
            ("thread_resolved", Some("70".into())),
            ("branch_reached", Some("develop".into())),
        ]
    );
    let pipelines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mr_pipelines")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(pipelines, 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_loading_placeholder_keeps_what_was_read_and_removal_keeps_the_history() {
    let (db, dir) = db().await;
    let state = ProjectState::default();
    let mut a = mr("1");
    a.found_branches = Some(["develop".to_string()].into());
    a.data.approved_by = vec!["bob".into()];
    db.save(URL, "1", &[a.clone(), mr("2")], &state, "d1")
        .await
        .unwrap();

    // Restarting: read again, shown as a placeholder, its status loading.
    let placeholder = MrRow {
        found_branches: None,
        data: gitlab_tracker_gitlab::models::TrackedMr::placeholder(
            "1".into(),
            "MR 1".into(),
            None,
        )
        .data,
        ..a.clone()
    };
    assert_eq!(
        db.save(URL, "1", &[placeholder, mr("2")], &state, "d2")
            .await
            .unwrap(),
        0
    );
    let (mrs, _) = db.load(URL, "1").await.unwrap().unwrap();
    assert_eq!(mrs[0].data.approved_by, ["bob"]);
    assert!(mrs[0].found_branches.contains("develop"));

    // Removed from the dashboard: not loaded, its row and history kept;
    // added back, it comes back with them.
    db.save(URL, "1", &[mr("2")], &state, "d3").await.unwrap();
    let (mrs, _) = db.load(URL, "1").await.unwrap().unwrap();
    assert_eq!(mrs.len(), 1);
    assert_eq!(kinds(&db, "1").await.last().unwrap().0, "untracked");
    db.save(URL, "1", &[mr("2"), a], &state, "d4")
        .await
        .unwrap();
    let (mrs, _) = db.load(URL, "1").await.unwrap().unwrap();
    let ids: Vec<&str> = mrs.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["2", "1"]);
    assert_eq!(kinds(&db, "1").await.last().unwrap().0, "tracked");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn projects_are_kept_apart() {
    let (db, dir) = db().await;
    let one = ProjectState {
        discovery_started_at: Some("2026-01-01".into()),
        dismissed_mr_ids: ["9".to_string()].into(),
        // By MR: its branches, none for one known on none (kept: not new).
        last_known_branches: [
            ("1".to_string(), ["main".to_string()].into()),
            ("4".to_string(), HashSet::new()),
        ]
        .into(),
    };
    db.save(URL, "1", &[mr("1")], &one, "d1").await.unwrap();
    db.save(URL, "2", &[mr("5")], &ProjectState::default(), "d1")
        .await
        .unwrap();
    let (mrs, state) = db.load(URL, "1").await.unwrap().unwrap();
    assert_eq!((mrs.len(), mrs[0].id.as_str()), (1, "1"));
    assert_eq!(state, one);
    assert!(db.load(URL, "3").await.unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}
