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
