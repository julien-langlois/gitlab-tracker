use super::ticks::FLASH_TICKS;
use super::*;
use crate::models::Pipeline;
use crossterm::event::KeyCode;

/// An empty app on project `1`, shared with the other modules' tests.
pub(crate) fn test_app() -> App {
    App::new(AppInit {
        token: std::sync::Arc::new("t".to_string().into()),
        project_id: "1".into(),
        base_url: "https://gitlab.example".into(),
        project_name: None,
        refresh_interval_secs: 900,
        sentinel_interval_secs: 120,
        config: AppConfig::default(),
        theme: crate::ui::theme::Palette::for_mode(crate::ui::theme::ThemeMode::Dark),
        project_settings: ProjectEntry {
            name: None,
            gitlab_url: "https://gitlab.example".into(),
            project_id: "1".into(),
            active: true,
            default_branches: None,
            table_label_prefixes: None,
            complexity_profile: None,
            tracked_branches: None,
            branch_flow: None,
            refresh_interval_secs: None,
            sentinel_interval_secs: None,
            activity_stale_days: None,
            activity_recent_days: None,
            review_reminder_hours: None,
            hotfix_branches: None,
            release_note_tickets: None,
            freeze_periods: None,
            show_cockpit: None,
            hide_merged: None,
            cockpit_thresholds: None,
            stats: None,
            visible_columns: None,
            views: Vec::new(),
            label_colors: None,
            signed_commits: None,
            tracker: None,
            gitlab_username: None,
            discover_new_mrs: None,
        },
    })
}

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

/// Tracks MRs "1".."n" with the given `(title, milestone)` pairs.
pub(crate) async fn app_with_mrs(mrs: &[(&str, &str)]) -> App {
    let mut app = test_app();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    for (i, _) in mrs.iter().enumerate() {
        app.apply_event(
            AppEvent::MrAdded((i + 1).to_string()),
            Arc::new(Semaphore::new(1)),
            &tx,
            &mut HashMap::new(),
        );
    }
    // Built outside an event drain: no selection move left pending.
    app.selection_moved = false;
    for (mr, (title, milestone)) in app.mrs.iter_mut().zip(mrs) {
        mr.title = (*title).into();
        // Test shorthand: "None" means no milestone.
        mr.milestone = Some(*milestone).filter(|m| *m != "None").map(Into::into);
    }
    app
}

#[tokio::test]
async fn the_status_bar_counts_the_list_whatever_status_is_picked() {
    use crate::status::StatusKind;
    let mut app = app_with_mrs(&[("fix a", "None"), ("fix b", "None"), ("feat c", "None")]).await;
    app.mrs[0].mergeability = MergeabilityStatus::Conflict;
    app.mrs[1].mergeability = MergeabilityStatus::Conflict;
    app.mrs[2].mergeability = MergeabilityStatus::Mergeable;

    app.set_status_pick(Some(StatusKind::Conflict));
    assert_eq!(app.visible_mrs().count(), 2);
    // The picked status narrows the list, not the counts.
    assert_eq!(
        app.status_counts(),
        [(StatusKind::Mergeable, 1), (StatusKind::Conflict, 2)]
    );

    // The search narrows both.
    app.input = "feat".into();
    assert_eq!(app.status_counts(), [(StatusKind::Mergeable, 1)]);
    assert_eq!(app.visible_mrs().count(), 0);
}

fn ids(app: &App) -> Vec<&str> {
    app.mrs.iter().map(|mr| mr.id.as_str()).collect()
}

#[tokio::test]
async fn sort_keys_and_stable_descending() {
    let mut app = app_with_mrs(&[("beta", "None"), ("Alpha", "v2"), ("beta", "V1")]).await;

    app.sort_column = SortColumn::Title;
    app.sort_order = SortOrder::Ascending;
    app.sort_mrs();
    assert_eq!(ids(&app), ["2", "1", "3"]); // case-insensitive, ties keep order

    app.sort_order = SortOrder::Descending;
    app.sort_mrs();
    assert_eq!(ids(&app), ["1", "3", "2"]); // ties still keep order
    app.sort_mrs();
    assert_eq!(ids(&app), ["1", "3", "2"]); // re-sorting never swaps ties

    app.sort_column = SortColumn::Milestone;
    app.sort_order = SortOrder::Ascending;
    app.sort_mrs();
    assert_eq!(ids(&app), ["3", "2", "1"]); // "None" last

    app.sort_column = SortColumn::Id;
    app.sort_order = SortOrder::Descending;
    app.sort_mrs();
    assert_eq!(ids(&app), ["3", "2", "1"]);
}

