//! The list: filters, search, sorting, selection.

use super::*;

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
#[test]
fn a_username_read_in_the_background_fills_the_me_views_without_replacing_one_set() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = test_app();
    let apply = |app: &mut App, name: &str| {
        app.apply_event(
            AppEvent::UsernameResolved(name.into()),
            Arc::new(Semaphore::new(1)),
            &tx,
            &mut HashMap::new(),
        );
    };
    assert_eq!(app.config.gitlab_username, None);
    apply(&mut app, "jane");
    assert_eq!(app.config.gitlab_username.as_deref(), Some("jane"));
    // The "me" filters show once it is known.
    assert!(app.requirement_met(gitlab_tracker_core::Requirement::GitlabUsername));
    apply(&mut app, "someone_else");
    assert_eq!(app.config.gitlab_username.as_deref(), Some("jane"));
}
