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