#[tokio::test]
async fn render_cache_matches_live_visible_list() {
    let mut app = app_with_mrs(&[("fix login", "None"), ("add stats", "None")]).await;
    app.input = "stats".into();
    let live: Vec<String> = app.visible_mrs().map(|mr| mr.id.clone()).collect();

    app.begin_render_cache();
    let cached: Vec<String> = app.visible_mrs().map(|mr| mr.id.clone()).collect();
    app.end_render_cache();

    assert_eq!(live, ["2"]);
    assert_eq!(cached, live);
}

#[tokio::test]
async fn branch_presence_reports_branch_commits_separately() {
    let mut app = app_with_mrs(&[("fix login", "None")]).await;
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[0].data.sha = Some("abc".into());
    let picked = BranchCommits {
        picked_from: HashMap::from([("abc".to_string(), 0)]),
        ..BranchCommits::default()
    };
    app.branch_commits.insert("staging".into(), picked);
    let mr = &app.mrs[0];

    assert_eq!(
        app.branch_presence(mr, "main"),
        Some(BranchPresence::Present)
    );
    assert_eq!(
        app.branch_presence(mr, "staging"),
        Some(BranchPresence::CherryPicked)
    );
    assert_eq!(
        app.branch_presence(mr, "prod"),
        Some(BranchPresence::Absent)
    );
}

#[tokio::test]
async fn behind_on_lists_the_merges_of_the_parent_missing_from_the_feature() {
    let mut app = app_with_mrs(&[
        ("on develop", "None"),
        ("synced", "None"),
        ("hotfix", "None"),
    ])
    .await;
    app.branches = ["master", "develop", "redesign_develop"]
        .map(String::from)
        .to_vec();
    let merged = |found: &[&str]| MrStatus::MergedIn(found.iter().map(|b| b.to_string()).collect());
    app.mrs[0].status = merged(&["develop"]);
    app.mrs[0].data.target_branch = "develop".into();
    app.mrs[1].status = merged(&["develop", "redesign_develop"]);
    app.mrs[1].data.target_branch = "develop".into();
    app.mrs[2].status = merged(&["master"]);
    app.mrs[2].data.target_branch = "master".into();
    for mr in &mut app.mrs {
        mr.data.state = GitlabMrState::Merged;
    }
    // Loaded (so `MergedIn`) but still in review, or closed: nothing to bring in.
    for (title, state) in [
        ("in review", GitlabMrState::Opened),
        ("closed", GitlabMrState::Closed),
    ] {
        let mut mr = app.mrs[0].clone();
        mr.data.title = title.into();
        mr.data.state = state;
        mr.status = merged(&[]);
        app.mrs.push(mr);
    }

    let ids = |mrs: Vec<&TrackedMr>| mrs.iter().map(|mr| mr.title.clone()).collect::<Vec<_>>();
    // The hotfix comes down to develop, then to the feature branch.
    assert_eq!(
        ids(app.behind_on("redesign_develop")),
        ["on develop", "hotfix"]
    );
    assert_eq!(ids(app.behind_on("develop")), ["hotfix"]);
    assert!(app.behind_on("master").is_empty());
}

#[tokio::test]
async fn markdown_checklist_lists_visible_rows_per_branch() {
    let mut app = app_with_mrs(&[("fix | login", "None"), ("add stats", "None")]).await;
    app.branches = vec!["main".into(), "staging".into()];
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[0].data.web_url = "https://g/mr/1".into();
    app.mrs[1].status = MrStatus::Loading;

    let md = app.markdown_checklist();
    let lines: Vec<&str> = md.lines().collect();
    assert_eq!(lines[0], "| MR | Title | State | main | staging |");
    assert_eq!(lines[1], "|---|---|---|---|---|");
    assert!(lines[2].starts_with("| [!1](https://g/mr/1) | fix \\| login |"));
    assert!(lines[2].ends_with(" ✅ | ❌ |"));
    assert!(lines[3].ends_with(" … | … |"));
}

#[tokio::test]
async fn filters_on_top_of_the_view_must_all_pass() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None"), ("d", "None")]).await;
    app.branches = vec!["main".into(), "staging".into(), "prod".into()];
    let on =
        |branches: &[&str]| MrStatus::MergedIn(branches.iter().map(|b| b.to_string()).collect());
    app.mrs[0].status = on(&["main"]); // absent from staging and prod
    app.mrs[1].status = on(&["main", "staging"]); // absent from prod only
    app.mrs[2].status = on(&["main", "staging", "prod"]);
    app.mrs[3].status = on(&["main"]);
    app.mrs[3].flagged = true;
    let index = |id: &str| app.filter_defs.iter().position(|d| d.id == id).unwrap();
    let absent = index("absent_from_branch");
    let flagged = index("flagged");
    let filter = |index, query: &str| ActiveFilter {
        index,
        query: query.into(),
    };
    app.set_filters(vec![filter(absent, "staging"), filter(absent, "prod")]);
    let visible = |app: &App| -> Vec<String> { app.visible_mrs().map(|m| m.id.clone()).collect() };
    assert_eq!(visible(&app), ["1", "4"]);
    // With a view: both again.
    app.set_filter(flagged, String::new());
    assert_eq!(visible(&app), ["4"]);
    app.set_filters(Vec::new());
    assert_eq!(visible(&app), ["4"]);
}

