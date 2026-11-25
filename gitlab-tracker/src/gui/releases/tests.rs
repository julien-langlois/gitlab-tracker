use super::*;

fn commit(id: &str, message: &str, parents: &[&str]) -> ApiCommit {
    ApiCommit {
        id: id.into(),
        message: message.into(),
        parent_ids: parents.iter().map(|p| p.to_string()).collect(),
        committed_date: Some("2026-11-10T10:00:00Z".into()),
        ..ApiCommit::default()
    }
}

#[test]
fn kinds_come_from_the_title() {
    assert_eq!(Kind::of("feat(api): Add x"), Kind::Feature);
    assert_eq!(Kind::of("feat(api)!: Drop v1"), Kind::Breaking);
    assert_eq!(Kind::of("fix: the footer"), Kind::Fix);
    assert_eq!(Kind::of("chore(deps): Bump tokio"), Kind::Other);
    assert_eq!(Kind::of("Add the dark mode"), Kind::Feature);
    assert_eq!(Kind::of("Revert \"feat: x\""), Kind::Fix);
    assert_eq!(Kind::of("Mise en page"), Kind::Other);
    assert_eq!(
        Kind::of("refactor: x\n\nBREAKING CHANGE: y"),
        Kind::Breaking
    );
    // A colon later in a sentence is no conventional head.
    assert_eq!(Kind::of("Fix the docs: typo"), Kind::Fix);
}

#[test]
fn titles_lose_their_head_in_the_notes() {
    assert_eq!(clean_title("feat(api): add x"), "api: Add x");
    assert_eq!(clean_title("fix: the footer"), "The footer");
    assert_eq!(clean_title("Add the dark mode"), "Add the dark mode");
}

#[test]
fn versions_move_by_what_waits() {
    assert_eq!(
        next_version("v2.3.3", Bump::Minor).as_deref(),
        Some("v2.4.0")
    );
    assert_eq!(next_version("3.7.0", Bump::Patch).as_deref(), Some("3.7.1"));
    assert_eq!(
        next_version("v1.9.2", Bump::Major).as_deref(),
        Some("v2.0.0")
    );
    // Before 1.0 a breaking change moves the minor.
    assert_eq!(next_version("0.4.1", Bump::Major).as_deref(), Some("0.5.0"));
    assert_eq!(next_version("v2.0", Bump::Patch).as_deref(), Some("v2.0.1"));
    assert_eq!(
        next_version("v3.0.0-rc.2", Bump::Patch).as_deref(),
        Some("v3.0.1")
    );
    assert_eq!(next_version("deploy-2026", Bump::Patch), None);
}

#[test]
fn merge_commits_name_their_mr_and_take_its_commits() {
    let app = crate::app::tests::test_app();
    // Newest first: a merge of !41 (with its two commits), a commit
    // pushed straight, a merge of !40.
    let commits = vec![
        commit(
            "m41",
            "Merge branch 'pay' into 'main'\n\nfeat(pay): Apple Pay\n\nSee merge request orbit/app!41",
            &["d1", "c2"],
        ),
        commit("c2", "wip", &["c1"]),
        commit("c1", "start", &["m40"]),
        commit("d1", "chore(release): 3.8.0", &["m40"]),
        commit(
            "m40",
            "Merge branch 'x' into 'main'\n\nfix: Crash on start\n\nSee merge request orbit/app!40",
            &["base", "x1"],
        ),
    ];
    let c = contents(&app, &commits, false);
    let ids: Vec<&str> = c.mrs.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["40", "41"]);
    assert_eq!(c.mrs[1].title, "feat(pay): Apple Pay");
    assert_eq!(c.mrs[1].kind, Kind::Feature);
    assert!(!c.mrs[1].tracked);
    // c1 and c2 are !41's; d1 alone came through no MR.
    assert_eq!(c.direct, 1);
}

#[test]
fn notes_have_a_section_per_kind() {
    let item = |id: &str, title: &str| Item {
        id: id.into(),
        title: title.into(),
        kind: Kind::of(title),
        tracked: true,
        at: None,
        ticket: None,
    };
    let out = notes(
        "v2.4.0",
        &[
            item("1", "fix: Crash"),
            item("2", "feat(api): Add x"),
            item("3", "docs: Readme"),
        ],
        true,
    );
    assert_eq!(
        out,
        "## v2.4.0\n\n### Features\n\n- api: Add x (!2)\n\n### Fixes\n\n- Crash (!1)\n\n### Other changes\n\n- Readme (!3)\n"
    );
}

#[test]
fn the_preview_shows_the_labels_of_links() {
    assert_eq!(
        unlink("[Feature #12](https://x/12) - Title (!4)"),
        "Feature #12 - Title (!4)"
    );
    assert_eq!(unlink("no link [here"), "no link [here");
}

#[test]
fn tickets_lead_the_lines_when_asked() {
    let mut item = Item {
        id: "4220".into(),
        title: "feat: New Greece website".into(),
        kind: Kind::Feature,
        tracked: true,
        at: None,
        ticket: Some(Ticket {
            id: "1406385".into(),
            kind: Some("Feature".into()),
            url: "https://redmine.example/issues/1406385".into(),
        }),
    };
    let with = notes("3.5.0", std::slice::from_ref(&item), true);
    assert!(
        with.contains("- [Feature #1406385](https://redmine.example/issues/1406385) - New Greece website (!4220)\n"),
        "{with}"
    );
    let without = notes("3.5.0", std::slice::from_ref(&item), false);
    assert!(
        without.contains("- New Greece website (!4220)\n"),
        "{without}"
    );
    // A Jira-like key keeps its form, with no kind.
    item.ticket = Some(Ticket {
        id: "PROJ-42".into(),
        kind: None,
        url: String::new(),
    });
    let jira = notes("3.5.0", &[item], true);
    assert!(
        jira.contains("- PROJ-42 - New Greece website (!4220)\n"),
        "{jira}"
    );
}

#[tokio::test]
async fn the_demo_shows_its_releases_and_what_waits() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let overview = crate::demo::demo_project_overview(&app);
    assert!(overview.releases.len() >= 10);
    assert_eq!(overview.releases[0].tag, "v2.3.3");
    let read = |range: &str| {
        let (commits, truncated) = crate::demo::demo_range(&app, range);
        Some(Read::Ready(contents(&app, &commits, truncated)))
    };
    let m = analyze(&app, Some(&overview), &read, Utc::now());
    assert_eq!(m.integration, "main");
    // The latest release went to production.
    assert!(m.releases[0].deployed.iter().any(|e| e == "production"));
    let held = m.releases[0]
        .contents
        .as_ref()
        .and_then(Read::ready)
        .unwrap();
    assert!(!held.mrs.is_empty(), "the hotfixes are in v2.3.3");
    let pending = m.pending.as_ref().and_then(Read::ready).unwrap();
    assert!(!pending.mrs.is_empty());
    // A fix picked into the release does not wait for the next one.
    for item in &held.mrs {
        assert!(!pending.mrs.iter().any(|p| p.id == item.id), "!{}", item.id);
    }
    assert!(m.next.is_some());
    assert!(m.cadence.is_some());
}
