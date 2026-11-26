//! Pipelines over GraphQL, split when too complex, REST on older GitLab.

use super::*;

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