#[tokio::test]
async fn absent_from_branch_filter_skips_present_picked_and_loading() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None"), ("d", "None")]).await;
    app.branches = vec!["main".into(), "prod".into()];
    let on_main = || MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[0].status = on_main(); // absent from prod
    app.mrs[1].status = MrStatus::MergedIn(HashSet::from(["main".into(), "prod".into()]));
    app.mrs[2].status = on_main(); // cherry-picked to prod
    app.mrs[2].data.sha = Some("abc".into());
    let picked = BranchCommits {
        picked_from: HashMap::from([("abc".to_string(), 0)]),
        ..BranchCommits::default()
    };
    app.branch_commits.insert("prod".into(), picked);
    app.mrs[3].status = MrStatus::Loading;
    let index = app
        .filter_defs
        .iter()
        .position(|d| d.id == "absent_from_branch")
        .unwrap();
    app.active_filter = ActiveFilter {
        index,
        query: "PROD".into(),
    };

    let visible: Vec<&str> = app.visible_mrs().map(|mr| mr.id.as_str()).collect();
    assert_eq!(visible, ["1"]);
}

#[tokio::test]
async fn only_mrs_that_just_reached_a_branch_are_reported() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None")]).await;
    for (mr, sha) in app.mrs.iter_mut().zip(["s1", "s2", "s3"]) {
        mr.status = MrStatus::MergedIn(HashSet::new());
        mr.data.sha = Some(sha.into());
    }
    app.mrs[2].status = MrStatus::MergedIn(HashSet::from(["prod".to_string()]));
    let picked = |shas: &[&str]| BranchCommits {
        picked_from: shas.iter().map(|s| (s.to_string(), 0)).collect(),
        ..BranchCommits::default()
    };
    let (before, after) = (picked(&["s1"]), picked(&["s1", "s2", "s3"]));

    let reached: Vec<(&str, &str)> = newly_copied(&app.mrs, "prod", &before, &after)
        .map(|(mr, how)| (mr.id.as_str(), how))
        .collect();
    // s1 was already picked; s3's MR is on prod by SHA anyway.
    assert_eq!(reached, [("2", "cherry-picked")]);
}

#[tokio::test]
async fn branch_rows_list_the_commits_missing_from_a_partial_port() {
    use crate::models::{CommitFingerprint, MrCommit};
    let mut app = app_with_mrs(&[("a", "None")]).await;
    app.branches = vec!["main".into(), "prod".into()];
    let commit = |sha: &str| MrCommit {
        sha: sha.into(),
        fingerprint: CommitFingerprint {
            author_email: "dev@example.com".into(),
            authored_at: 0,
            title: sha.into(),
        },
        author_name: String::new(),
    };
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[0].data.commits = vec![commit("c1"), commit("c2")];
    let prod = BranchCommits {
        shas: HashSet::from(["c1".to_string()]),
        ..BranchCommits::default()
    };
    app.branch_commits.insert("prod".into(), prod);

    let rows = app.branch_rows(&app.mrs[0]);
    assert_eq!(rows[0].presence, Some(BranchPresence::Present));
    assert!(rows[0].missing.is_empty());
    assert_eq!(rows[1].presence, Some(BranchPresence::Partial));
    let missing: Vec<&str> = rows[1].missing.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(missing, ["c2"]);
}

#[test]
fn reached_branches_skip_first_sighting_target_and_new_columns() {
    let set = |bs: &[&str]| bs.iter().map(|b| b.to_string()).collect::<HashSet<_>>();
    let found = set(&["develop", "preprod", "master", "release"]);
    let just_added = set(&["release"]);

    assert!(reached_branches(None, &found, "develop", &just_added).is_empty());
    let reached = reached_branches(Some(&set(&["develop"])), &found, "develop", &just_added);
    assert_eq!(
        set(&reached.iter().map(String::as_str).collect::<Vec<_>>()),
        set(&["preprod", "master"])
    );
}

#[tokio::test]
async fn review_filter_lists_my_waiting_reviews_oldest_request_first() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None"), ("d", "None")]).await;
    app.config.gitlab_username = Some("me".into());
    for mr in &mut app.mrs {
        mr.data.reviewers = vec!["Me (@me)".into()];
        mr.mergeability = MergeabilityStatus::NotApproved;
    }
    let requested = |at: &str| {
        let mut review = crate::models::ReviewActivity::default();
        review.review_requested_at.insert("me".into(), at.into());
        Some(review)
    };
    app.mrs[0].data.review = requested("2026-09-03T10:00:00Z");
    app.mrs[1].data.review = requested("2026-09-01T10:00:00Z");
    app.mrs[2].data.approved_by = vec!["me".into()]; // already approved
    app.mrs[3].data.review = requested("2026-09-02T10:00:00Z");
    let index = app
        .filter_defs
        .iter()
        .position(|d| d.id == "reviewer_me")
        .unwrap();
    app.active_filter = ActiveFilter {
        index,
        query: String::new(),
    };

    let visible: Vec<&str> = app.visible_mrs().map(|mr| mr.id.as_str()).collect();
    assert_eq!(visible, ["2", "4", "1"]);
}

