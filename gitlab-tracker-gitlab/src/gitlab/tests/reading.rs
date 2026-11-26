//! What is read from GitLab's answers: commits, logs, diffs, pagination, branches.

use super::*;

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
