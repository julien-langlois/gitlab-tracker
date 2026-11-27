use super::*;
use crate::models::{MrData, Pipeline, PipelineJob, TrackedMr};

fn job(name: &str, status: &str) -> PipelineJob {
    PipelineJob {
        id: 0,
        name: name.into(),
        stage: "test".into(),
        status: status.into(),
        duration: Some(60.0),
        queued_duration: None,
        needs: None,
    }
}

#[test]
fn a_failure_then_a_pass_on_the_same_commit_is_flaky() {
    let mut app = crate::app::tests::test_app();
    let now = board::parse_time("2026-11-05T12:00:00Z").unwrap();
    let mut mr = TrackedMr::placeholder("1".into(), "A".into(), None);
    let pipeline = |id, at: &str, sha: &str, status, jobs| Pipeline {
        id,
        status,
        created_at: Some(at.into()),
        updated_at: Some(at.replace("00:00Z", "10:00Z")),
        sha: Some(sha.into()),
        jobs,
        needs_read: false,
    };
    mr.data = MrData {
        title: "A".into(),
        state: GitlabMrState::Opened,
        pipelines: vec![
            pipeline(
                3,
                "2026-11-04T10:00:00Z",
                "b",
                PipelineState::Failed,
                vec![job("unit", "failed"), job("e2e", "success")],
            ),
            pipeline(
                2,
                "2026-11-03T10:00:00Z",
                "a",
                PipelineState::Success,
                vec![job("unit", "success"), job("e2e", "success")],
            ),
            pipeline(
                1,
                "2026-11-03T09:00:00Z",
                "a",
                PipelineState::Failed,
                vec![job("unit", "success"), job("e2e", "failed")],
            ),
        ],
        ..MrData::default()
    };
    app.mrs = vec![mr];
    let ci = analyze(&app, now, 7);
    assert_eq!(ci.pipelines, 3);
    let e2e = ci.jobs.iter().find(|j| j.name == "e2e").unwrap();
    assert_eq!(e2e.flaky_runs, 1);
    let unit = ci.jobs.iter().find(|j| j.name == "unit").unwrap();
    // Failed on its last commit, never passed on it: not flaky, failing.
    assert_eq!(unit.flaky_runs, 0);
    assert!((unit.failure_rate - 1.0 / 3.0).abs() < 1e-9);
    // The MR is held back by its latest pipeline.
    assert_eq!(ci.blocked.len(), 1);
    assert_eq!(ci.blocked[0].jobs, vec!["unit".to_string()]);
    assert!((ci.success.unwrap() - 1.0 / 3.0).abs() < 1e-9);
    // Six jobs of a minute, four of them in the two failed pipelines.
    assert_eq!(ci.budget.runner_secs, 360.0);
    assert_eq!(ci.budget.failed_secs, 240.0);
    assert_eq!(ci.budget.per_pipeline, Some(120.0));
    assert_eq!(ci.budget.queued, None);
    assert_eq!(hours_text(5400.0), "1.5 h");
    assert_eq!(hours_text(600.0), "10 min");
}

#[tokio::test]
async fn the_demo_staging_is_red_since_the_registry_timed_out() {
    let app = crate::demo::demo_app(crate::config::AppConfig::default()).await;
    let overview = crate::demo::demo_project_overview(&app);
    let red = red_branches(&app, &overview.pipelines);
    assert_eq!(red.len(), 1, "{red:?}");
    assert_eq!(red[0].branch, "staging");
    assert_eq!(red[0].failed, 2);
    assert_eq!(red[0].jobs, ["deploy-staging"]);
    assert!(red[0].last_green.is_some());
    // The sync that turned it red brought no MR: an infrastructure failure.
    assert!(red[0].arrived.is_empty());
}

fn run(id: u64, status: PipelineState, at: &str, sha: &str) -> crate::gitlab_detail::PipelineRun {
    crate::gitlab_detail::PipelineRun {
        id,
        sha: sha.into(),
        status,
        created_at: at.into(),
        web_url: format!("https://g.io/p/{id}"),
    }
}

fn merged(id: &str, at: &str, sha: &str) -> TrackedMr {
    let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
    mr.data = MrData {
        title: format!("MR {id}"),
        state: GitlabMrState::Merged,
        target_branch: "main".into(),
        merged_at: Some(at.into()),
        sha: Some(sha.into()),
        ..MrData::default()
    };
    mr
}

fn branch(history: Vec<crate::gitlab_detail::PipelineRun>) -> crate::gitlab_detail::BranchPipeline {
    let latest = history[0].clone();
    crate::gitlab_detail::BranchPipeline {
        branch: "main".into(),
        id: latest.id,
        sha: latest.sha.clone(),
        status: latest.status,
        created_at: latest.created_at.clone(),
        updated_at: latest.created_at.clone(),
        web_url: latest.web_url.clone(),
        failed_jobs: vec!["unit-tests".into()],
        history,
    }
}

#[test]
fn a_branch_red_since_an_mr_names_it() {
    use PipelineState::*;
    let mut app = crate::app::tests::test_app();
    app.branches = vec!["main".into()];
    app.mrs = vec![
        merged("1", "2026-11-20T09:00:00Z", "aaa"),
        // Merged between the last green and the first red: the suspect.
        merged("2", "2026-11-23T10:00:00Z", "bbb"),
        merged("3", "2026-11-25T10:00:00Z", "ccc"),
    ];
    let history = vec![
        run(14, Running, "2026-11-26T09:00:00Z", "ddd"),
        run(13, Failed, "2026-11-25T10:05:00Z", "ccc"),
        run(12, Failed, "2026-11-23T10:05:00Z", "bbb"),
        run(11, Success, "2026-11-20T09:05:00Z", "aaa"),
    ];
    let red = red_branches(&app, &[branch(history)]);
    assert_eq!(red.len(), 1);
    // The running one is passed over: two red in a row since !2.
    assert_eq!(red[0].failed, 2);
    assert_eq!(red[0].first_red.id, 12);
    assert_eq!(red[0].last_green.as_ref().map(|g| g.id), Some(11));
    assert_eq!(red[0].arrived, [("2".to_string(), "MR 2".to_string())]);
}

#[test]
fn one_red_pipeline_or_a_green_one_last_is_no_banner() {
    use PipelineState::*;
    let mut app = crate::app::tests::test_app();
    app.branches = vec!["main".into()];
    // A flaky failure alone.
    let once = vec![
        run(12, Failed, "2026-11-23T10:05:00Z", "bbb"),
        run(11, Success, "2026-11-20T09:05:00Z", "aaa"),
    ];
    assert!(red_branches(&app, &[branch(once)]).is_empty());
    // Green again.
    let fixed = vec![
        run(13, Success, "2026-11-25T10:05:00Z", "ccc"),
        run(12, Failed, "2026-11-23T10:05:00Z", "bbb"),
        run(11, Failed, "2026-11-20T09:05:00Z", "aaa"),
    ];
    assert!(red_branches(&app, &[branch(fixed)]).is_empty());
}
