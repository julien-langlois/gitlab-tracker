use super::*;
use crate::models::{DiffStats, MrData, TrackedMr};

fn mr(id: &str, files: Vec<ChangedFile>) -> TrackedMr {
    let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
    mr.data = MrData {
        title: format!("MR {id}"),
        author: format!("Dev {id} (@dev{id})"),
        state: GitlabMrState::Opened,
        diff_stats: Some(DiffStats {
            files_changed: files.len() as u32,
            additions: 10,
            commits_count: 1,
            files,
            files_listed: true,
            ..DiffStats::default()
        }),
        ..MrData::default()
    };
    mr.mergeability = MergeabilityStatus::Mergeable;
    mr
}

fn file(path: &str, hunks: &[(u32, u32)]) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        hunks: hunks.to_vec(),
        ..ChangedFile::default()
    }
}

#[test]
fn pairs_by_how_they_meet() {
    let mut app = crate::app::tests::test_app();
    app.mrs = vec![
        mr(
            "1",
            vec![
                file("src/a.rs", &[(10, 20)]),
                file("src/b.rs", &[(1, 5)]),
                file("Cargo.lock", &[(1, 9)]),
            ],
        ),
        mr(
            "2",
            vec![file("src/a.rs", &[(15, 30)]), file("Cargo.lock", &[(1, 9)])],
        ),
        mr(
            "3",
            vec![
                file("src/b.rs", &[(11, 12)]),
                file("src/a.rs", &[(200, 210)]),
            ],
        ),
    ];
    let o = analyze(&app, false, false);
    assert_eq!(o.mrs.len(), 3);
    // The lock file is left out.
    assert!(o.files.iter().all(|f| f.path != "Cargo.lock"));
    assert_eq!(o.pair("1", "2").unwrap().level(), Level::SameLines);
    // b.rs lines 5 and 11 are close; a.rs far apart.
    assert_eq!(o.pair("1", "3").unwrap().level(), Level::Near);
    assert_eq!(o.pair("2", "3").unwrap().level(), Level::SameFile);
    assert_eq!(o.pairs[0].level(), Level::SameLines);
    // With the lock files, !1 and !2 share one more file.
    let with = analyze(&app, true, false);
    assert_eq!(with.pair("1", "2").unwrap().files.len(), 2);
}

#[test]
fn common_lines_are_merged() {
    assert_eq!(common_lines(&[(1, 10)], &[(5, 20), (8, 12)]), vec![(5, 10)]);
    assert_eq!(
        common_lines(&[(1, 3), (10, 12)], &[(3, 10)]),
        vec![(3, 3), (10, 10)]
    );
    assert!(common_lines(&[(1, 3)], &[(5, 9)]).is_empty());
}

#[test]
fn folders_two_levels_deep() {
    assert_eq!(folder("src/api/graphql/schema.rs"), "src/api/");
    assert_eq!(folder("Cargo.toml"), "/");
    assert_eq!(folder("config/default.toml"), "config/");
}

#[test]
fn lookups_find_an_mr_a_pair_in_either_order_and_a_file() {
    let mut app = crate::app::tests::test_app();
    app.mrs = vec![
        mr("1", vec![file("src/a.rs", &[(10, 20)])]),
        mr("2", vec![file("src/a.rs", &[(15, 25)])]),
        mr("3", vec![file("src/c.rs", &[(1, 2)])]),
    ];
    let o = analyze(&app, false, true);
    assert_eq!(o.mr("2").map(|m| m.id.as_str()), Some("2"));
    assert!(o.mr("9").is_none());
    let pair = o.pair("1", "2").expect("1 and 2 share src/a.rs");
    assert!(std::ptr::eq(pair, o.pair("2", "1").expect("either order")));
    assert!(o.pair("1", "3").is_none());
    assert_eq!(o.file("src/a.rs").map(|f| f.mrs.len()), Some(2));
    assert!(o.file("src/c.rs").is_none(), "changed by one MR only");
}

