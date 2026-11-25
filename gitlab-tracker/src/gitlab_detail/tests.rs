use super::*;

#[test]
fn version_tags_read_as_versions() {
    for tag in [
        "v1.2.0",
        "1.2.3",
        "V2.0",
        "v3.0.0-rc.1",
        "1.2.3.4",
        "2.1.0+build5",
    ] {
        assert!(is_version_tag(tag), "{tag}");
    }
    for tag in [
        "deploy-2026",
        "v1",
        "release",
        "v1.x",
        "1..2",
        "staging-1.2",
    ] {
        assert!(!is_version_tag(tag), "{tag}");
    }
}
use crate::gitlab::FileDiff;

#[test]
fn the_merge_history_round_trips_and_knows_when_it_is_current() {
    let now = chrono::Utc::now();
    let history = MergeHistory {
        merged: [(12, "2026-11-01T10:00:00Z".to_string())].into(),
        read_at: Some((now - chrono::Duration::hours(3)).to_rfc3339()),
    };
    let text = serde_json::to_string(&history).unwrap();
    assert_eq!(
        serde_json::from_str::<MergeHistory>(&text).unwrap(),
        history
    );
    assert!(history.is_current(now));
    // A read cut short two days ago: only its oldest weeks are known.
    let behind = MergeHistory {
        read_at: Some((now - chrono::Duration::days(2)).to_rfc3339()),
        ..history.clone()
    };
    assert!(!behind.is_current(now));
    assert!(!MergeHistory::default().is_current(now));
    // An older file without some fields still loads.
    assert_eq!(
        serde_json::from_str::<MergeHistory>("{}").unwrap(),
        MergeHistory::default()
    );
}

fn file(path: &str, diff: &str) -> FileDiff {
    FileDiff {
        old_path: path.into(),
        new_path: path.into(),
        new_file: false,
        renamed_file: false,
        deleted_file: false,
        diff: diff.into(),
    }
}

#[test]
fn merge_commits_name_their_mr_and_its_title() {
    let message = "Merge branch 'feat/login' into 'main'\n\nfeat(auth): Login with SSO\n\nCloses #12\n\nSee merge request group/sub/project!42";
    assert_eq!(
        read_merge_message("Merge branch 'feat/login' into 'main'", message),
        ("feat(auth): Login with SSO".into(), Some("42".into()))
    );
    // A squash commit keeps its title, the trailer still names the MR.
    assert_eq!(
        read_merge_message(
            "fix: Typo",
            "fix: Typo\n\nSee merge request group/project!7"
        ),
        ("fix: Typo".into(), Some("7".into()))
    );
    // A direct push names none.
    assert_eq!(
        read_merge_message("docs: README", "docs: README\n"),
        ("docs: README".into(), None)
    );
}

#[test]
fn spans_follow_the_base_file() {
    // Line 11 replaced, then one line inserted before line 14.
    let patch = "--- a/f\n+++ b/f\n@@ -10,5 +10,6 @@\n ctx\n-old\n+new\n ctx\n ctx\n+added\n ctx";
    assert_eq!(changed_spans(patch), [(22, 23), (27, 27)]);
    // A new file: everything goes before line 1.
    assert_eq!(changed_spans("@@ -0,0 +1,2 @@\n+a\n+b"), [(1, 1)]);
}

#[test]
fn only_close_changes_clash() {
    let ours = [
        file("same_line.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+B\n c"),
        file("adjacent.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+B\n c"),
        file("far_apart.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+B\n c"),
        file("binary.png", ""),
        file("only_ours.rs", "@@ -1 +1 @@\n-a\n+b"),
    ];
    let theirs = [
        file("same_line.rs", "@@ -5,3 +5,3 @@\n a\n-b\n+X\n c"),
        file("adjacent.rs", "@@ -6,3 +6,3 @@\n b\n-c\n+C\n d"),
        file("far_apart.rs", "@@ -40,3 +40,3 @@\n a\n-b\n+B\n c"),
        file("binary.png", ""),
    ];
    let files = changed_on_both_sides(&ours, &theirs);
    let clash = |p: &str| files.iter().find(|f| f.path == p).map(|f| f.clash);
    assert_eq!(clash("same_line.rs"), Some(true));
    assert_eq!(clash("adjacent.rs"), Some(true));
    assert_eq!(clash("far_apart.rs"), Some(false));
    assert_eq!(clash("binary.png"), Some(true));
    assert_eq!(clash("only_ours.rs"), None);
    // Clashes first.
    assert_eq!(files.last().map(|f| f.path.as_str()), Some("far_apart.rs"));
}