#[tokio::test]
async fn merged_by_me_shows_through_hide_merged_latest_merge_first() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None"), ("d", "None")]).await;
    app.config.gitlab_username = Some("jane".into());
    app.config.hide_merged = true;
    let merge = |mr: &mut TrackedMr, at: &str, by: &str| {
        mr.data.state = GitlabMrState::Merged;
        mr.data.merged_at = Some(at.into());
        mr.data.merged_by = Some(by.into());
    };
    // An earlier instant written with another offset: sorted by instant.
    merge(&mut app.mrs[0], "2026-11-20T09:00:00+01:00", "Jane (@jane)");
    merge(&mut app.mrs[1], "2026-11-20T09:30:00+00:00", "Jane (@jane)");
    merge(&mut app.mrs[2], "2026-11-20T12:00:00+01:00", "Bob (@bob)");
    let index = app
        .filter_defs
        .iter()
        .position(|d| d.id == "merged_by_me")
        .unwrap();
    app.set_filter(index, String::new());
    app.sort_column = SortColumn::MergedAt;
    app.sort_order = SortOrder::Descending;
    app.sort_mrs();
    // The latest merge first, whoever merged it; the open MR last.
    assert_eq!(ids(&app), ["3", "2", "1", "4"]);
    let visible: Vec<&str> = app.visible_mrs().map(|mr| mr.id.as_str()).collect();
    assert_eq!(visible, ["2", "1"]);
    assert_eq!(app.filter_count(index), 2);

    // On top of "All", too; "All" alone keeps them hidden.
    let all = app.filter_defs.iter().position(|d| d.id == "all").unwrap();
    app.set_filter(all, String::new());
    assert_eq!(app.visible_mrs().count(), 1);
    app.set_filters(vec![ActiveFilter {
        index,
        query: String::new(),
    }]);
    assert_eq!(app.visible_mrs().count(), 2);

    // Ascending: the open MR still last.
    app.sort_order = SortOrder::Ascending;
    app.sort_mrs();
    assert_eq!(ids(&app), ["1", "2", "3", "4"]);
}

#[tokio::test]
async fn flag_toggles_the_selected_row_while_searching() {
    let mut app = app_with_mrs(&[("fix login", "None"), ("add stats", "None")]).await;
    app.input = "stats".into();
    app.table_state.select(Some(0));

    assert_eq!(app.toggle_flag_selected().as_deref(), Some("2"));
    assert!(app.mrs[1].flagged && !app.mrs[0].flagged);
}

#[tokio::test]
async fn session_digest_counts_what_changed_while_closed() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None")]).await;
    let opened = |branches: &[&str]| MrBaseline {
        state: GitlabMrState::Opened,
        branches: branches.iter().map(|b| b.to_string()).collect(),
        pipeline: Some(PipelineState::Success),
    };
    let baseline = HashMap::from([
        ("1".to_string(), opened(&[])),
        ("2".to_string(), opened(&["main"])),
        ("3".to_string(), opened(&[])),
    ]);
    // !1 got merged and reached main + staging; !2 now fails CI; !3 is unchanged.
    app.mrs[0].data.state = GitlabMrState::Merged;
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".into(), "staging".into()]));
    app.mrs[1].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[1].data.pipelines = vec![Pipeline {
        id: 1,
        status: PipelineState::Failed,
        created_at: None,
        jobs: vec![],
        ..Pipeline::default()
    }];
    app.mrs[2].status = MrStatus::MergedIn(HashSet::new());

    assert_eq!(
        session_digest(&baseline, &app.mrs).as_deref(),
        Some("Since last session: 1 merged · 2 new on a branch · 1 CI failing")
    );
    assert_eq!(session_digest(&baseline, &app.mrs[2..]), None);
    // The same changes one by one: as many as the digest counts.
    let changes = session_changes(&baseline, &app.mrs);
    assert_eq!(changes.len(), 4);
    assert_eq!(changes[0].kind, ChangeKind::Merged);
    assert!(changes
        .iter()
        .any(|c| c.id == "2" && c.kind == ChangeKind::CiFailing));
    assert!(session_changes(&baseline, &app.mrs[2..]).is_empty());
}

#[test]
fn flash_message_expires_after_a_few_ticks() {
    let mut app = test_app();
    app.flash("done", false);
    for _ in 1..FLASH_TICKS {
        app.tick_flash();
    }
    assert!(app.flash.is_some());
    app.tick_flash();
    assert!(app.flash.is_none());
}

