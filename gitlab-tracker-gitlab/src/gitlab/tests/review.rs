//! The review timeline: drafts, requests, approvals, threads.

use super::*;

#[test]
fn unresolved_threads_ignore_plain_and_resolved_notes() {
    let json = r#"[
        {"notes": [{"resolvable": true, "resolved": false}, {"resolvable": true, "resolved": true}]},
        {"notes": [{"resolvable": true, "resolved": true}]},
        {"notes": [{"body": "plain comment", "resolvable": false}]},
        {"notes": [{"resolvable": null, "resolved": null}]},
        {"notes": [{"resolvable": true}]}
    ]"#;
    let discussions: Vec<super::Discussion> = serde_json::from_str(json).unwrap();
    let threads = review_threads(&discussions);
    // Plain comments and notes with null fields are not review threads.
    assert_eq!(threads.len(), 3);
    assert_eq!(threads.iter().filter(|t| !t.resolved).count(), 2);
}
/// A note as GitLab's discussions list it.
fn note(id: u64, user: &str, system: bool, body: &str, at: &str) -> String {
    format!(
        r#"{{"notes": [{{"id": {id}, "system": {system}, "author": {{"username": "{user}", "name": "{user}"}},
            "body": "{body}", "created_at": "{at}"}}]}}"#
    )
}
fn activity_of(notes: &[String], draft_now: bool) -> ReviewActivity {
    let json = format!("[{}]", notes.join(","));
    let discussions: Vec<Discussion> = serde_json::from_str(&json).unwrap();
    review_activity(&discussions, "dev", draft_now)
}
#[test]
fn a_draft_is_reviewed_along_the_way_and_its_review_starts_once_ready() {
    // Opened as a draft (its first move is out of draft), nobody asked to
    // review, comments and pushes during the work, then ready.
    let activity = activity_of(
        &[
            note(1, "dev", true, "added 1 commit", "2026-09-01T09:00:00Z"),
            note(2, "alice", false, "Early thought", "2026-09-02T10:00:00Z"),
            note(3, "dev", true, "added 1 commit", "2026-09-03T10:00:00Z"),
            note(
                4,
                "renovate-bot",
                false,
                "Dependency dashboard",
                "2026-09-03T11:00:00Z",
            ),
            note(
                5,
                "dev",
                true,
                "marked this merge request as **ready**",
                "2026-09-05T09:00:00Z",
            ),
            note(6, "renovate-bot", false, "Rebased", "2026-09-05T09:30:00Z"),
            note(7, "alice", false, "One more thing", "2026-09-05T14:00:00Z"),
            note(8, "dev", true, "added 1 commit", "2026-09-05T16:00:00Z"),
            note(
                9,
                "alice",
                true,
                "approved this merge request",
                "2026-09-06T09:00:00Z",
            ),
        ],
        false,
    );
    assert!(activity.was_draft);
    assert_eq!(activity.ready_at.as_deref(), Some("2026-09-05T09:00:00Z"));
    assert_eq!(
        activity.draft_comments, 1,
        "Alice's early note; not the bot's"
    );
    // The first review is Alice's note once ready, not the bot's.
    assert_eq!(
        activity.first_review_at.as_deref(),
        Some("2026-09-05T14:00:00Z")
    );
    // The push during the draft is not a round; the one after the review is.
    assert_eq!(activity.pushes_after_review, 1);
    assert_eq!(activity.up_for_review_at(), Some("2026-09-05T09:00:00Z"));
    assert_eq!(activity.version, crate::models::REVIEW_ACTIVITY_VERSION);
}
#[test]
fn a_review_asked_on_a_draft_puts_it_up_for_review() {
    // The team's way: a draft, a review asked while still a draft, the
    // approval while still a draft, then marked ready to merge. The wait and
    // the review are not development (regression: they all went to it).
    let activity = activity_of(
        &[
            note(1, "dev", true, "added 3 commits", "2026-11-23T09:00:00Z"),
            note(
                2,
                "dev",
                true,
                "requested review from @alice",
                "2026-11-23T16:30:00+01:00",
            ),
            note(
                3,
                "alice",
                true,
                "approved this merge request",
                "2026-11-24T11:23:00+01:00",
            ),
            note(
                4,
                "dev",
                true,
                "marked this merge request as **ready**",
                "2026-11-24T11:24:00+01:00",
            ),
        ],
        false,
    );
    assert!(activity.was_draft);
    assert_eq!(
        activity.up_for_review_at(),
        Some("2026-11-23T16:30:00+01:00"),
        "the request, not the later move to ready"
    );
    assert_eq!(
        activity.ready_at.as_deref(),
        Some("2026-11-24T11:24:00+01:00")
    );
    assert_eq!(
        activity.first_review_at.as_deref(),
        Some("2026-11-24T11:23:00+01:00"),
        "an approval on a draft is the review"
    );
    assert_eq!(activity.draft_comments, 0);
    // Comments asked for on a draft are the review too.
    let commented = activity_of(
        &[
            note(
                1,
                "dev",
                true,
                "requested review from @alice",
                "2026-11-23T16:30:00Z",
            ),
            note(2, "alice", false, "Rename this", "2026-11-24T10:00:00Z"),
        ],
        true,
    );
    assert_eq!(
        commented.first_review_at.as_deref(),
        Some("2026-11-24T10:00:00Z")
    );
    assert_eq!(commented.up_for_review_at(), Some("2026-11-23T16:30:00Z"));
    assert_eq!(commented.draft_comments, 0);
}
#[test]
fn an_approval_on_a_draft_nobody_was_asked_to_review_is_the_review() {
    let activity = activity_of(
        &[note(
            1,
            "alice",
            true,
            "approved this merge request",
            "2026-09-02T10:00:00Z",
        )],
        true,
    );
    assert_eq!(
        activity.first_review_at.as_deref(),
        Some("2026-09-02T10:00:00Z")
    );
    assert_eq!(activity.draft_comments, 0);
    assert_eq!(activity.up_for_review_at(), None, "still being written");
}
#[test]
fn an_mr_never_in_draft_is_up_for_review_at_its_first_request() {
    let activity = activity_of(
        &[
            note(
                1,
                "dev",
                true,
                "requested review from @alice",
                "2026-09-01T09:00:00Z",
            ),
            note(2, "alice", false, "Looks good", "2026-09-01T10:00:00Z"),
        ],
        false,
    );
    assert!(!activity.was_draft);
    assert_eq!(activity.ready_at, None);
    assert_eq!(activity.draft_comments, 0);
    assert_eq!(
        activity.first_review_at.as_deref(),
        Some("2026-09-01T10:00:00Z")
    );
    assert_eq!(activity.up_for_review_at(), Some("2026-09-01T09:00:00Z"));
}
#[test]
fn a_draft_with_no_move_yet_is_told_by_its_flag() {
    let notes = [note(
        1,
        "alice",
        false,
        "Early thought",
        "2026-09-02T10:00:00Z",
    )];
    let draft = activity_of(&notes, true);
    assert!(draft.was_draft);
    assert_eq!(
        (draft.draft_comments, draft.first_review_at.as_deref()),
        (1, None)
    );
    assert_eq!(draft.up_for_review_at(), None, "still being written");
    let ready = activity_of(&notes, false);
    assert_eq!(
        ready.first_review_at.as_deref(),
        Some("2026-09-02T10:00:00Z")
    );
}
#[test]
fn back_to_draft_after_the_review_keeps_the_first_ready() {
    // Ready, reviewed, back to draft for rework, ready again: the review
    // started at the first ready; the rework is a round.
    let activity = activity_of(
        &[
            note(
                1,
                "dev",
                true,
                "marked this merge request as **ready**",
                "2026-09-02T09:00:00Z",
            ),
            note(2, "alice", false, "Needs a rework", "2026-09-02T10:00:00Z"),
            note(
                3,
                "dev",
                true,
                "marked this merge request as **draft**",
                "2026-09-02T11:00:00Z",
            ),
            note(4, "alice", false, "Better", "2026-09-03T10:00:00Z"),
            note(5, "dev", true, "added 2 commits", "2026-09-03T12:00:00Z"),
            note(
                6,
                "dev",
                true,
                "marked this merge request as **ready**",
                "2026-09-04T09:00:00Z",
            ),
        ],
        false,
    );
    assert_eq!(activity.ready_at.as_deref(), Some("2026-09-02T09:00:00Z"));
    assert_eq!(
        activity.first_review_at.as_deref(),
        Some("2026-09-02T10:00:00Z")
    );
    assert_eq!(activity.draft_comments, 1);
    assert_eq!(activity.pushes_after_review, 1);
}
#[test]
fn draft_moves_read_every_gitlab_wording() {
    for (body, expected) in [
        ("marked this merge request as **draft**", Some(true)),
        ("marked this merge request as **ready**", Some(false)),
        (
            "marked this merge request as **draft** from 1a2b3c4",
            Some(true),
        ),
        ("marked as a **Work In Progress**", Some(true)),
        ("unmarked as a **Work In Progress**", Some(false)),
        ("marked as a **draft**", Some(true)),
        ("unmarked as a **draft**", Some(false)),
        (
            "marked the checklist item **Draft the docs** as completed",
            None,
        ),
        ("changed title from **Draft: x** to **x**", None),
        ("approved this merge request", None),
    ] {
        assert_eq!(draft_move(body), expected, "{body}");
    }
}
#[test]
fn an_older_review_read_is_read_again() {
    let mut cached = CachedMrData::default();
    assert!(!review_current(&cached), "never read");
    cached.review = Some(ReviewActivity::default());
    assert!(!review_current(&cached), "read before the drafts were");
    cached.review = Some(ReviewActivity {
        version: crate::models::REVIEW_ACTIVITY_VERSION,
        ..ReviewActivity::default()
    });
    assert!(review_current(&cached));
}
#[test]
fn review_activity_keeps_each_standing_approval_with_its_time() {
    let json = r#"[
        {"notes": [{"id": 1, "system": true, "author": {"username": "alice", "name": "Alice"},
                    "body": "approved this merge request", "created_at": "2026-09-01T10:00:00.000Z"}]},
        {"notes": [{"id": 2, "system": true, "author": {"username": "carol", "name": "Carol"},
                    "body": "approved this merge request", "created_at": "2026-09-01T11:00:00.000Z"}]},
        {"notes": [{"id": 3, "system": true, "author": {"username": "carol", "name": "Carol"},
                    "body": "unapproved this merge request", "created_at": "2026-09-01T12:00:00.000Z"}]},
        {"notes": [{"id": 4, "system": true, "author": {"username": "bob", "name": "Bob"},
                    "body": "approved this merge request", "created_at": "2026-09-02T09:00:00.000Z"}]}
    ]"#;
    let discussions: Vec<Discussion> = serde_json::from_str(json).unwrap();
    let activity = review_activity(&discussions, "dev", false);
    let approvals: Vec<(&str, &str)> = activity
        .approvals
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        approvals,
        [
            ("alice", "2026-09-01T10:00:00.000Z"),
            ("bob", "2026-09-02T09:00:00.000Z")
        ]
    );
}
#[test]
fn review_activity_reads_comments_approvals_requests_and_pushes() {
    let json = r#"[
        {"notes": [{"id": 1, "system": true, "author": {"username": "dev", "name": "Dev"},
                    "body": "requested review from @alice and @bob", "created_at": "2026-09-01T09:00:00.000Z"}]},
        {"notes": [{"id": 2, "author": {"username": "dev", "name": "Dev"},
                    "body": "Self note", "created_at": "2026-09-01T09:30:00.000Z"}]},
        {"notes": [
            {"id": 3, "author": {"username": "alice", "name": "Alice"}, "resolvable": true, "resolved": false,
             "body": "Why\n  this   change?", "created_at": "2026-09-01T10:00:00.000Z"},
            {"id": 4, "author": {"username": "dev", "name": "Dev"}, "resolvable": true, "resolved": false,
             "body": "Because.", "created_at": "2026-09-01T11:00:00.000Z"}]},
        {"notes": [{"id": 5, "system": true, "author": {"username": "dev", "name": "Dev"},
                    "body": "added 2 commits\n\n<ul><li>abc</li></ul>", "created_at": "2026-09-01T12:00:00.000Z"}]},
        {"notes": [{"id": 6, "system": true, "author": {"username": "bob", "name": "Bob"},
                    "body": "approved this merge request", "created_at": "2026-09-01T13:00:00.000Z"}]},
        {"notes": [{"id": 7, "system": true, "author": {"username": "bob", "name": "Bob"},
                    "body": "unapproved this merge request", "created_at": "2026-09-01T14:00:00.000Z"}]},
        {"notes": [{"id": 8, "system": true, "author": {"username": "dev", "name": "Dev"},
                    "body": "removed review request for @bob", "created_at": "2026-09-01T15:00:00.000Z"}]}
    ]"#;
    let discussions: Vec<Discussion> = serde_json::from_str(json).unwrap();

    let activity = review_activity(&discussions, "dev", false);
    // The author's own note is not a review; Alice's comment is the first one.
    assert_eq!(
        activity.first_review_at.as_deref(),
        Some("2026-09-01T10:00:00.000Z")
    );
    assert_eq!(activity.approved_at, None, "unapproved afterwards");
    assert!(activity.approvals.is_empty(), "Bob's approval withdrawn");
    assert_eq!(
        activity.last_push_at.as_deref(),
        Some("2026-09-01T12:00:00.000Z")
    );
    // The push came after Alice's review: one round back to the author.
    assert_eq!(activity.pushes_after_review, 1);
    // Who pushed: the author here, and a push is not a review.
    assert_eq!(
        activity.last_push_by.get("dev").map(String::as_str),
        Some("2026-09-01T12:00:00.000Z")
    );
    assert_eq!(
        activity.first_review_requested_at.as_deref(),
        Some("2026-09-01T09:00:00.000Z")
    );
    let requested: Vec<&str> = activity
        .review_requested_at
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(requested, ["alice"]);
    assert_eq!(
        activity.last_activity_by.get("bob").map(String::as_str),
        Some("2026-09-01T13:00:00.000Z")
    );
    assert_eq!(
        activity.last_activity_by.get("dev").map(String::as_str),
        Some("2026-09-01T11:00:00.000Z")
    );

    let threads = review_threads(&discussions);
    assert_eq!(threads.len(), 1);
    let thread = &threads[0];
    assert_eq!((thread.note_id, thread.author.as_str()), (3, "alice"));
    assert_eq!(thread.excerpt, "Why this change?");
    assert_eq!((thread.notes, thread.resolved), (2, false));
    assert_eq!(thread.last_note_at, "2026-09-01T11:00:00.000Z");
}
