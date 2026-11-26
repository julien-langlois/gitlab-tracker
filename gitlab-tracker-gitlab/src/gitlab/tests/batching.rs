//! Request batching: the list reads, what is read again and when, the estimate.

use super::*;

fn listed(json: serde_json::Value) -> GitLabMr {
    serde_json::from_value(json).unwrap()
}
#[test]
fn mrs_are_listed_by_iid_with_a_mergeability_recheck() {
    assert_eq!(
        mr_list_url(&ctx("https://gl.test"), &["4", "17"]),
        "https://gl.test/api/v4/projects/1/merge_requests?with_merge_status_recheck=true&iids[]=4&iids[]=17"
    );
}
#[test]
fn only_open_mrs_that_cannot_merge_read_their_own_page() {
    let mr = |state: &str, status: &str| {
        listed(serde_json::json!({
            "title": "t", "state": state, "detailed_merge_status": status
        }))
    };
    assert!(!needs_single_read(&mr("merged", "not_open")));
    assert!(!needs_single_read(&mr("closed", "not_open")));
    assert!(!needs_single_read(&mr("opened", "mergeable")));
    // `diverged_commits_count` (commits behind) comes with the page only.
    assert!(needs_single_read(&mr("opened", "need_rebase")));
    assert!(needs_single_read(&mr("opened", "draft_status")));
    // Still computing: the page read retries.
    assert!(needs_single_read(&mr("opened", "checking")));
}
fn reviewed(unresolved: u32) -> CachedMrData {
    CachedMrData {
        review: Some(ReviewActivity {
            version: crate::models::REVIEW_ACTIVITY_VERSION,
            ..ReviewActivity::default()
        }),
        user_notes_count: unresolved,
        mergeability: Some(MergeabilityStatus::Mergeable),
        ..CachedMrData::default()
    }
}
#[test]
fn an_open_mr_rereads_its_review_only_on_a_sign_of_change() {
    let recent = Some(chrono::TimeDelta::minutes(10));
    let ok = MergeabilityStatus::Mergeable;
    // Nothing moved: cache kept.
    assert!(!review_stale(&reviewed(0), true, Some(true), ok, recent));
    assert!(!review_stale(&reviewed(2), true, Some(false), ok, recent));
    // updated_at or the note count moved.
    assert!(review_stale(&reviewed(0), false, Some(true), ok, recent));
    // A thread was resolved / opened without any new note.
    assert!(review_stale(&reviewed(2), true, Some(true), ok, recent));
    assert!(review_stale(&reviewed(0), true, Some(false), ok, recent));
    // The mergeability moved (an approval rule may now be met).
    assert!(review_stale(
        &reviewed(0),
        true,
        Some(true),
        MergeabilityStatus::NotApproved,
        recent
    ));
    // Safety net: read again once an hour whatever happens.
    assert!(review_stale(
        &reviewed(0),
        true,
        Some(true),
        ok,
        Some(REVIEW_MAX_AGE)
    ));
    assert!(review_stale(&reviewed(0), true, Some(true), ok, None));
    // Never read, or a manual [R].
    assert!(review_stale(
        &CachedMrData::default(),
        true,
        None,
        ok,
        recent
    ));
    let forced = CachedMrData {
        cache_policy: CachePolicy::ForceAll,
        ..reviewed(0)
    };
    assert!(review_stale(&forced, true, Some(true), ok, recent));
}
fn at(rfc3339: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .with_timezone(&chrono::Utc)
}
#[test]
fn a_branch_scan_reads_only_what_arrived_since_the_last_one() {
    let now = at("2026-11-10T12:00:00Z");
    let floor = at("2026-10-01T00:00:00Z");
    let full = BranchScan::Full {
        since: floor.to_rfc3339(),
    };
    assert_eq!(plan_branch_scan(None, floor, now), full);
    let scope = BranchScanScope {
        floor,
        head: Some("abc".into()),
        full_at: now - chrono::TimeDelta::hours(2),
    };
    assert_eq!(
        plan_branch_scan(Some(&scope), floor, now),
        BranchScan::Range { from: "abc".into() }
    );
    // A newer floor is still covered by the index.
    assert_eq!(
        plan_branch_scan(Some(&scope), floor + chrono::TimeDelta::days(3), now),
        BranchScan::Range { from: "abc".into() }
    );
    // Older MRs to cover, no known head, or a day old: rebuilt in full.
    let older = floor - chrono::TimeDelta::days(1);
    assert!(matches!(
        plan_branch_scan(Some(&scope), older, now),
        BranchScan::Full { .. }
    ));
    let headless = BranchScanScope {
        head: None,
        ..scope.clone()
    };
    assert_eq!(plan_branch_scan(Some(&headless), floor, now), full);
    let stale = BranchScanScope {
        full_at: now - BRANCH_FULL_RESCAN,
        ..scope
    };
    assert_eq!(plan_branch_scan(Some(&stale), floor, now), full);
}
#[test]
fn a_range_scan_asks_gitlab_for_a_revision_range() {
    let url = branch_scan_url(
        &ctx("https://gl.test"),
        "release/1.2",
        &BranchScan::Range { from: "abc".into() },
    );
    assert_eq!(
        url,
        "https://gl.test/api/v4/projects/1/repository/commits?ref_name=abc..release%2F1.2"
    );
    let url = branch_scan_url(
        &ctx("https://gl.test"),
        "main",
        &BranchScan::Full {
            since: "2026-10-01T00:00:00+00:00".into(),
        },
    );
    assert!(url.ends_with("ref_name=main&since=2026-10-01T00%3A00%3A00%2B00%3A00"));
}
fn tracked(
    id: &str,
    state: GitlabMrState,
    mergeability: MergeabilityStatus,
) -> crate::models::TrackedMr {
    let mut mr = crate::models::TrackedMr::placeholder(id.into(), "t".into(), None);
    mr.state = state;
    mr.mergeability = mergeability;
    mr
}
#[test]
fn the_estimate_counts_what_list_reads_and_review_caching_save() {
    use GitlabMrState::{Merged, Opened};
    // Open, mergeable, reviewed a minute ago: no page, no discussions, no approvals.
    let mut mr = tracked("1", Opened, MergeabilityStatus::Mergeable);
    mr.review = Some(ReviewActivity {
        version: crate::models::REVIEW_ACTIVITY_VERSION,
        ..ReviewActivity::default()
    });
    mr.review_read_at = Some(chrono::Utc::now().to_rfc3339());
    mr.milestone_events = Some(Vec::new());
    let e = mr.estimate(&Opened, false);
    assert_eq!((e.base, e.discussions, e.saved), (0, 0, 3));
    // Its milestone history never read: one read more.
    mr.milestone_events = None;
    assert_eq!(mr.estimate(&Opened, false).discussions, 1);
    mr.milestone_events = Some(Vec::new());
    // Same MR reviewed two hours ago: discussions + approvals again.
    mr.review_read_at = Some((chrono::Utc::now() - chrono::TimeDelta::hours(2)).to_rfc3339());
    let e = mr.estimate(&Opened, false);
    assert_eq!((e.base, e.discussions, e.saved), (0, 2, 1));
    // Needs a rebase: its page is read for the commits behind.
    let e = tracked("2", Opened, MergeabilityStatus::NeedsRebase).estimate(&Opened, false);
    assert_eq!((e.base, e.saved), (1, 0));
    // Merged: its page comes from the list.
    let e = tracked("3", Merged, MergeabilityStatus::NotOpen).estimate(&Merged, true);
    assert_eq!((e.base, e.saved), (0, 1));
}
#[tokio::test]
async fn a_refresh_reads_the_mrs_in_one_list_and_skips_unchanged_reviews() {
    const UPDATED: &str = "2026-11-10T08:00:00Z";
    fn route(path: &str, _body: &str) -> String {
        let p = path.split('?').next().unwrap();
        if p.ends_with("/merge_requests") {
            serde_json::json!([
                {"iid": 1, "title": "merged", "state": "merged",
                 "detailed_merge_status": "not_open", "merge_commit_sha": "m1"},
                {"iid": 2, "title": "ready", "state": "opened",
                 "detailed_merge_status": "mergeable", "updated_at": UPDATED,
                 "user_notes_count": 3, "blocking_discussions_resolved": true},
                {"iid": 3, "title": "behind", "state": "opened",
                 "detailed_merge_status": "need_rebase"},
            ])
            .to_string()
        } else if p.ends_with("/merge_requests/3") {
            serde_json::json!({"iid": 3, "title": "behind", "state": "opened",
                "detailed_merge_status": "need_rebase", "diverged_commits_count": 2})
            .to_string()
        } else if p.ends_with("/approvals") {
            r#"{"approved_by": []}"#.into()
        } else if p.ends_with("/changes") {
            r#"{"changes": []}"#.into()
        } else {
            "[]".into()
        }
    }
    let (url, seen) = fake_gitlab(route).await;
    let unchanged_review = CachedMrData {
        updated_at: Some(UPDATED.into()),
        cached_state: Some(GitlabMrState::Opened),
        review: Some(ReviewActivity {
            version: crate::models::REVIEW_ACTIVITY_VERSION,
            ..ReviewActivity::default()
        }),
        review_read_at: Some(chrono::Utc::now().to_rfc3339()),
        notes_seen: Some(3),
        mergeability: Some(MergeabilityStatus::Mergeable),
        ..CachedMrData::default()
    };
    let mrs = vec![
        ("1".to_string(), CachedMrData::default()),
        ("2".to_string(), unchanged_review),
        ("3".to_string(), CachedMrData::default()),
    ];
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_mr_fetches(ctx(&url), mrs, Arc::new(Semaphore::new(3)), tx);

    let mut loaded = HashMap::new();
    while loaded.len() < 3 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("every MR is loaded")
            .unwrap();
        if let GitlabEvent::MrLoaded(data) = event {
            loaded.insert(data.id.clone(), data);
        }
    }
    let seen = seen.lock().unwrap().clone();
    let count = |suffix: &str| {
        seen.iter()
            .filter(|p| p.split('?').next().unwrap().ends_with(suffix))
            .count()
    };
    assert_eq!(count("/merge_requests"), 1, "{seen:#?}");
    // Pages: only the open MR that cannot merge, for its commits behind.
    assert_eq!(
        count("/merge_requests/1") + count("/merge_requests/2"),
        0,
        "{seen:#?}"
    );
    assert_eq!(count("/merge_requests/3"), 1);
    let behind = loaded["3"]
        .data
        .diff_stats
        .as_ref()
        .and_then(|d| d.commits_behind);
    assert_eq!(behind, Some(2));
    // Unchanged open MR: review kept. Never-read open or merged MR: read, the
    // merged one too (approved and merged between two refreshes).
    assert_eq!(
        count("/merge_requests/2/discussions") + count("/merge_requests/2/approvals"),
        0
    );
    assert_eq!(count("/merge_requests/3/discussions"), 1);
    assert_eq!(count("/merge_requests/3/approvals"), 1);
    assert_eq!(count("/merge_requests/1/approvals"), 1);
    assert_eq!(loaded["3"].data.notes_seen, None); // the list gave no count for it
    assert!(loaded["3"].data.review_read_at.is_some());
}
#[tokio::test]
async fn milestone_history_is_read_again_only_when_the_milestone_moved() {
    const UPDATED: &str = "2026-11-10T08:00:00Z";
    fn route(path: &str, _body: &str) -> String {
        let p = path.split('?').next().unwrap();
        if p.ends_with("/merge_requests") {
            serde_json::json!([
                {"iid": 1, "title": "moved", "state": "merged", "updated_at": UPDATED,
                 "milestone": {"title": "1.5"}},
                {"iid": 2, "title": "kept", "state": "merged", "updated_at": UPDATED,
                 "milestone": {"title": "1.4"}},
            ])
            .to_string()
        } else if p.ends_with("/resource_milestone_events") {
            serde_json::json!([
                {"created_at": "2026-11-09T10:00:00Z", "action": "add",
                 "milestone": {"title": "1.5"}, "user": {"username": "bob", "name": "Bob"}},
                {"created_at": "2026-11-01T10:00:00Z", "action": "add",
                 "milestone": {"title": "1.4"}, "user": {"username": "ann", "name": "Ann"}},
            ])
            .to_string()
        } else if p.ends_with("/changes") {
            r#"{"changes": []}"#.into()
        } else {
            "[]".into()
        }
    }
    let (url, seen) = fake_gitlab(route).await;
    let read_with_1_4 = CachedMrData {
        updated_at: Some(UPDATED.into()),
        cached_state: Some(GitlabMrState::Merged),
        milestone: Some("1.4".into()),
        milestone_events: Some(Vec::new()),
        ..CachedMrData::default()
    };
    let mrs = vec![
        ("1".to_string(), read_with_1_4.clone()),
        ("2".to_string(), read_with_1_4),
    ];
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_mr_fetches(ctx(&url), mrs, Arc::new(Semaphore::new(3)), tx);
    let mut loaded = HashMap::new();
    while loaded.len() < 2 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("every MR is loaded")
            .unwrap();
        if let GitlabEvent::MrLoaded(data) = event {
            loaded.insert(data.id.clone(), data);
        }
    }
    let seen = seen.lock().unwrap().clone();
    let events_read = |id: &str| {
        seen.iter()
            .filter(|p| p.contains(&format!("/merge_requests/{id}/resource_milestone_events")))
            .count()
    };
    assert_eq!((events_read("1"), events_read("2")), (1, 0), "{seen:#?}");
    // Read oldest first, the author as "Name (@login)".
    let events = loaded["1"].data.milestone_events.clone().unwrap();
    let titles: Vec<_> = events.iter().map(|e| e.milestone.as_deref()).collect();
    assert_eq!(titles, [Some("1.4"), Some("1.5")]);
    assert_eq!(events[1].by.as_deref(), Some("Bob (@bob)"));
    assert_eq!(loaded["2"].data.milestone_events, Some(Vec::new()));
}
#[tokio::test]
async fn settled_mrs_are_read_again_only_when_they_changed() {
    fn route(path: &str, _body: &str) -> String {
        let p = path.split('?').next().unwrap();
        assert!(p.ends_with("/merge_requests"), "only the list is read: {p}");
        serde_json::json!([
            {"iid": 1, "title": "same", "state": "merged", "updated_at": "t1",
             "milestone": {"title": "1.4"}},
            {"iid": 2, "title": "touched", "state": "merged", "updated_at": "t2-new",
             "milestone": {"title": "1.4"}},
            {"iid": 3, "title": "milestone only", "state": "merged", "updated_at": "t3"},
        ])
        .to_string()
    }
    let (url, seen) = fake_gitlab(route).await;
    let settled = |id: &str, updated: &str, events_read| SettledMr {
        id: id.into(),
        updated_at: Some(updated.into()),
        milestone: Some("1.4".into()),
        events_read,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_settled_check(
        ctx(&url),
        vec![
            settled("1", "t1", true),
            settled("2", "t2", true),
            settled("3", "t3", true),
            // History never read: refreshed, without being listed.
            settled("4", "t4", false),
        ],
        Arc::new(Semaphore::new(3)),
        tx,
    );
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("a change is reported")
        .unwrap();
    let GitlabEvent::SettledMrsChanged(ids) = event else {
        panic!("unexpected event");
    };
    assert_eq!(ids, ["2", "3", "4"]);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:#?}");
    assert!(!seen[0].contains("iids[]=4"), "{seen:#?}");
}
#[tokio::test]
async fn an_mr_missing_from_the_list_reads_its_own_page() {
    fn route(path: &str, _body: &str) -> String {
        let p = path.split('?').next().unwrap();
        if p.ends_with("/merge_requests") {
            "[]".into() // e.g. not visible through the list
        } else if p.ends_with("/merge_requests/9") {
            r#"{"iid": 9, "title": "alone", "state": "closed"}"#.into()
        } else if p.ends_with("/changes") {
            r#"{"changes": []}"#.into()
        } else {
            "[]".into()
        }
    }
    let (url, seen) = fake_gitlab(route).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_mr_fetches(
        ctx(&url),
        vec![("9".to_string(), CachedMrData::default())],
        Arc::new(Semaphore::new(3)),
        tx,
    );
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, GitlabEvent::MrLoaded(data) if data.data.title == "alone"));
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().any(|p| p.contains("/merge_requests/9?")),
        "{seen:#?}"
    );
}