#[tokio::test]
async fn hide_merged_keeps_merged_mrs_tracked_but_invisible() {
    let mut app = app_with_mrs(&[("fix login", "None"), ("add stats", "None")]).await;
    app.mrs[0].state = GitlabMrState::Merged;
    app.config.hide_merged = true;

    let visible: Vec<&str> = app.visible_mrs().map(|mr| mr.id.as_str()).collect();
    assert_eq!(visible, ["2"]);
    assert_eq!(app.mrs.len(), 2);
}

#[tokio::test]
async fn an_mr_merged_under_hide_merged_stays_selected_until_another_is() {
    let mut app =
        app_with_mrs(&[("fix login", "None"), ("add stats", "None"), ("ci", "None")]).await;
    app.config.hide_merged = true;
    app.table_state.select(Some(1));
    let selected = app.selected_id();
    assert_eq!(selected.as_deref(), Some("2"));

    // A refresh sees it merged: it stays listed and selected (the MR page with it).
    app.mrs[1].state = GitlabMrState::Merged;
    app.keep_selection(selected);
    assert_eq!(app.selected_id().as_deref(), Some("2"));
    assert_eq!(app.visible_mrs().count(), 3);

    // Selecting the next MR lets it go, the selection staying on the one picked.
    app.next_row();
    app.release_kept();
    assert_eq!(app.selected_id().as_deref(), Some("3"));
    let visible: Vec<&str> = app.visible_mrs().map(|mr| mr.id.as_str()).collect();
    assert_eq!(visible, ["1", "3"]);
}

#[tokio::test]
async fn an_mr_hidden_above_the_selection_does_not_move_it() {
    let mut app =
        app_with_mrs(&[("fix login", "None"), ("add stats", "None"), ("ci", "None")]).await;
    app.config.hide_merged = true;
    app.table_state.select(Some(2));
    let selected = app.selected_id();

    app.mrs[0].state = GitlabMrState::Merged;
    app.keep_selection(selected);
    assert_eq!(app.selected_id().as_deref(), Some("3"));
    assert_eq!(app.visible_mrs().count(), 2);
}

#[tokio::test]
async fn delete_key_removes_selected_visible_mr() {
    let mut app = app_with_mrs(&[
        ("fix login", "None"),
        ("add stats", "None"),
        ("stats view", "None"),
    ])
    .await;
    app.input = "stats".into();
    // MR "1" (= `mrs[0]`) is hidden by the search, so visible row 0 is another MR:
    // the old index-based removal deleted `mrs[0]` instead.
    app.table_state.select(Some(0));
    let victim = app.visible_mrs().next().unwrap().id.clone();
    let survivor = app.visible_mrs().nth(1).unwrap().id.clone();
    assert_ne!(victim, "1");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Delete);
    crate::events::handle_key_event(
        key,
        &mut app,
        &Arc::new(Semaphore::new(1)),
        &tx,
        &mut HashMap::new(),
    )
    .await;
    while let Ok(event) = rx.try_recv() {
        app.apply_event(event, Arc::new(Semaphore::new(1)), &tx, &mut HashMap::new());
    }

    assert_eq!(app.mrs.len(), 2);
    assert!(app.mrs.iter().any(|m| m.id == "1") && app.mrs.iter().any(|m| m.id == survivor));
    assert!(app.dismissed_mr_ids.contains(&victim));
    // The selection is clamped to the (now single-row) visible list.
    assert_eq!(app.table_state.selected(), Some(0));
}

#[cfg(feature = "stats")]
#[tokio::test]
async fn stats_snapshot_stores_sentinels_as_null() {
    use gitlab_tracker_stats::snapshot::SnapshotTrigger;
    let mut app = app_with_mrs(&[("t", "None")]).await;
    let profile = app.config.complexity_profile.clone();
    let mr = &mut app.mrs[0];
    // Placeholder: no assignee, no milestone.
    let snap = mr.stats_snapshot(SnapshotTrigger::OnRefresh, "42", &profile);
    assert_eq!((snap.assignee, snap.milestone), (None, None));

    mr.assignee = Some("Jane (@jane)".into());
    mr.milestone = Some("v1.0".into());
    let snap = mr.stats_snapshot(SnapshotTrigger::OnRefresh, "42", &profile);
    assert_eq!(snap.assignee.as_deref(), Some("Jane (@jane)"));
    assert_eq!(snap.milestone.as_deref(), Some("v1.0"));
    assert_eq!(snap.project_id, "42");
}

