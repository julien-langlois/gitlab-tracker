//! Branches: presence, what a branch lacks, new arrivals, the commit index.

use super::*;

#[tokio::test]
async fn branch_presence_reports_branch_commits_separately() {
    let mut app = app_with_mrs(&[("fix login", "None")]).await;
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[0].data.sha = Some("abc".into());
    let picked = BranchCommits {
        picked_from: HashMap::from([("abc".to_string(), 0)]),
        ..BranchCommits::default()
    };
    app.branch_commits.insert("staging".into(), picked);
    let mr = &app.mrs[0];

    assert_eq!(
        app.branch_presence(mr, "main"),
        Some(BranchPresence::Present)
    );
    assert_eq!(
        app.branch_presence(mr, "staging"),
        Some(BranchPresence::CherryPicked)
    );
    assert_eq!(
        app.branch_presence(mr, "prod"),
        Some(BranchPresence::Absent)
    );
}
#[tokio::test]
async fn behind_on_lists_the_merges_of_the_parent_missing_from_the_feature() {
    let mut app = app_with_mrs(&[
        ("on develop", "None"),
        ("synced", "None"),
        ("hotfix", "None"),
    ])
    .await;
    app.branches = ["master", "develop", "redesign_develop"]
        .map(String::from)
        .to_vec();
    let merged = |found: &[&str]| MrStatus::MergedIn(found.iter().map(|b| b.to_string()).collect());
    app.mrs[0].status = merged(&["develop"]);
    app.mrs[0].data.target_branch = "develop".into();
    app.mrs[1].status = merged(&["develop", "redesign_develop"]);
    app.mrs[1].data.target_branch = "develop".into();
    app.mrs[2].status = merged(&["master"]);
    app.mrs[2].data.target_branch = "master".into();
    for mr in &mut app.mrs {
        mr.data.state = GitlabMrState::Merged;
    }
    // Loaded (so `MergedIn`) but still in review, or closed: nothing to bring in.
    for (title, state) in [
        ("in review", GitlabMrState::Opened),
        ("closed", GitlabMrState::Closed),
    ] {
        let mut mr = app.mrs[0].clone();
        mr.data.title = title.into();
        mr.data.state = state;
        mr.status = merged(&[]);
        app.mrs.push(mr);
    }

    let ids = |mrs: Vec<&TrackedMr>| mrs.iter().map(|mr| mr.title.clone()).collect::<Vec<_>>();
    // The hotfix comes down to develop, then to the feature branch.
    assert_eq!(
        ids(app.behind_on("redesign_develop")),
        ["on develop", "hotfix"]
    );
    assert_eq!(ids(app.behind_on("develop")), ["hotfix"]);
    assert!(app.behind_on("master").is_empty());
}
#[tokio::test]
async fn only_mrs_that_just_reached_a_branch_are_reported() {
    let mut app = app_with_mrs(&[("a", "None"), ("b", "None"), ("c", "None")]).await;
    for (mr, sha) in app.mrs.iter_mut().zip(["s1", "s2", "s3"]) {
        mr.status = MrStatus::MergedIn(HashSet::new());
        mr.data.sha = Some(sha.into());
    }
    app.mrs[2].status = MrStatus::MergedIn(HashSet::from(["prod".to_string()]));
    let picked = |shas: &[&str]| BranchCommits {
        picked_from: shas.iter().map(|s| (s.to_string(), 0)).collect(),
        ..BranchCommits::default()
    };
    let (before, after) = (picked(&["s1"]), picked(&["s1", "s2", "s3"]));

    let reached: Vec<(&str, &str)> = newly_copied(&app.mrs, "prod", &before, &after)
        .map(|(mr, how)| (mr.id.as_str(), how))
        .collect();
    // s1 was already picked; s3's MR is on prod by SHA anyway.
    assert_eq!(reached, [("2", "cherry-picked")]);
}
#[tokio::test]
async fn branch_rows_list_the_commits_missing_from_a_partial_port() {
    use crate::models::{CommitFingerprint, MrCommit};
    let mut app = app_with_mrs(&[("a", "None")]).await;
    app.branches = vec!["main".into(), "prod".into()];
    let commit = |sha: &str| MrCommit {
        sha: sha.into(),
        fingerprint: CommitFingerprint {
            author_email: "dev@example.com".into(),
            authored_at: 0,
            title: sha.into(),
        },
        author_name: String::new(),
    };
    app.mrs[0].status = MrStatus::MergedIn(HashSet::from(["main".to_string()]));
    app.mrs[0].data.commits = vec![commit("c1"), commit("c2")];
    let prod = BranchCommits {
        shas: HashSet::from(["c1".to_string()]),
        ..BranchCommits::default()
    };
    app.branch_commits.insert("prod".into(), prod);

    let rows = app.branch_rows(&app.mrs[0]);
    assert_eq!(rows[0].presence, Some(BranchPresence::Present));
    assert!(rows[0].missing.is_empty());
    assert_eq!(rows[1].presence, Some(BranchPresence::Partial));
    let missing: Vec<&str> = rows[1].missing.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(missing, ["c2"]);
}
#[test]
fn reached_branches_skip_first_sighting_target_and_new_columns() {
    let set = |bs: &[&str]| bs.iter().map(|b| b.to_string()).collect::<HashSet<_>>();
    let found = set(&["develop", "preprod", "master", "release"]);
    let just_added = set(&["release"]);

    assert!(reached_branches(None, &found, "develop", &just_added).is_empty());
    let reached = reached_branches(Some(&set(&["develop"])), &found, "develop", &just_added);
    assert_eq!(
        set(&reached.iter().map(String::as_str).collect::<Vec<_>>()),
        set(&["preprod", "master"])
    );
}
#[test]
fn new_branch_notifications_skip_first_sighting_at_startup() {
    let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
    let main = set(&["main"]);
    // First launch, startup sync: silent.
    assert!(branches_to_notify(None, &main, false).is_empty());
    // Known open MR (empty set) that merged while the app was closed: notifies.
    assert_eq!(branches_to_notify(Some(&set(&[])), &main, false), ["main"]);
    // Already known on main: nothing new.
    assert!(branches_to_notify(Some(&main), &main, true).is_empty());
    // MR added after startup: notifies as before.
    assert_eq!(branches_to_notify(None, &main, true), ["main"]);
}
#[tokio::test]
async fn a_range_scan_grows_the_index_a_full_one_replaces_it() {
    use crate::gitlab::BranchScan;
    let mut app = test_app();
    let full = BranchScan::Full {
        since: "2026-10-01T00:00:00+00:00".into(),
    };
    scanned(&mut app, Some(shas(&["a", "b"])), full.clone(), "b");
    let scope = &app.branch_scan_scopes["prod"];
    assert_eq!(scope.head.as_deref(), Some("b"));
    assert_eq!(scope.floor.to_rfc3339(), "2026-10-01T00:00:00+00:00");

    // Only the new commits were read: added to what is known.
    scanned(
        &mut app,
        Some(shas(&["c"])),
        BranchScan::Range { from: "b".into() },
        "c",
    );
    assert_eq!(app.branch_commits["prod"].shas.len(), 3);
    assert_eq!(app.branch_scan_scopes["prod"].head.as_deref(), Some("c"));

    // A failed range read (head gone after a force-push?): next scan is full.
    scanned(&mut app, None, BranchScan::Range { from: "c".into() }, "c");
    assert_eq!(app.branch_commits["prod"].shas.len(), 3); // index kept meanwhile
    assert!(!app.branch_scan_scopes.contains_key("prod"));

    // A full scan rebuilds the index from scratch.
    scanned(&mut app, Some(shas(&["z"])), full, "z");
    assert_eq!(app.branch_commits["prod"], shas(&["z"]));
}
