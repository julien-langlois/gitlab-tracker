//! Reading GitLab: fetches, refresh cycles, the sentinel, the request counter.

use super::*;

#[tokio::test]
async fn failed_fetch_of_untracked_mr_releases_spinner() {
    let mut app = test_app();
    app.pending_initial_fetches.insert("1".into());
    app.pending_refresh_fetches
        .extend(["1".to_string(), "2".to_string()]);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // MR "1" is not in `app.mrs` (removed while its fetch was in flight):
    // the failure must still release it, and only it.
    app.apply_event(
        AppEvent::MrFailed {
            id: "1".into(),
            error: crate::gitlab::GitlabError::Cancelled,
        },
        Arc::new(Semaphore::new(1)),
        &tx,
        &mut HashMap::new(),
    );

    assert!(app.pending_initial_fetches.is_empty());
    assert_eq!(app.pending_refresh_fetches.len(), 1);
}
#[tokio::test]
async fn time_log_caches_failures_until_the_next_cycle() {
    use gitlab_tracker_core::TrackerError;
    let fake = Arc::new(FakeTracker::default());
    fake.time_entries
        .lock()
        .unwrap()
        .extend([Err(TrackerError::Auth("HTTP 401".into())), Ok(vec![])]);
    let mut app = app_with_mrs(&[("t", "None")]).await;
    app.mrs[0].linked_ticket = Some(
        serde_json::from_value(serde_json::json!({
            "schema_version": gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION,
            "id": "42", "subject": "s", "status": "New", "url": "u"
        }))
        .unwrap(),
    );
    app.tracker = Some(fake.clone());
    app.tracker_view = TrackerView::TimeLog;
    app.table_state.select(Some(0));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let calls = || {
        fake.time_entry_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    };

    // Several main-loop iterations while the first request is in flight: one call.
    app.ensure_time_entries(&tx);
    app.ensure_time_entries(&tx);
    let event = rx.recv().await.unwrap();
    app.apply_event(event, Arc::new(Semaphore::new(1)), &tx, &mut HashMap::new());
    assert_eq!(calls(), 1);
    assert!(
        matches!(app.time_entries.get("42"), Some(TimeLogState::Failed(e)) if e.contains("401"))
    );

    // The failure is shown, not retried on every loop iteration…
    app.ensure_time_entries(&tx);
    assert_eq!(calls(), 1);

    // …until the next refresh cycle / [R] clears the cache.
    app.time_entries.clear();
    app.ensure_time_entries(&tx);
    let event = rx.recv().await.unwrap();
    app.apply_event(event, Arc::new(Semaphore::new(1)), &tx, &mut HashMap::new());
    assert_eq!(calls(), 2);
    assert!(matches!(app.time_entries.get("42"), Some(TimeLogState::Loaded(e)) if e.is_empty()));
}
#[tokio::test]
async fn read_only_mode_never_fetches_or_mutates() {
    let mut app = app_with_mrs(&[("t", "None")]).await;
    app.read_only = true;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    while rx.try_recv().is_ok() {} // setup events

    // [R]: the countdown restarts, no MR is refetched.
    app.time_left = 3;
    press(&mut app, &tx, KeyCode::Char('r')).await;
    assert_eq!(app.time_left, app.refresh_interval_secs);
    assert!(app.pending_refresh_fetches.is_empty());

    // Typing an MR id or a branch in the input: nothing is added.
    app.input_mode = InputMode::Editing;
    app.input = "999".into();
    press(&mut app, &tx, KeyCode::Enter).await;
    app.input_mode = InputMode::Editing;
    app.input = "release".into();
    press(&mut app, &tx, KeyCode::Enter).await;
    assert!(rx.try_recv().is_err(), "no MrAdded sent");
    assert!(!app.branches.contains(&"release".to_string()));
}
#[tokio::test]
async fn failed_fetch_keeps_the_title() {
    let mut app = app_with_mrs(&[("Fix login", "None")]).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let error = AppEvent::MrFailed {
        id: "1".into(),
        error: crate::gitlab::GitlabError::Http(reqwest::StatusCode::BAD_GATEWAY),
    };
    let persist = app.apply_event(error, Arc::new(Semaphore::new(1)), &tx, &mut HashMap::new());
    assert_eq!(
        app.mrs[0].title, "Fix login",
        "the error is not written in the title"
    );
    assert_eq!(
        app.mrs[0].status,
        MrStatus::Error("HTTP 502 Bad Gateway".into())
    );
    assert!(!persist, "errors are not persisted");
}
#[test]
fn no_refresh_cycle_while_the_previous_one_runs() {
    let mut app = test_app();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    app.pending_branch_scans = 1;
    assert!(app.refresh_in_flight());
    app.time_left = 0;
    app.on_tick(&Arc::new(Semaphore::new(1)), &tx);
    assert!(app.refresh_cycle.is_none(), "cycle skipped");
    assert_eq!(app.time_left, app.refresh_interval_secs, "re-armed");
    app.pending_branch_scans = 0;
    assert!(!app.refresh_in_flight());
}
#[tokio::test]
async fn the_sentinel_checks_on_its_countdown_but_not_during_a_refresh() {
    let mut app = test_app();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    // No permit: a check started here never reaches the network.
    let blocked = Arc::new(Semaphore::new(0));
    app.time_left = 100;
    app.sentinel_left = 1;
    app.on_tick(&blocked, &tx);
    assert!(!app.sentinel.in_flight);
    assert_eq!(app.sentinel_left, 0);

    app.pending_branch_scans = 1;
    app.on_tick(&blocked, &tx);
    assert!(!app.sentinel.in_flight, "a refresh is running");
    assert_eq!(app.sentinel_left, app.sentinel_interval_secs, "re-armed");

    app.pending_branch_scans = 0;
    app.sentinel_left = 0;
    app.on_tick(&blocked, &tx);
    assert!(app.sentinel.in_flight);

    // Off: the countdown stays still.
    let mut app = test_app();
    app.sentinel_interval_secs = 0;
    app.sentinel_left = 0;
    app.on_tick(&blocked, &tx);
    assert!(!app.sentinel.in_flight);
}
#[tokio::test]
async fn an_old_sentinel_watermark_runs_a_full_refresh_instead() {
    let mut app = test_app();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let now = chrono::Utc::now();
    app.sentinel.since = now - chrono::TimeDelta::hours(1);
    app.sentinel_left = 0;
    app.time_left = 100;
    app.on_tick(&Arc::new(Semaphore::new(0)), &tx);
    assert!(!app.sentinel.in_flight);
    assert_eq!(
        app.time_left, app.refresh_interval_secs,
        "the full cycle ran"
    );
    assert!(app.sentinel.since > now - chrono::TimeDelta::minutes(1));
}
#[tokio::test]
async fn a_sentinel_report_rereads_only_the_changed_mrs() {
    use crate::sentinel::{ListedMr, SentinelReport, OVERLAP};
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None")]).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    app.pending_initial_fetches.clear();
    app.pending_refresh_fetches.clear();
    for mr in &mut app.mrs {
        mr.updated_at = Some("2026-11-23T09:00:00Z".into());
        mr.status = MrStatus::MergedIn(Default::default());
    }
    app.sentinel.in_flight = true;
    let now = chrono::Utc::now();
    let report = SentinelReport {
        local_now: now,
        server_now: Some(now),
        truncated: false,
        mrs: vec![
            ListedMr {
                iid: 1,
                updated_at: Some("2026-11-23T09:00:00Z".into()),
            },
            ListedMr {
                iid: 2,
                updated_at: Some("2026-11-23T10:00:00Z".into()),
            },
        ],
        pipelines: Vec::new(),
        events: Vec::new(),
    };
    app.apply_event(
        AppEvent::SentinelChecked(Some(Box::new(report))),
        Arc::new(Semaphore::new(0)),
        &tx,
        &mut HashMap::new(),
    );
    assert!(!app.sentinel.in_flight);
    assert_eq!(
        app.pending_refresh_fetches,
        HashSet::from(["2".to_string()])
    );
    assert!(matches!(app.mrs[0].status, MrStatus::MergedIn(_)));
    assert_eq!(app.sentinel.since, now - OVERLAP);

    // A failed check keeps the watermark.
    app.sentinel.in_flight = true;
    app.apply_event(
        AppEvent::SentinelChecked(None),
        Arc::new(Semaphore::new(0)),
        &tx,
        &mut HashMap::new(),
    );
    assert!(!app.sentinel.in_flight);
    assert_eq!(app.sentinel.since, now - OVERLAP);
}
#[test]
fn refresh_cycle_is_timed_until_its_last_mr() {
    let mut app = test_app();
    app.pending_refresh_fetches
        .extend(["1".to_string(), "2".to_string()]);
    app.start_refresh_cycle("manual");
    assert!(app.refresh_cycle.as_ref().is_some_and(|c| c.mrs == 2));
    app.complete_fetch("1");
    assert!(app.refresh_cycle.is_some(), "one MR still in flight");
    app.complete_fetch("2");
    assert!(
        app.refresh_cycle.is_none(),
        "logged and cleared on the last MR"
    );
    // Nothing in flight: nothing to time.
    app.start_refresh_cycle("auto");
    assert!(app.refresh_cycle.is_none());
}
#[test]
fn complete_fetch_flags() {
    let mut app = test_app();
    app.pending_initial_fetches
        .extend(["1".to_string(), "2".to_string()]);

    app.pending_refresh_fetches.insert("3".into());
    let done = |notify_allowed, initial_sync_just_completed, from_refresh_cycle| FetchCompletion {
        notify_allowed,
        initial_sync_just_completed,
        from_refresh_cycle,
    };

    // An untracked fetch (e.g. MrAdded) must not release the startup fence.
    assert_eq!(app.complete_fetch("99"), done(false, false, false));
    assert_eq!(app.complete_fetch("1"), done(false, false, false));
    // Last startup fetch: completes the initial sync, notifications still off for it.
    assert_eq!(app.complete_fetch("2"), done(false, true, false));
    // Refresh-cycle fetch: its ticket was already re-fetched by the cycle.
    assert_eq!(app.complete_fetch("3"), done(true, false, true));
}
#[tokio::test]
async fn the_request_counter_shows_what_bulk_reads_save() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None")]).await;
    let ticket = |id: &str| -> gitlab_tracker_core::LinkedTicket {
        serde_json::from_value(serde_json::json!({
            "schema_version": gitlab_tracker_core::LINKED_TICKET_SCHEMA_VERSION,
            "id": id, "subject": "s", "status": "New", "url": "u"
        }))
        .unwrap()
    };
    // Two MRs on the same ticket: one read for both.
    app.mrs[0].linked_ticket = Some(ticket("42"));
    app.mrs[1].linked_ticket = Some(ticket("42"));
    app.mrs[2].linked_ticket = Some(ticket("7"));
    for mr in &mut app.mrs {
        mr.state = GitlabMrState::Closed;
        mr.review = Some(Default::default());
    }
    app.tracker = Some(Arc::new(FakeTracker::default()));
    app.recompute_api_call_estimate();

    // Three closed MRs: one list read instead of three pages, and one GraphQL
    // query instead of 3 × (pipelines + jobs) REST reads (no cache yet).
    assert_eq!(app.saved_gitlab_calls, 2 + 5);
    assert_eq!(app.estimated_tracker_calls, 2);
    assert_eq!(app.saved_tracker_calls, 1);
    assert_eq!(saved_percent(2, 1), 33);
    assert_eq!(saved_percent(0, 0), 0);
}