#[test]
fn new_branch_notifications_skip_first_sighting_at_startup() {
    let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
    let main = set(&["main"]);
    // First launch, startup sync: silent.
    assert!(branches_to_notify(None, &main, false).is_empty());
    // Known open MR (empty set) that merged while the app was closed: notifies.
    assert_eq!(branches_to_notify(Some(&set(&[])), &main, false), ["main"]);
    // Already known on main: nothing new.
    assert!(branches_to_notify(Some(&main), &main, true).is_empty());
    // MR added after startup: notifies as before.
    assert_eq!(branches_to_notify(None, &main, true), ["main"]);
}

#[test]
fn mouse_focus_follows_rendered_pane_areas() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    let mut app = test_app();
    app.layout.inspector_area = Rect::new(65, 0, 35, 20);
    app.layout.tracker_area = Some(Rect::new(65, 20, 35, 10));
    let mut hover = |column, row| {
        let kind = MouseEventKind::Moved;
        let modifiers = KeyModifiers::NONE;
        crate::events::handle_mouse_event(
            MouseEvent {
                kind,
                column,
                row,
                modifiers,
            },
            &mut app,
        );
        app.active_pane
    };
    assert_eq!(hover(10, 5), ActivePane::Dashboard);
    assert_eq!(hover(70, 19), ActivePane::Inspector);
    // First Tracker row: the old hand-made layout put this in the Inspector.
    assert_eq!(hover(70, 20), ActivePane::Tracker);
}

#[tokio::test]
async fn light_theme_never_uses_dark_only_foregrounds() {
    use crate::ui::theme::{self, Palette, ThemeMode};
    use ratatui::{backend::TestBackend, style::Color, Terminal};

    let mut app = app_with_mrs(&[("fix login", "v1.0"), ("add stats", "None")]).await;
    app.theme = Palette::for_mode(ThemeMode::Light);
    app.table_state.select(Some(0));
    theme::set_light(true);

    let dark = Palette::for_mode(ThemeMode::Dark);
    let dark_only = [
        Color::White,
        dark.fg,
        dark.muted,
        dark.muted_dim,
        dark.muted_hint,
    ];
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    for mode in [InputMode::Normal, InputMode::Help, InputMode::ColumnPicker] {
        app.input_mode = mode;
        terminal
            .draw(|f| crate::ui::render_ui(f, &mut app))
            .unwrap();
        for cell in terminal.backend().buffer().content() {
            // Text on the terminal's own (light) background must use light colours.
            if cell.bg == Color::Reset && cell.symbol().trim() != "" {
                assert!(
                    !dark_only.contains(&cell.fg),
                    "{:?}: {:?} drawn in dark-theme colour {:?}",
                    app.input_mode,
                    cell.symbol(),
                    cell.fg
                );
            }
        }
    }
    theme::set_light(false);
}

