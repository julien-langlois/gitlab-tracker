use super::*;
use crate::models::{MrData, ReviewActivity, TrackedMr};

fn mr(id: &str, author: &str, state: GitlabMrState) -> TrackedMr {
    let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
    mr.data = MrData {
        title: format!("MR {id}"),
        author: author.into(),
        state,
        created_at: Some("2026-10-20T08:00:00Z".into()),
        ..MrData::default()
    };
    mr
}

#[test]
fn profile_as_author_and_reviewer() {
    let mut app = crate::app::tests::test_app();
    let now = board::parse_time("2026-11-05T08:00:00Z").unwrap();
    // Ann's MR, merged: asked on the 21st, reviewed the 22nd, approved the
    // 24th, merged the 25th.
    let mut a = mr("1", "Ann Lee (@ann)", GitlabMrState::Merged);
    a.data.merged_at = Some("2026-10-25T08:00:00Z".into());
    a.data.review = Some(ReviewActivity {
        first_review_requested_at: Some("2026-10-21T08:00:00Z".into()),
        first_review_at: Some("2026-10-22T08:00:00Z".into()),
        approved_at: Some("2026-10-24T08:00:00Z".into()),
        pushes_after_review: 2,
        ..ReviewActivity::default()
    });
    // Bob's MR: Ann asked on the 1st, answered 2 h later, and approved.
    let mut b = mr("2", "Bob Ray (@bob)", GitlabMrState::Opened);
    b.data.reviewers = vec!["Ann Lee (@ann)".into()];
    b.data.approved_by = vec!["ann".into()];
    b.data.review = Some(ReviewActivity {
        review_requested_at: [("ann".to_string(), "2026-11-01T08:00:00Z".to_string())].into(),
        last_activity_by: [("ann".to_string(), "2026-11-01T10:00:00Z".to_string())].into(),
        ..ReviewActivity::default()
    });
    // Cid's MR, which Ann has nothing to do with.
    let c = mr("3", "Cid Moe (@cid)", GitlabMrState::Opened);
    app.mrs = vec![a, b, c];
    let p = profile(&app, "ann", now, 30);
    assert_eq!(p.seen, 2);
    assert_eq!(p.merged, 1);
    assert_eq!(p.cycle, Some(5.0));
    assert_eq!(p.stages, [Some(1.0), Some(2.0), Some(1.0)]);
    assert_eq!(p.first_review, Some(1.0));
    assert_eq!(p.rounds, Some(2.0));
    assert_eq!(p.requested, 1);
    assert_eq!(p.approved, 1);
    assert_eq!(p.buckets, [1, 0, 0, 0]);
    assert_eq!(p.unanswered, 0);
    assert_eq!(p.weeks[WEEKS - 1].reviews, 1);
    assert_eq!(p.weeks[WEEKS - 1].asked, 1);
    assert_eq!(p.weeks.iter().map(|w| w.merged).sum::<u32>(), 1);
    assert_eq!(p.recent.len(), 1);
    assert_eq!(p.recent[0].id, "1");
}