#[test]
fn the_matrix_takes_the_ten_lowest_ids_and_my_pairs_are_counted_once() {
    let mut app = crate::app::tests::test_app();
    app.config.gitlab_username = Some("dev3".into());
    // Twelve MRs on the same file: every two of them are a pair.
    app.mrs = (1..=12)
        .map(|i| mr(&i.to_string(), vec![file("src/a.rs", &[(1, 5)])]))
        .collect();
    let o = analyze(&app, false, false);
    let expected: Vec<String> = (1..=10).map(|i| i.to_string()).collect();
    // By number, not as text ("10" after "9", "11" and "12" left out).
    assert_eq!(o.matrix, expected);
    // !3 is in a pair with each of the eleven others.
    assert_eq!(o.my_pairs, 11);
}

/// An open MR last pushed at `pushed`, and an MR merged into the same target
/// at `merged`, each with its files.
fn ahead_pair(
    mine: Vec<ChangedFile>,
    theirs: Vec<ChangedFile>,
    pushed: &str,
    merged: &str,
) -> (crate::app::App, TrackedMr) {
    let mut open = mr("10", mine);
    open.data.target_branch = "main".into();
    open.data.review = Some(crate::models::ReviewActivity {
        last_push_at: Some(pushed.into()),
        ..Default::default()
    });
    let mut done = mr("20", theirs);
    done.data.target_branch = "main".into();
    done.data.state = GitlabMrState::Merged;
    done.data.merged_at = Some(merged.into());
    done.mergeability = MergeabilityStatus::NotOpen;
    let mut app = crate::app::tests::test_app();
    app.mrs = vec![open.clone(), done];
    (app, open)
}

#[test]
fn a_merge_on_the_same_lines_since_the_last_push_is_a_conflict_to_come() {
    let (app, open) = ahead_pair(
        vec![file("src/a.rs", &[(18, 22)]), file("src/b.rs", &[(1, 3)])],
        vec![file("src/a.rs", &[(12, 30)]), file("src/b.rs", &[(40, 50)])],
        "2026-11-20T10:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    let found = ahead(&app, &open);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].merged, "20");
    // Only the file met on the same lines; b.rs is apart.
    assert_eq!(found[0].files.len(), 1);
    assert_eq!(found[0].files[0].path, "src/a.rs");
    assert_eq!(lines_text(&found[0].files[0].theirs), "12–30");
}

#[test]
fn no_conflict_to_come_when_the_branch_already_has_it_or_lines_differ() {
    // Merged before the last push: the rebase took it in.
    let (app, open) = ahead_pair(
        vec![file("src/a.rs", &[(18, 22)])],
        vec![file("src/a.rs", &[(12, 30)])],
        "2026-11-25T10:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    assert!(ahead(&app, &open).is_empty());
    // Lines a few apart: git merges them.
    let (app, open) = ahead_pair(
        vec![file("src/a.rs", &[(40, 42)])],
        vec![file("src/a.rs", &[(12, 30)])],
        "2026-11-20T10:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    assert!(ahead(&app, &open).is_empty());
    // Another target: not in its way.
    let (mut app, open) = ahead_pair(
        vec![file("src/a.rs", &[(18, 22)])],
        vec![file("src/a.rs", &[(12, 30)])],
        "2026-11-20T10:00:00Z",
        "2026-11-24T10:00:00Z",
    );
    app.mrs[1].data.target_branch = "develop".into();
    assert!(ahead(&app, &open).is_empty());
    // Already in conflict: GitLab says so, nothing to foresee.
    app.mrs[1].data.target_branch = "main".into();
    let mut conflicted = open.clone();
    conflicted.mergeability = MergeabilityStatus::Conflict;
    assert!(ahead(&app, &conflicted).is_empty());
    assert_eq!(ahead(&app, &open).len(), 1);
}

#[tokio::test]
async fn the_demo_shows_a_conflict_to_come() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let index = app
        .mrs
        .iter()
        .find(|m| m.title.starts_with("perf(db): Index shipments"))
        .expect("the demo's !215");
    let found = ahead(&app, index);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].title.contains("NULL tracking numbers"));
    assert_eq!(found[0].files[0].path, "src/db/shipments.rs");
}
