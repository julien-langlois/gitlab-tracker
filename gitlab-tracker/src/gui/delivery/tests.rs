use super::*;
use crate::models::{MrData, ReviewActivity};

fn at(s: &str) -> DateTime<Utc> {
    board::parse_time(s).unwrap()
}

#[test]
fn a_path_cuts_its_time_into_the_steps_it_went_through() {
    let path = Path {
        id: "1".into(),
        title: String::new(),
        author: String::new(),
        created: at("2026-11-01T00:00:00Z"),
        ready: Some(at("2026-11-02T00:00:00Z")),
        never_draft: true,
        draft_comments: 0,
        reviewed: Some(at("2026-11-04T00:00:00Z")),
        approved: None,
        merged: Some(at("2026-11-05T00:00:00Z")),
        delivered: Some(at("2026-11-07T00:00:00Z")),
        first_commit: None,
        rounds: Some(0),
    };
    let stages = path.stages(at("2026-11-10T00:00:00Z"));
    // No approval: its time goes to the merge.
    assert_eq!(stages, [Some(1.0), Some(2.0), None, Some(1.0), Some(2.0)]);
    assert_eq!(path.total(), Some(6.0));
}

#[test]
fn an_open_mr_counts_its_current_step_up_to_now() {
    let path = Path {
        id: "1".into(),
        title: String::new(),
        author: String::new(),
        created: at("2026-11-01T00:00:00Z"),
        ready: Some(at("2026-11-02T00:00:00Z")),
        never_draft: true,
        draft_comments: 0,
        reviewed: None,
        approved: None,
        merged: None,
        delivered: None,
        first_commit: None,
        rounds: None,
    };
    let stages = path.stages(at("2026-11-05T00:00:00Z"));
    assert_eq!(stages, [Some(1.0), Some(3.0), None, None, None]);
}

#[test]
fn the_dora_bands() {
    assert_eq!(Level::of_frequency(10.0), Level::Elite);
    assert_eq!(Level::of_frequency(2.0), Level::High);
    assert_eq!(Level::of_frequency(0.5), Level::Medium);
    assert_eq!(Level::of_frequency(0.1), Level::Low);
    assert_eq!(Level::of_lead(0.5), Level::Elite);
    assert_eq!(Level::of_lead(40.0), Level::Low);
    assert_eq!(Level::of_failure_rate(0.05), Level::Elite);
    assert_eq!(Level::of_failure_rate(0.3), Level::Low);
    assert_eq!(Level::of_restore(0.5), Level::High);
}

#[test]
fn a_late_push_without_a_count_is_one_round() {
    let mut mr = TrackedMr::placeholder("1".into(), "A".into(), None);
    mr.data = MrData {
        review: Some(ReviewActivity {
            first_review_at: Some("2026-11-01T10:00:00Z".into()),
            last_push_at: Some("2026-11-02T10:00:00Z".into()),
            ..ReviewActivity::default()
        }),
        ..MrData::default()
    };
    assert_eq!(rounds_of(&mr), Some(1));
    mr.data.review.as_mut().unwrap().pushes_after_review = 3;
    assert_eq!(rounds_of(&mr), Some(3));
}

#[test]
fn a_change_says_which_way_and_whether_it_is_better() {
    // A shorter lead time is better.
    assert_eq!(
        versus(7.2, 8.9, (18, 15), false, 0.05),
        Versus::Moved {
            up: false,
            better: true
        }
    );
    // More failures is worse.
    assert_eq!(
        versus(0.38, 0.25, (8, 9), false, 0.005),
        Versus::Moved {
            up: true,
            better: false
        }
    );
    // Two incidents against one: too few to tell.
    assert_eq!(
        versus(3.6, 2.1, (2, 1), false, 0.05),
        Versus::TooFew { up: true }
    );
    assert_eq!(versus(1.0, 1.02, (9, 9), true, 0.05), Versus::Same);
}

#[tokio::test]
async fn the_period_before_counts_nothing_of_the_current_one() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let overview = crate::demo::demo_project_overview(&app);
    let now = Utc::now();
    let current = analyze(&app, Some(&overview), now, 30);
    let before = analyze(&app, Some(&overview), now - Duration::days(30), 30);
    let start = now - Duration::days(30);
    assert!(before
        .merged
        .iter()
        .all(|p| p.merged.is_some_and(|m| m <= start)));
    assert!(current
        .merged
        .iter()
        .all(|p| p.merged.is_some_and(|m| m >= start)));
    let ids = |d: &Delivery| d.merged.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert!(ids(&before).iter().all(|id| !ids(&current).contains(id)));
    assert!(
        !before.merged.is_empty(),
        "the demo has merges in the period before"
    );
}

