use super::*;
use crate::models::{MrData, Pipeline, ReviewActivity, TrackedMr};

#[test]
fn events_come_from_the_dates_of_the_mrs() {
    let mut app = crate::app::tests::test_app();
    let mut mr = TrackedMr::placeholder("7".into(), "Fix it".into(), None);
    mr.data = MrData {
        title: "Fix it".into(),
        author: "Ann Lee (@ann)".into(),
        state: GitlabMrState::Merged,
        created_at: Some("2026-11-01T08:00:00Z".into()),
        merged_at: Some("2026-11-04T08:00:00Z".into()),
        merged_by: Some("Bob Ray (@bob)".into()),
        review: Some(ReviewActivity {
            review_requested_at: [("bob".to_string(), "2026-11-02T08:00:00Z".to_string())].into(),
            ..ReviewActivity::default()
        }),
        pipelines: vec![Pipeline {
            id: 9,
            status: PipelineState::Failed,
            created_at: Some("2026-11-03T08:00:00Z".into()),
            ..Pipeline::default()
        }],
        ..MrData::default()
    };
    app.mrs = vec![mr];
    let theme = Theme::dark();
    let since = board::parse_time("2026-10-01T00:00:00Z").unwrap();
    let events = events_of(&app, 0, since, &theme);
    let kinds: Vec<Kind> = events.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![Kind::Activity, Kind::Review, Kind::Pipeline, Kind::Merge]
    );
    assert!(events[1].what.contains("Bob Ray"));
    assert!(events[3].what.contains("Bob Ray"));
    // Only what happened since the cut.
    let since = board::parse_time("2026-11-03T12:00:00Z").unwrap();
    assert_eq!(events_of(&app, 0, since, &theme).len(), 1);
}
