use super::*;
use std::collections::HashMap;

#[test]
fn cherry_pick_sources_reads_every_full_sha_trailer() {
    let a = "a".repeat(40);
    let b = "0123456789abcdef0123456789abcdef01234567";
    let msg =
        format!("Fix login\n\n(cherry picked from commit {a})\n(cherry picked from commit {b})");
    let found: Vec<&str> = super::cherry_pick_sources(&msg).collect();
    assert_eq!(found, [a.as_str(), b]);
    assert_eq!(super::cherry_pick_sources("plain commit").count(), 0);
    // A truncated or non-hex SHA is not a cherry-pick source.
    assert_eq!(
        super::cherry_pick_sources("(cherry picked from commit abc123)").count(),
        0
    );
}

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
    // Opened as a draft (its first move is out of draft), a reviewer asked
    // at once, comments and pushes during the work, then ready.
    let activity = activity_of(
        &[
            note(
                1,
                "dev",
                true,
                "requested review from @alice",
                "2026-09-01T09:00:00Z",
            ),
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

#[test]
fn job_logs_are_cleaned_and_cut_to_their_end() {
    let raw = "\x1b[0Ksection_start:1:step\r\x1b[0K\x1b[32;1m$ cargo test\x1b[0;m\n\
               Downloading 10%\rDownloading 100%\n\n\
               \x1b[31merror\x1b[0m: 2 tests failed\n\
               section_end:2:step\r\x1b[0K\n";
    assert_eq!(
        super::log_tail(raw, 10),
        "$ cargo test\nDownloading 100%\nerror: 2 tests failed"
    );
    assert_eq!(super::log_tail(raw, 1), "error: 2 tests failed");
}

#[test]
fn diff_totals_handle_overflow_and_null_changes() {
    let parse = |json: &str| super::diff_totals(&serde_json::from_str(json).unwrap());
    let patch = r"@@ -1 +1,2 @@\n-a\n+b\n+c"; // JSON-escaped newlines
    let normal = format!(r#"{{"changes": [{{"diff": "{patch}"}}, {{"diff": null}}]}}"#);
    assert_eq!(parse(&normal), (2, 2, 1));
    let overflow = format!(
        r#"{{"overflow": true, "changes_count": "1000+", "changes": [{{"diff": "{patch}"}}]}}"#
    );
    assert_eq!(parse(&overflow), (1000, 2, 1));
    assert_eq!(
        parse(r#"{"changes_count": "42", "changes": null}"#),
        (42, 0, 0)
    );
}

#[test]
fn pagination_stops_on_short_page_or_cap() {
    assert_eq!(next_page(1, PAGE_SIZE), Some(2));
    assert_eq!(next_page(3, 42), None); // short page = last page
    assert_eq!(next_page(1, 0), None);
    assert_eq!(next_page(MAX_PAGES, PAGE_SIZE), None); // safety cap
}

#[test]
fn diff_stats_cache_rejects_legacy_entries() {
    use crate::models::DiffStats;
    let stats = |files_changed, additions, commits_count| DiffStats {
        files_changed,
        additions,
        commits_count,
        files_listed: true,
        ..DiffStats::default()
    };
    let commit = |sha: &str| MrCommit {
        sha: sha.into(),
        fingerprint: CommitFingerprint {
            author_email: "a@b".into(),
            authored_at: 0,
            title: sha.into(),
        },
        author_name: String::new(),
    };
    let two = [commit("1"), commit("2")];
    assert!(!diff_stats_cache_valid(None, &[]));
    assert!(diff_stats_cache_valid(Some(&stats(3, 10, 2)), &two));
    assert!(diff_stats_cache_valid(Some(&stats(0, 0, 0)), &[])); // empty MR
    assert!(!diff_stats_cache_valid(Some(&stats(3, 0, 2)), &two)); // old line fields
    assert!(!diff_stats_cache_valid(Some(&stats(3, 10, 0)), &[])); // before commits_count
    assert!(!diff_stats_cache_valid(Some(&stats(3, 10, 2)), &[])); // before the commit list
    let before_files = DiffStats {
        files_listed: false,
        ..stats(3, 10, 2)
    };
    assert!(!diff_stats_cache_valid(Some(&before_files), &two)); // before the files
}

#[test]
fn hunks_give_the_old_lines_they_replace() {
    let patch =
        "@@ -10,3 +10,4 @@ fn a()\n a\n-b\n+c\n+d\n@@ -40,0 +42,2 @@\n+e\n+f\n@@ -7 +9 @@\n-x\n+y";
    assert_eq!(hunk_ranges(patch), vec![(10, 12), (40, 40), (7, 7)]);
    let body: MrChanges = serde_json::from_str(
        r#"{"changes": [{"diff": "@@ -1,2 +1,2 @@\n-a\n+b", "new_path": "src/a.rs", "old_path": "src/old.rs", "renamed_file": true}]}"#,
    )
    .unwrap();
    let files = changed_files(&body);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "src/a.rs");
    assert_eq!(files[0].old_path.as_deref(), Some("src/old.rs"));
    assert_eq!(files[0].hunks, vec![(1, 2)]);
    assert_eq!((files[0].added, files[0].deleted), (1, 1));
}

#[test]
fn branch_index_matches_copies_across_time_zones() {
    let commits: Vec<CommitJson> = serde_json::from_str(&format!(
        r#"[
            {{"id": "c1", "title": " Fix login ", "message": "Fix login\n\n(cherry picked from commit {sha})",
              "author_email": "Dev@Example.com", "authored_date": "2026-09-28T15:03:01.000+02:00",
              "committed_date": "2026-09-30T08:00:00Z"}},
            {{"id": "c2", "title": "No date", "message": "No date", "author_email": "x@y"}}
        ]"#,
        sha = "a".repeat(40)
    ))
    .unwrap();
    let index = index_branch_commits(&commits);

    assert!(index.shas.contains("c1") && index.shas.contains("c2"));
    let committed = chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z")
        .unwrap()
        .timestamp();
    assert_eq!(index.picked_from.get(&"a".repeat(40)), Some(&committed));
    // Same instant written in UTC, e-mail case and title padding normalised.
    let copy = CommitFingerprint {
        author_email: "dev@example.com".into(),
        authored_at: chrono::DateTime::parse_from_rfc3339("2026-09-28T13:03:01Z")
            .unwrap()
            .timestamp(),
        title: "Fix login".into(),
    };
    assert_eq!(index.fingerprints, HashMap::from([(copy, committed)]));
    assert_eq!(normalize_title(" '#1410856: Update' "), "#1410856: Update");
    assert_eq!(normalize_title("\"Revert \"x\"\""), "Revert \"x");
}

#[test]
fn diff_lines_keep_double_sign_changes() {
    // GitLab's usual shape: no file headers, hunks only.
    let patch = "@@ -1,2 +1,3 @@\n ctx\n-old\n+++i;\n+-- sql comment\n--- removed dashes";
    assert_eq!(count_diff_lines(patch), (2, 2));
    // With file headers: they are skipped, but only before the first hunk.
    let with_headers = "--- a/f.rs\n+++ b/f.rs\n@@ -1 +1 @@\n-a\n+b";
    assert_eq!(count_diff_lines(with_headers), (1, 1));
}

#[test]
fn discovery_anchor_advances_but_never_moves_back() {
    let at = |s: &str| s.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
    // First poll right after startup: now - margin is before the anchor → unchanged.
    assert_eq!(
        next_discovery_anchor("2026-09-29T10:00:00+00:00", at("2026-09-29T10:00:05Z")),
        "2026-09-29T10:00:00+00:00"
    );
    // A day later: advances to poll start minus the margin.
    assert_eq!(
        next_discovery_anchor("2026-09-29T10:00:00+00:00", at("2026-09-30T10:00:00Z")),
        "2026-09-30T09:00:00+00:00"
    );
    // Unparseable anchor is kept as-is.
    assert_eq!(
        next_discovery_anchor("garbage", at("2026-09-30T10:00:00Z")),
        "garbage"
    );
}

// ── Request batching ─────────────────────────────────────────────────────

fn ctx(base_url: &str) -> FetchContext {
    FetchContext {
        base_url: base_url.to_string(),
        token: Arc::new(zeroize::Zeroizing::new("t".to_string())),
        project_id: "1".into(),
        branches: vec!["main".into()],
    }
}

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

/// What a route returns for a 404.
pub const NOT_FOUND: &str = "<404>";

/// A stand-in GitLab answering every request through `route(path, body)`, and
/// recording the paths it was asked for. A [`NOT_FOUND`] body answers 404.
pub async fn fake_gitlab(
    route: fn(&str, &str) -> String,
) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                let head_end = loop {
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                    let read = socket.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        return;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                };
                let head = String::from_utf8_lossy(&raw[..head_end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .map_or(0, |v| v.trim().parse::<usize>().unwrap_or(0));
                while raw.len() < head_end + length {
                    let read = socket.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    raw.extend_from_slice(&chunk[..read]);
                }
                let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                let path = head.split(' ').nth(1).unwrap_or_default().to_string();
                let request = String::from_utf8_lossy(&raw[head_end..]).to_string();
                let body = route(&path, &request);
                log.lock().unwrap().push(path);
                let (status, body) = if body == NOT_FOUND {
                    ("404 Not Found", "{}".to_string())
                } else {
                    ("200 OK", body)
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (url, seen)
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

// ── Pipelines over GraphQL ───────────────────────────────────────────────

/// The iids a GraphQL request asks for.
fn asked_iids(body: &str) -> Vec<String> {
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    body["variables"]["iids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

/// A GraphQL answer giving each MR one finished pipeline with one job.
fn pipelines_answer(iids: &[String]) -> String {
    let nodes: Vec<_> = iids
        .iter()
        .map(|iid| {
            serde_json::json!({ "iid": iid, "pipelines": { "nodes": [{
                "id": format!("gid://gitlab/Ci::Pipeline/{iid}0"), "status": "SUCCESS",
                "createdAt": "2026-11-10T08:00:00Z", "updatedAt": "2026-11-10T08:05:00Z",
                "sha": "abc", "jobs": { "nodes": [{
                    "id": format!("gid://gitlab/Ci::Build/{iid}00"), "name": "test",
                    "status": "SUCCESS", "duration": 30, "queuedDuration": 2.0,
                    "stage": { "name": "test" } }] } }] } })
        })
        .collect();
    serde_json::json!({ "data": { "project": { "mergeRequests": { "nodes": nodes } } } })
        .to_string()
}

/// Common answers for a refresh of closed MRs 1..=3 (no page read needed).
fn closed_mrs_route(path: &str) -> Option<String> {
    let p = path.split('?').next().unwrap();
    if p.ends_with("/merge_requests") {
        Some(
            serde_json::json!([1, 2, 3]
                .map(|iid| serde_json::json!({"iid": iid, "title": "t", "state": "closed"})))
            .to_string(),
        )
    } else if p.ends_with("/changes") {
        Some(r#"{"changes": []}"#.into())
    } else if p.ends_with("/api/graphql") {
        None
    } else {
        Some("[]".into())
    }
}

async fn refresh_three(
    route: fn(&str, &str) -> String,
) -> (String, Vec<String>, Vec<MrLoadedData>) {
    let (url, seen) = fake_gitlab(route).await;
    let mrs = ["1", "2", "3"]
        .map(|id| (id.to_string(), CachedMrData::default()))
        .to_vec();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    spawn_mr_fetches(ctx(&url), mrs, Arc::new(Semaphore::new(3)), tx);
    let mut loaded = Vec::new();
    while loaded.len() < 3 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("every MR is loaded")
            .unwrap();
        if let GitlabEvent::MrLoaded(data) = event {
            loaded.push(*data);
        }
    }
    let seen = seen.lock().unwrap().clone();
    (url, seen, loaded)
}

fn rest_pipeline_reads(seen: &[String]) -> usize {
    seen.iter()
        .filter(|p| p.contains("/pipelines") || p.contains("/jobs"))
        .count()
}

#[tokio::test]
async fn the_pipelines_of_a_refresh_come_in_one_graphql_query() {
    fn route(path: &str, body: &str) -> String {
        closed_mrs_route(path).unwrap_or_else(|| pipelines_answer(&asked_iids(body)))
    }
    let (_, seen, loaded) = refresh_three(route).await;
    assert_eq!(
        seen.iter().filter(|p| *p == "/api/graphql").count(),
        1,
        "{seen:#?}"
    );
    assert_eq!(rest_pipeline_reads(&seen), 0, "{seen:#?}");
    for mr in &loaded {
        let pipeline = &mr.data.pipelines[0];
        assert_eq!(pipeline.id, format!("{}0", mr.id).parse::<u64>().unwrap());
        assert_eq!(pipeline.status, crate::models::PipelineState::Success);
        assert_eq!(pipeline.jobs[0].stage, "test");
    }
}

#[tokio::test]
async fn a_query_too_complex_is_split_and_the_size_remembered() {
    fn route(path: &str, body: &str) -> String {
        closed_mrs_route(path).unwrap_or_else(|| {
            let iids = asked_iids(body);
            if iids.len() > 1 {
                r#"{"errors": [{"message": "Query has complexity of 400, which exceeds max complexity of 250"}]}"#.into()
            } else {
                pipelines_answer(&iids)
            }
        })
    }
    let (url, seen, loaded) = refresh_three(route).await;
    assert_eq!(rest_pipeline_reads(&seen), 0, "{seen:#?}");
    assert!(loaded.iter().all(|mr| mr.data.pipelines.len() == 1));
    // 3 → 2 + 1 → 1 + 1 + 1: five queries, then single-MR batches from now on.
    assert_eq!(seen.iter().filter(|p| *p == "/api/graphql").count(), 5);
    assert_eq!(
        crate::gitlab_graphql::support(&url),
        crate::gitlab_graphql::Support::Batch(1)
    );
}

#[tokio::test]
async fn an_older_gitlab_falls_back_to_rest_for_good() {
    fn route(path: &str, _body: &str) -> String {
        closed_mrs_route(path).unwrap_or_else(|| {
            r#"{"errors": [{"message": "Field 'queuedDuration' doesn't exist on type 'CiJob'"}]}"#.into()
        })
    }
    let (url, seen, _) = refresh_three(route).await;
    assert_eq!(seen.iter().filter(|p| *p == "/api/graphql").count(), 1);
    // Each MR read its pipelines list over REST instead.
    let rest = seen
        .iter()
        .filter(|p| p.contains("/merge_requests/") && p.contains("/pipelines"))
        .count();
    assert_eq!(rest, 3, "{seen:#?}");
    assert_eq!(
        crate::gitlab_graphql::support(&url),
        crate::gitlab_graphql::Support::Off
    );
}
