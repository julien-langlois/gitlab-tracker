use super::*;
use crate::models::{MrData, ReviewActivity, TrackedMr};
use gitlab_tracker_core::MergeabilityStatus;

fn mr(id: &str, author: &str, reviewers: &[&str], state: GitlabMrState) -> TrackedMr {
    let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
    mr.data = MrData {
        title: format!("MR {id}"),
        author: author.into(),
        reviewers: reviewers.iter().map(|r| r.to_string()).collect(),
        state,
        created_at: Some("2026-11-01T08:00:00Z".into()),
        ..MrData::default()
    };
    mr.mergeability = MergeabilityStatus::Mergeable;
    mr
}

#[test]
fn reviews_waiting_and_load() {
    let mut app = crate::app::tests::test_app();
    let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
    let mut a = mr(
        "1",
        "Ann Lee (@ann)",
        &["Bob Ray (@bob)"],
        GitlabMrState::Opened,
    );
    a.review = Some(ReviewActivity {
        review_requested_at: [("bob".to_string(), "2026-11-03T08:00:00Z".to_string())].into(),
        ..ReviewActivity::default()
    });
    let mut b = mr(
        "2",
        "Bob Ray (@bob)",
        &["Ann Lee (@ann)"],
        GitlabMrState::Opened,
    );
    // Asked of Ann two hours ago: lighter than Bob's two-day-old request.
    b.review = Some(ReviewActivity {
        review_requested_at: [("ann".to_string(), "2026-11-05T06:00:00Z".to_string())].into(),
        ..ReviewActivity::default()
    });
    let mut c = mr(
        "3",
        "Cid Moe (@cid)",
        &["Bob Ray (@bob)"],
        GitlabMrState::Opened,
    );
    c.approved_by = vec!["bob".into()];
    app.mrs = vec![a, b, c];
    let analysis = analyze(&app, now, 30, false);
    let bob = analysis.people.iter().find(|p| p.login == "bob").unwrap();
    // Bob approved !3: only !1 waits on him, since its request two days ago.
    assert_eq!(bob.reviews.len(), 1);
    assert_eq!(bob.reviews[0].id, "1");
    assert_eq!(bob.name, "Bob Ray");
    assert_eq!(bob.mrs.len(), 1);
    // Bob is the most loaded.
    assert!((bob.load - 1.0).abs() < f32::EPSILON);
    let ann = analysis.people.iter().find(|p| p.login == "ann").unwrap();
    assert!(ann.load < 1.0);
    assert_eq!(analysis.matrix["ann"]["bob"], 1);
}

#[test]
fn bots_stay_out_unless_asked() {
    let mut app = crate::app::tests::test_app();
    let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
    app.mrs = vec![mr(
        "1",
        "Renovate Bot (@renovate_bot)",
        &[],
        GitlabMrState::Opened,
    )];
    assert!(analyze(&app, now, 30, false).people.is_empty());
    let with = analyze(&app, now, 30, true);
    assert_eq!(with.people.len(), 1);
    assert_eq!(with.people[0].level, Level::Bot);
}

#[test]
fn a_push_answers_a_review_and_shows_who_helped() {
    let mut app = crate::app::tests::test_app();
    let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
    let mut a = mr(
        "1",
        "Ann Lee (@ann)",
        &["Bob Ray (@bob)"],
        GitlabMrState::Opened,
    );
    // Asked on the 3rd, Bob answered with a commit a day later, no comment.
    a.review = Some(ReviewActivity {
        review_requested_at: [("bob".to_string(), "2026-11-03T08:00:00Z".to_string())].into(),
        last_push_by: [
            ("bob".to_string(), "2026-11-04T08:00:00Z".to_string()),
            ("ann".to_string(), "2026-11-02T08:00:00Z".to_string()),
        ]
        .into(),
        ..ReviewActivity::default()
    });
    app.mrs = vec![a];
    let analysis = analyze(&app, now, 30, false);
    let bob = analysis.people.iter().find(|p| p.login == "bob").unwrap();
    assert_eq!(bob.response, Some(1.0));
    assert_eq!(bob.weeks[7], 1, "the push counts as this week's review");
    assert_eq!(bob.helped.len(), 1);
    assert_eq!(
        (bob.helped[0].id.as_str(), bob.helped[0].author.as_str()),
        ("1", "Ann Lee (@ann)")
    );
    // Ann's pushes to her own MR are no help to anyone.
    let ann = analysis.people.iter().find(|p| p.login == "ann").unwrap();
    assert!(ann.helped.is_empty());
    assert_eq!(ann.weeks, [0; 8]);
}