/// Regression: an auto-refresh that briefly shrinks the visible list left the
/// table offset near its end, so once the MRs came back the first rows stayed
/// hidden above the viewport until the user scrolled up with the mouse.
#[tokio::test]
async fn table_rows_stay_visible_after_the_list_shrinks_and_grows() {
    use ratatui::{backend::TestBackend, Terminal};

    let titles: Vec<String> = (1..=40).map(|i| format!("mr {i}")).collect();
    let pairs: Vec<(&str, &str)> = titles.iter().map(|t| (t.as_str(), "None")).collect();
    let mut app = app_with_mrs(&pairs).await;
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    let mut draw = |app: &mut App| {
        terminal.draw(|f| crate::ui::render_ui(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let shows = |lines: &[String], id: &str| {
        let needle = format!("!{id} ");
        lines.iter().any(|l| l.contains(&needle))
    };

    // Scroll to the bottom of the long list.
    app.table_state.select(Some(39));
    draw(&mut app);
    assert!(app.table_state.offset() > 0);

    // A refresh drops most MRs for a moment, then reloads them.
    let all = std::mem::take(&mut app.mrs);
    app.mrs = all[..3].to_vec();
    app.clamp_selection();
    draw(&mut app);
    app.mrs = all[..10].to_vec();
    let lines = draw(&mut app);

    for id in 1..=10 {
        assert!(
            shows(&lines, &id.to_string()),
            "MR !{id} hidden:\n{}",
            lines.join("\n")
        );
    }
    assert_eq!(app.table_state.offset(), 0);
}

/// Tracker double: time-entry responses are queued per test; calls are counted.
#[derive(Default)]
struct FakeTracker {
    time_entries: std::sync::Mutex<
        std::collections::VecDeque<
            Result<Vec<gitlab_tracker_core::TimeEntry>, gitlab_tracker_core::TrackerError>,
        >,
    >,
    time_entry_calls: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl gitlab_tracker_core::TrackerProvider for FakeTracker {
    fn name(&self) -> &'static str {
        "Fake"
    }
    fn detect_ticket_id(&self, _title: &str, _description: &str) -> Option<String> {
        None
    }
    async fn fetch_ticket(
        &self,
        ticket_id: &str,
    ) -> Result<gitlab_tracker_core::LinkedTicket, gitlab_tracker_core::TrackerError> {
        Err(gitlab_tracker_core::TrackerError::NotFound(
            ticket_id.into(),
        ))
    }
    fn ticket_url(&self, ticket_id: &str) -> String {
        format!("https://tracker.example/{ticket_id}")
    }
    async fn fetch_time_entries(
        &self,
        _ticket_id: &str,
    ) -> Result<Vec<gitlab_tracker_core::TimeEntry>, gitlab_tracker_core::TrackerError> {
        self.time_entry_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.time_entries
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(vec![]))
    }
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

#[test]
fn legacy_state_sentinels_restore_as_none() {
    use crate::models::without_sentinel;
    for legacy in ["None", "none", "Loading", "", "  "] {
        assert_eq!(without_sentinel(Some(legacy.into())), None, "{legacy:?}");
    }
    assert_eq!(
        without_sentinel(Some("v1.0".into())).as_deref(),
        Some("v1.0")
    );
    assert_eq!(without_sentinel(None), None);
}

#[cfg(feature = "stats")]
#[test]
fn stale_stats_report_is_ignored() {
    let mut view = StatsViewState::default();
    let old = view.next_generation(); // e.g. 30-day window
    view.loading = true;
    let new = view.next_generation(); // [W] pressed: 90-day window
    let invalid = |msg: &str| gitlab_tracker_stats::StatsError::InvalidData(msg.into());
    view.apply_result(old, Err(invalid("old window")));
    assert!(
        view.loading && view.error.is_none(),
        "late response ignored"
    );
    view.apply_result(new, Err(invalid("new window")));
    assert!(!view.loading);
    assert_eq!(view.error.as_deref(), Some("Invalid data: new window"));
}

async fn press(app: &mut App, tx: &UnboundedSender<AppEvent>, code: KeyCode) -> bool {
    crate::events::handle_key_event(
        crossterm::event::KeyEvent::from(code),
        app,
        &Arc::new(Semaphore::new(1)),
        tx,
        &mut HashMap::new(),
    )
    .await
}

#[tokio::test]
async fn project_picker_only_switches_to_another_project() {
    let mut app = test_app();
    let entry = |url: &str, id: &str| ProjectEntry {
        gitlab_url: url.into(),
        project_id: id.into(),
        ..ProjectEntry::default()
    };
    // The current project (trailing slash ignored), then another one.
    app.project_picker = vec![
        entry("https://gitlab.example/", "1"),
        entry("https://gitlab.example", "2"),
    ];
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    app.input_mode = InputMode::ProjectPicker;
    press(&mut app, &tx, KeyCode::Enter).await;
    assert!(
        app.switch_to.is_none(),
        "re-selecting the current project is a no-op"
    );

    app.input_mode = InputMode::ProjectPicker;
    press(&mut app, &tx, KeyCode::Down).await;
    press(&mut app, &tx, KeyCode::Enter).await;
    assert_eq!(app.switch_to.map(|p| p.project_id), Some("2".to_string()));
    assert_eq!(app.input_mode, InputMode::Normal);
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

#[cfg(feature = "stats")]
#[tokio::test]
async fn demo_stats_overlay_uses_the_real_handler() {
    let mut app = app_with_mrs(&[("t", "None")]).await;
    app.read_only = true;
    app.stats_db = crate::demo::seed_demo_stats_db("1").await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    press(&mut app, &tx, KeyCode::Char('g')).await;
    assert_eq!(app.input_mode, InputMode::Stats);
    let first = app.stats_view.generation;
    assert!(first > 0, "opening the overlay starts a report");

    // [W] used to change the title only: it now recomputes the report.
    let window = app.stats_view.window;
    press(&mut app, &tx, KeyCode::Char('w')).await;
    assert_ne!(app.stats_view.window, window);
    assert!(app.stats_view.generation > first);

    // Esc used to quit the demo from the overlay: it closes the overlay.
    assert!(!press(&mut app, &tx, KeyCode::Esc).await);
    assert_eq!(app.input_mode, InputMode::Normal);
    // Quitting needs a confirmation, as in the app.
    assert!(!press(&mut app, &tx, KeyCode::Esc).await);
    assert!(press(&mut app, &tx, KeyCode::Char('y')).await);
}

#[tokio::test]
async fn label_filter_autocompletes_the_tracked_labels() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None")]).await;
    app.mrs[0].labels = vec!["review::ongoing".into(), "Project Foo - Sprint 4".into()];
    app.mrs[1].labels = vec!["Review::Done".into(), "project foo - sprint 4".into()];
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    app.open_filter_picker();
    assert!(app.label_suggestions().is_empty(), "not on the Label row");
    app.filter_picker.cursor = app
        .visible_filter_defs()
        .iter()
        .position(|(_, def)| def.id == "label")
        .unwrap();
    // Every label once (first casing seen), plus their scope, sorted.
    assert_eq!(
        app.label_suggestions(),
        [
            "Project Foo - Sprint 4",
            "review::",
            "Review::Done",
            "review::ongoing"
        ]
    );

    for c in "REVIEW".chars() {
        press(&mut app, &tx, KeyCode::Char(c)).await;
    }
    assert_eq!(
        app.label_suggestions(),
        ["review::", "Review::Done", "review::ongoing"]
    );
    // Shift+Tab from nothing highlights the last one; Tab wraps to the first.
    press(&mut app, &tx, KeyCode::BackTab).await;
    assert_eq!(app.filter_picker.suggestion, Some(2));
    press(&mut app, &tx, KeyCode::Tab).await;
    assert_eq!(app.filter_picker.suggestion, Some(0));
    // Typing drops the highlight: Enter would apply the text as typed.
    press(&mut app, &tx, KeyCode::Char(':')).await;
    assert_eq!(app.filter_picker.suggestion, None);
    press(&mut app, &tx, KeyCode::Backspace).await;

    press(&mut app, &tx, KeyCode::Tab).await;
    press(&mut app, &tx, KeyCode::Tab).await;
    press(&mut app, &tx, KeyCode::Enter).await;
    assert_eq!(app.input_mode, InputMode::Normal);
    assert_eq!(app.active_filter.query, "Review::Done");
    let visible: Vec<&str> = app.visible_mrs().map(|mr| mr.id.as_str()).collect();
    assert_eq!(visible, ["2"]);
}

#[test]
fn requirements_hide_unavailable_filters_and_columns() {
    let mut app = test_app();
    let filter_ids = |app: &App| -> Vec<&str> {
        app.visible_filter_defs()
            .iter()
            .map(|(_, f)| f.id)
            .collect()
    };
    assert!(
        !filter_ids(&app).contains(&"assigned_to_me"),
        "no gitlab_username"
    );
    app.config.gitlab_username = Some("jane".into());
    assert!(filter_ids(&app).contains(&"assigned_to_me"));
    assert!(filter_ids(&app).contains(&"reviewer_me"));
    // Columns that need a tracker are hidden without one.
    assert!(app
        .visible_column_defs()
        .iter()
        .all(|c| c.requires != Some(gitlab_tracker_core::Requirement::Tracker)));
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
fn pane_scroll_stops_at_the_last_rendered_line() {
    let mut pane = PaneScroll {
        offset: 0,
        content_lines: 30,
        height: 10,
    };
    pane.down(15);
    assert_eq!(pane.offset, 15);
    pane.down(15);
    assert_eq!(pane.offset, 20, "30 lines in a 10-row pane: max offset 20");
    pane.up(25);
    assert_eq!(pane.offset, 0);
    pane.down(3);
    pane.reset();
    assert_eq!(pane.offset, 0);
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

fn scanned(
    app: &mut App,
    commits: Option<BranchCommits>,
    scan: crate::gitlab::BranchScan,
    head: &str,
) {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    app.pending_branch_scans = 1;
    app.apply_event(
        AppEvent::BranchCommitsLoaded {
            branch: "prod".into(),
            commits,
            scan,
            head: Some(head.into()),
        },
        Arc::new(Semaphore::new(1)),
        &tx,
        &mut HashMap::new(),
    );
}

fn shas(list: &[&str]) -> BranchCommits {
    BranchCommits {
        shas: list.iter().map(|s| s.to_string()).collect(),
        ..BranchCommits::default()
    }
}

#[tokio::test]
async fn a_range_scan_grows_the_index_a_full_one_replaces_it() {
    use crate::gitlab::BranchScan;
    let mut app = test_app();
    let full = BranchScan::Full {
        since: "2026-10-01T00:00:00+00:00".into(),
    };
    scanned(&mut app, Some(shas(&["a", "b"])), full.clone(), "b");
    let scope = &app.branch_scan_scopes["prod"];
    assert_eq!(scope.head.as_deref(), Some("b"));
    assert_eq!(scope.floor.to_rfc3339(), "2026-10-01T00:00:00+00:00");

    // Only the new commits were read: added to what is known.
    scanned(
        &mut app,
        Some(shas(&["c"])),
        BranchScan::Range { from: "b".into() },
        "c",
    );
    assert_eq!(app.branch_commits["prod"].shas.len(), 3);
    assert_eq!(app.branch_scan_scopes["prod"].head.as_deref(), Some("c"));

    // A failed range read (head gone after a force-push?): next scan is full.
    scanned(&mut app, None, BranchScan::Range { from: "c".into() }, "c");
    assert_eq!(app.branch_commits["prod"].shas.len(), 3); // index kept meanwhile
    assert!(!app.branch_scan_scopes.contains_key("prod"));

    // A full scan rebuilds the index from scratch.
    scanned(&mut app, Some(shas(&["z"])), full, "z");
    assert_eq!(app.branch_commits["prod"], shas(&["z"]));
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