#[tokio::test]
async fn the_demo_has_a_delivery_to_show() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let now = Utc::now();
    let overview = crate::demo::demo_project_overview(&app);
    let d = analyze(&app, Some(&overview), now, 30);
    assert!(d.merged.len() >= 5, "{}", d.merged.len());
    assert!(d.dora.has_deployments);
    assert!(d.dora.lead.is_some());
    assert!(!d.dora.reverts.is_empty());
    assert!(d.dora.restore.is_some());
    assert!(d.rounds.reviewed > 0);
    assert!(slowest(&d.medians).is_some());
    let ci = super::super::ci::analyze(&app, now, 7);
    let report = weekly_report(&app, Some(&overview), &ci, now);
    assert!(report.contains("Deployment frequency"), "{report}");
    assert!(report.contains("v2.4.0"), "{report}");
}

fn merged(id: &str, title: &str) -> TrackedMr {
    let mut mr = TrackedMr::placeholder(id.into(), title.into(), None);
    mr.data.author = "Alex <@alex>".into();
    mr.data.state = GitlabMrState::Merged;
    mr
}

fn commit(sha: &str, title: &str) -> crate::models::MrCommit {
    crate::models::MrCommit {
        sha: sha.into(),
        fingerprint: crate::models::CommitFingerprint {
            author_email: "alex@example.com".into(),
            authored_at: 0,
            title: title.into(),
        },
        author_name: "Alex".into(),
    }
}

async fn app_with(mrs: Vec<TrackedMr>) -> App {
    let mut app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    app.mrs = mrs;
    app
}

#[tokio::test]
async fn a_revert_finds_what_it_undoes_by_description_commit_or_title() {
    let mut original = merged("12", "feat: quote cache");
    original.data.commits = vec![commit("1a2b3c4d5e6f", "feat: quote cache")];
    let other = merged("13", "feat: something else");

    // GitLab's Revert button: the description names the MR.
    let mut by_button = merged("20", "Revert \"feat: something else\"");
    by_button.data.description = "This reverts merge request !12".into();
    // `git revert`: the description (the commit's body) names the commit.
    let mut by_commit = merged("21", "Revert the cache");
    by_commit.data.description = "This reverts commit 1a2b3c4d5e6f0011.".into();
    // Only the title.
    let by_title = merged("22", "Revert \"feat: quote cache\"");
    // A commit's title, under an MR named otherwise.
    let mut by_commit_title = merged("23", "revert: back to the old cache");
    by_commit_title.data.commits = vec![commit("ffff", "Revert \"feat: quote cache\"")];
    // Not a revert.
    let mut fix = merged("24", "fix: cache size");
    fix.data.description = "This reverts merge request !12".into();

    let app = app_with(vec![
        original,
        other,
        by_button.clone(),
        by_commit.clone(),
        by_title.clone(),
        by_commit_title.clone(),
        fix.clone(),
    ])
    .await;
    let found = |mr: &TrackedMr| reverted(&app, mr).map(|(m, how)| (m.id.clone(), how));
    // The description wins over the quoted title.
    assert_eq!(found(&by_button), Some(("12".into(), Found::Description)));
    assert_eq!(found(&by_commit), Some(("12".into(), Found::Commit)));
    assert_eq!(found(&by_title), Some(("12".into(), Found::Title)));
    assert_eq!(found(&by_commit_title), Some(("12".into(), Found::Title)));
    assert_eq!(found(&fix), None);
}

#[tokio::test]
async fn hotfixes_follow_the_project_branches_and_the_label() {
    let mut app = app_with(vec![]).await;
    let mut mr = merged("30", "fix: login");
    mr.data.source_branch = "hotfix/login".into();
    assert!(is_hotfix(&app, &mr));
    app.config.hotfix_branches = "fix/prod-*, urgent-*".into();
    assert!(!is_hotfix(&app, &mr));
    mr.data.source_branch = "fix/prod-login".into();
    assert!(is_hotfix(&app, &mr));
    app.config.hotfix_branches = String::new();
    assert!(!is_hotfix(&app, &mr));
    mr.data.labels = vec!["Hotfix".into()];
    assert!(is_hotfix(&app, &mr));
}

#[tokio::test]
async fn the_demo_lists_its_revert_and_hotfixes() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let overview = crate::demo::demo_project_overview(&app);
    let d = analyze(&app, Some(&overview), Utc::now(), 90);
    let revert = d
        .undos
        .iter()
        .find(|u| u.kind == UndoKind::Revert)
        .expect("the demo has a revert");
    let (_, title, _) = revert.undoes.as_ref().expect("it finds what it undoes");
    assert!(title.contains("LRU eviction"), "{title}");
    assert!(revert.after.is_some());
    assert!(d.undos.iter().any(|u| u.kind == UndoKind::Hotfix));
    // What the change failure rate counts is in the list.
    for id in d.dora.reverts.iter().chain(&d.dora.hotfixes) {
        assert!(d.undos.iter().any(|u| &u.id == id && u.counted), "!{id}");
    }
}
