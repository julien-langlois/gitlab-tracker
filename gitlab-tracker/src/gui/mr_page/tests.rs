use super::*;

#[test]
fn milestone_changes_read_as_set_changed_and_removed() {
    let event = |at: &str, milestone: Option<&str>, added| MilestoneEvent {
        at: at.into(),
        milestone: milestone.map(str::to_string),
        added,
        by: Some("Bob (@bob)".into()),
    };
    let events = [
        event("1", Some("1.4"), true),
        event("2", Some("1.5"), true),
        // A change recorded as a remove then an add at the same moment.
        event("3", Some("1.5"), false),
        event("3", Some("1.6"), true),
        event("4", Some("1.6"), false),
        // Deleted since: still told.
        event("5", None, true),
    ];
    let changes: Vec<_> = milestone_changes(&events)
        .into_iter()
        .map(|(at, change, _)| (at, change))
        .collect();
    assert_eq!(
        changes,
        [
            ("1", MilestoneChange::Set("1.4")),
            ("2", MilestoneChange::Changed("1.4", "1.5")),
            ("3", MilestoneChange::Changed("1.5", "1.6")),
            ("4", MilestoneChange::Removed("1.6")),
            ("5", MilestoneChange::Set("?")),
        ]
    );
}

#[tokio::test]
async fn the_checks_put_what_blocks_first() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let mr = app
        .mrs
        .iter()
        .find(|m| m.mergeability == MergeabilityStatus::Conflict)
        .expect("the demo has an MR in conflict");
    let data = crate::demo::demo_page_data(mr);
    let list = checks(
        mr,
        data.overview.as_ref(),
        data.rules.as_deref(),
        data.threads.as_deref().ok(),
        data.conflicts.as_deref(),
    );
    assert_eq!(list.first().map(|c| c.verdict), Some(Verdict::Blocks));
    assert!(list.iter().any(|c| c.what.contains("Conflicts")));
    // Sorted: nothing met before something that blocks or warns.
    let order: Vec<u8> = list
        .iter()
        .map(|c| match c.verdict {
            Verdict::Blocks => 0,
            Verdict::Warns => 1,
            Verdict::Met => 2,
        })
        .collect();
    assert!(order.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn medians_and_spans_read_as_days() {
    assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(median(Vec::new()), None);
    assert_eq!(span(0.5), "12 h");
    assert_eq!(span(2.06), "2.1 d");
}
