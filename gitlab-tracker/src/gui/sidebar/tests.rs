use super::*;

/// Every registered filter: the app's, and those of the linked plugins.
fn all() -> Vec<Entry> {
    gitlab_tracker_core::collect_all_filters()
        .into_iter()
        .enumerate()
        .collect()
}

#[test]
fn every_filter_has_a_place_in_the_panel() {
    let visible = all();
    let groups = Groups::of(&visible);
    let mut placed: Vec<usize> = groups
        .views
        .iter()
        .map(|((i, _), _)| *i)
        .chain(groups.more_views.iter().map(|(i, _)| *i))
        .chain(groups.effort.iter().map(|(i, _)| *i))
        .chain(groups.filters.iter().map(|(i, _)| *i))
        .chain(groups.more_filters.iter().map(|(i, _)| *i))
        .collect();
    placed.sort_unstable();
    let expected: Vec<usize> = visible.iter().map(|(i, _)| *i).collect();
    assert_eq!(placed, expected, "each filter once, none left out");
    // The filters with a value get a field; the others a click.
    for (_, def) in groups.filters.iter().chain(&groups.more_filters) {
        assert!(def.needs_text_input, "{}", def.id);
    }
    for (_, def) in groups
        .views
        .iter()
        .map(|(e, _)| e)
        .chain(&groups.more_views)
    {
        assert!(!def.needs_text_input, "{}", def.id);
    }
}

#[test]
fn the_arrows_cycle_through_the_suggestions() {
    assert_eq!(step(None, 3, true, false), Some(0), "↓ starts at the first");
    assert_eq!(step(None, 3, false, true), Some(2), "↑ starts at the last");
    assert_eq!(step(Some(2), 3, true, false), Some(0), "wraps forward");
    assert_eq!(step(Some(0), 3, false, true), Some(2), "wraps back");
    assert_eq!(step(Some(1), 3, false, false), Some(1), "no key: stays");
    assert_eq!(
        step(Some(4), 3, false, false),
        None,
        "fewer suggestions now"
    );
    assert_eq!(step(Some(0), 0, true, false), None);
}

#[test]
fn the_tui_filters_are_all_registered() {
    let ids: Vec<&str> = all().iter().map(|(_, d)| d.id).collect();
    for id in [
        "all",
        "flagged",
        "state_opened",
        "state_merged",
        "state_closed",
        "mergeability_mergeable",
        "mergeability_conflict",
        "mergeability_rebase",
        "mergeability_not_approved",
        "mergeability_requested_changes",
        "mergeability_draft",
        "mergeability_discussions",
        "has_notes",
        "ci_failing",
        "assigned_to_me",
        "reviewer_me",
        "merged_by_me",
        "effort_easy",
        "effort_medium",
        "effort_complex",
        "milestone",
        "target_branch",
        "assignee",
        "absent_from_branch",
        "label",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
}
