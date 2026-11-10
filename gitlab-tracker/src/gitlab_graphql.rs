//! GitLab GraphQL reads that replace per-MR REST loops (see `GRAPHQL.md`).
//!
//! Step 1 — pipelines and jobs: REST needs one request for an MR's pipelines plus
//! one per pipeline for its jobs (up to 6 per MR); one GraphQL query returns both
//! for a batch of MRs. Everything else stays on REST.
//!
//! Robustness first, since a GraphQL query fails as a whole:
//! * **Too complex** (GitLab caps query complexity and depth): the batch is split
//!   in two and retried; the smaller size is remembered for the instance.
//! * **Unknown field or argument** (an older GitLab): GraphQL pipelines are turned
//!   off for that instance for the rest of the session.
//! * **Anything else** (network, 5xx, partial data with errors): nothing is learnt.
//!
//! In every case an MR GraphQL did not answer for reads its pipelines over REST
//! (`gitlab::fetch_pipelines`), exactly as before. `GITLAB_TRACKER_GRAPHQL=off`
//! turns GraphQL off altogether.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use gitlab_tracker_core::pacing::{self, SendPaced};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::gitlab::{http_client, FetchContext};
use crate::models::{Pipeline, PipelineJob, PipelineState};

/// Pipelines read per MR: the same window as the REST read (`per_page=5`).
pub const PIPELINES_PER_MR: usize = 5;
/// Jobs read per pipeline: the same as the REST read (`per_page=50`).
pub const JOBS_PER_PIPELINE: usize = 50;
/// MRs per query until the instance says it is too much.
pub const INITIAL_BATCH: usize = 10;

/// What this process knows of an instance's GraphQL API, for pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Usable, with at most this many MRs per query.
    Batch(usize),
    /// Not usable (turned off, or the schema lacks a field): REST only.
    Off,
}

fn registry() -> &'static Mutex<HashMap<String, Support>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Support>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// `GITLAB_TRACKER_GRAPHQL=off` (or `0`, `false`) keeps every read on REST.
fn turned_off() -> bool {
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| {
        std::env::var("GITLAB_TRACKER_GRAPHQL")
            .is_ok_and(|v| matches!(v.trim().to_lowercase().as_str(), "off" | "0" | "false"))
    })
}

/// How pipelines are read from `base_url`'s instance right now.
pub fn support(base_url: &str) -> Support {
    if turned_off() {
        return Support::Off;
    }
    let registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    registry
        .get(&pacing::instance_key(base_url))
        .copied()
        .unwrap_or(Support::Batch(INITIAL_BATCH))
}

fn learn(base_url: &str, support: Support) {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.insert(pacing::instance_key(base_url), support);
}

/// How a project is named in a query: its path, or its numeric id as a global ID.
#[derive(Debug, PartialEq, Eq)]
enum ProjectRef {
    Path(String),
    Id(String),
}

fn project_ref(project_id: &str) -> ProjectRef {
    let id = project_id.trim();
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
        ProjectRef::Id(format!("gid://gitlab/Project/{id}"))
    } else {
        // Paths may be configured URL-encoded, as REST takes them (`group%2Fproject`).
        let path = urlencoding::decode(id).map_or_else(|_| id.to_string(), |p| p.into_owned());
        ProjectRef::Path(path)
    }
}

/// The MR fields read: its pipelines with their jobs. Retried jobs are left out,
/// like the REST jobs list does by default.
fn fragment() -> String {
    format!(
        "fragment MrPipelines on MergeRequest {{ iid \
           pipelines(first: {PIPELINES_PER_MR}) {{ nodes {{ id status createdAt updatedAt sha \
             jobs(first: {JOBS_PER_PIPELINE}, retried: false) {{ nodes {{ \
               id name status duration queuedDuration stage {{ name }} }} }} }} }} }}"
    )
}

/// The query and its variables for the MRs `iids` of `project_id`.
pub(crate) fn request_body(project_id: &str, iids: &[&str]) -> Value {
    let mrs = "mergeRequests(iids: $iids, first: $first) { nodes { ...MrPipelines } }";
    let (query, variables) = match project_ref(project_id) {
        ProjectRef::Path(path) => (
            format!(
                "query($project: ID!, $iids: [String!], $first: Int) \
                 {{ project(fullPath: $project) {{ {mrs} }} }} {}",
                fragment()
            ),
            serde_json::json!({ "project": path, "iids": iids, "first": iids.len() }),
        ),
        ProjectRef::Id(gid) => (
            format!(
                "query($ids: [ID!], $iids: [String!], $first: Int) \
                 {{ projects(ids: $ids) {{ nodes {{ {mrs} }} }} }} {}",
                fragment()
            ),
            serde_json::json!({ "ids": [gid], "iids": iids, "first": iids.len() }),
        ),
    };
    serde_json::json!({ "query": query, "variables": variables })
}

#[derive(Debug, Deserialize)]
pub(crate) struct Response {
    data: Option<Value>,
    #[serde(default)]
    errors: Vec<ResponseError>,
}

#[derive(Debug, Deserialize)]
struct ResponseError {
    #[serde(default)]
    message: String,
}

/// What one query brought back.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// Pipelines by MR iid (an MR left out reads them over REST).
    Read(HashMap<String, Vec<Pipeline>>),
    /// Over the instance's complexity / depth limits: split the batch.
    TooBig,
    /// The schema lacks something the query asks for: GraphQL is not usable here.
    Unsupported(String),
    /// Anything else: these MRs read over REST, nothing learnt.
    Failed(String),
}

/// Reads a GraphQL answer. Errors win over data: a partial answer is not trusted.
pub(crate) fn outcome(response: Response) -> Outcome {
    if let Some(first) = response.errors.first() {
        let messages: Vec<String> = response
            .errors
            .iter()
            .map(|e| e.message.to_lowercase())
            .collect();
        let any = |words: &[&str]| messages.iter().any(|m| words.iter().any(|w| m.contains(w)));
        if any(&["complexity", "depth", "too many", "max_", "maximum"]) {
            return Outcome::TooBig;
        }
        if any(&[
            "doesn't exist on type",
            "doesn't accept argument",
            "isn't a defined input type",
            "unknown argument",
            "undefined field",
        ]) {
            return Outcome::Unsupported(first.message.clone());
        }
        return Outcome::Failed(first.message.clone());
    }
    let Some(data) = response.data else {
        return Outcome::Failed("no data".into());
    };
    let project = match data.get("project") {
        Some(project) => project,
        None => data.pointer("/projects/nodes/0").unwrap_or(&Value::Null),
    };
    let Some(nodes) = project
        .pointer("/mergeRequests/nodes")
        .and_then(Value::as_array)
    else {
        return Outcome::Failed("project not found or not visible".into());
    };
    Outcome::Read(
        nodes
            .iter()
            .filter_map(|mr| {
                let iid = mr.get("iid")?.as_str()?.to_string();
                let mut pipelines: Vec<Pipeline> = mr
                    .pointer("/pipelines/nodes")?
                    .as_array()?
                    .iter()
                    .filter_map(pipeline_from)
                    .collect();
                // Newest first, as the REST list returns them.
                pipelines.sort_by_key(|p| std::cmp::Reverse(p.id));
                Some((iid, pipelines))
            })
            .collect(),
    )
}

/// The numeric id at the end of a global ID (`gid://gitlab/Ci::Pipeline/123`).
fn gid_number(gid: &str) -> Option<u64> {
    gid.rsplit('/').next()?.parse().ok()
}

fn text(node: &Value, key: &str) -> Option<String> {
    node.get(key)?.as_str().map(str::to_string)
}

/// A pipeline node in the shape the REST read gives (lowercase statuses, numeric
/// ids, newest job first).
fn pipeline_from(node: &Value) -> Option<Pipeline> {
    let status = text(node, "status").unwrap_or_default().to_lowercase();
    let mut jobs: Vec<PipelineJob> = node
        .pointer("/jobs/nodes")
        .and_then(Value::as_array)
        .map(|jobs| jobs.iter().filter_map(job_from).collect())
        .unwrap_or_default();
    jobs.sort_by_key(|j| std::cmp::Reverse(j.id));
    Some(Pipeline {
        id: gid_number(node.get("id")?.as_str()?)?,
        // Same mapping as REST: statuses the app does not model become `Unknown`.
        status: serde_json::from_value(Value::String(status)).unwrap_or(PipelineState::Unknown),
        created_at: text(node, "createdAt"),
        updated_at: text(node, "updatedAt"),
        sha: text(node, "sha"),
        jobs,
    })
}

fn job_from(node: &Value) -> Option<PipelineJob> {
    Some(PipelineJob {
        id: gid_number(node.get("id")?.as_str()?)?,
        name: text(node, "name").unwrap_or_default(),
        stage: node
            .pointer("/stage/name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        status: text(node, "status").unwrap_or_default().to_lowercase(),
        duration: node.get("duration").and_then(Value::as_f64),
        queued_duration: node.get("queuedDuration").and_then(Value::as_f64),
    })
}

/// One query for `iids`, under one permit of the instance's limit.
async fn query(ctx: &FetchContext, iids: &[&str], semaphore: &Semaphore) -> Outcome {
    let Ok(_permit) = semaphore.acquire().await else {
        return Outcome::Failed("cancelled".into());
    };
    let url = format!("{}/api/graphql", ctx.base_url.trim_end_matches('/'));
    let sent = http_client()
        .post(url)
        .bearer_auth(ctx.token.as_str())
        .json(&request_body(&ctx.project_id, iids))
        .send_paced()
        .await;
    let response = match sent {
        Ok(res) if res.status().is_success() => res,
        Ok(res) => return Outcome::Failed(format!("HTTP {}", res.status())),
        Err(e) => return Outcome::Failed(e.without_url().to_string()),
    };
    match response.json::<Response>().await {
        Ok(body) => outcome(body),
        Err(e) => Outcome::Failed(format!("undecodable answer: {e}")),
    }
}

/// Pipelines (with their jobs) of the MRs `iids`, a batch per query. MRs missing
/// from the result must read them over REST.
pub async fn mr_pipelines(
    ctx: &FetchContext,
    iids: &[String],
    semaphore: &Semaphore,
) -> HashMap<String, Vec<Pipeline>> {
    let mut read = HashMap::new();
    let Support::Batch(mut size) = support(&ctx.base_url) else {
        return read;
    };
    let mut queue: VecDeque<Vec<&str>> = iids
        .chunks(size)
        .map(|chunk| chunk.iter().map(String::as_str).collect())
        .collect();
    while let Some(batch) = queue.pop_front() {
        match query(ctx, &batch, semaphore).await {
            Outcome::Read(pipelines) => read.extend(pipelines),
            Outcome::TooBig if batch.len() > 1 => {
                let half = batch.len().div_ceil(2);
                if half < size {
                    size = half;
                    learn(&ctx.base_url, Support::Batch(size));
                    tracing::info!(batch = size, "GraphQL query too complex — smaller batches");
                }
                let (first, second) = batch.split_at(half);
                queue.push_front(second.to_vec());
                queue.push_front(first.to_vec());
            }
            Outcome::TooBig => {
                tracing::warn!(mr = batch[0], "GraphQL query too complex for one MR — REST");
            }
            Outcome::Unsupported(message) => {
                tracing::warn!(%message, "GraphQL pipelines not supported by this GitLab — REST from now on");
                learn(&ctx.base_url, Support::Off);
                break;
            }
            Outcome::Failed(message) => {
                tracing::warn!(%message, mrs = batch.len(), "GraphQL pipelines read failed — REST for these MRs");
            }
        }
    }
    read
}

/// Requests reading `mrs` MRs' pipelines takes: `Some(n)` over GraphQL, `None`
/// when the instance reads them over REST (counted per MR).
pub fn queries_for(base_url: &str, mrs: usize) -> Option<usize> {
    match support(base_url) {
        Support::Batch(size) => Some(mrs.div_ceil(size.max(1))),
        Support::Off => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(json: Value) -> Response {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_project_is_named_by_path_or_by_global_id() {
        assert_eq!(
            project_ref("42"),
            ProjectRef::Id("gid://gitlab/Project/42".into())
        );
        assert_eq!(
            project_ref("group/app"),
            ProjectRef::Path("group/app".into())
        );
        assert_eq!(
            project_ref("group%2Fsub%2Fapp"),
            ProjectRef::Path("group/sub/app".into())
        );
    }

    #[test]
    fn the_query_asks_for_the_same_window_as_rest() {
        let body = request_body("group/app", &["4", "17"]);
        let query = body["query"].as_str().unwrap();
        assert!(query.contains("project(fullPath: $project)"));
        assert!(query.contains("pipelines(first: 5)"));
        assert!(query.contains("jobs(first: 50, retried: false)"));
        assert_eq!(body["variables"]["project"], "group/app");
        assert_eq!(body["variables"]["iids"], serde_json::json!(["4", "17"]));
        assert_eq!(body["variables"]["first"], 2);

        let body = request_body("42", &["4"]);
        assert!(body["query"]
            .as_str()
            .unwrap()
            .contains("projects(ids: $ids)"));
        assert_eq!(
            body["variables"]["ids"],
            serde_json::json!(["gid://gitlab/Project/42"])
        );
    }

    #[test]
    fn pipelines_come_back_in_the_rest_shape() {
        let answer = response(
            serde_json::json!({ "data": { "project": { "mergeRequests": { "nodes": [
            { "iid": "4", "pipelines": { "nodes": [
                { "id": "gid://gitlab/Ci::Pipeline/10", "status": "SUCCESS",
                  "createdAt": "2026-11-10T08:00:00Z", "updatedAt": "2026-11-10T08:05:00Z",
                  "sha": "abc", "jobs": { "nodes": [
                      { "id": "gid://gitlab/Ci::Build/100", "name": "lint", "status": "SUCCESS",
                        "duration": 12, "queuedDuration": 1.5, "stage": { "name": "test" } },
                      { "id": "gid://gitlab/Ci::Build/101", "name": "deploy", "status": "MANUAL",
                        "duration": null, "queuedDuration": null, "stage": { "name": "deploy" } } ] } },
                { "id": "gid://gitlab/Ci::Pipeline/11", "status": "WAITING_FOR_RESOURCE",
                  "createdAt": null, "updatedAt": null, "sha": "def", "jobs": { "nodes": [] } } ] } },
            { "iid": "17", "pipelines": { "nodes": [] } } ] } } } }),
        );
        let Outcome::Read(read) = outcome(answer) else {
            panic!("expected pipelines");
        };
        assert!(read["17"].is_empty()); // answered: no REST read for it
        let pipelines = &read["4"];
        assert_eq!(pipelines.iter().map(|p| p.id).collect::<Vec<_>>(), [11, 10]); // newest first
        assert_eq!(pipelines[0].status, PipelineState::Unknown); // as REST maps it
        assert_eq!(pipelines[1].status, PipelineState::Success);
        assert_eq!(pipelines[1].sha.as_deref(), Some("abc"));
        let jobs = &pipelines[1].jobs;
        assert_eq!(jobs.iter().map(|j| j.id).collect::<Vec<_>>(), [101, 100]);
        assert_eq!(
            (jobs[1].name.as_str(), jobs[1].stage.as_str()),
            ("lint", "test")
        );
        assert_eq!(jobs[1].status, "success");
        assert_eq!(
            (jobs[1].duration, jobs[1].queued_duration),
            (Some(12.0), Some(1.5))
        );
        assert_eq!(
            (jobs[0].status.as_str(), jobs[0].duration),
            ("manual", None)
        );
    }

    #[test]
    fn a_numeric_project_answers_under_projects() {
        let answer = response(serde_json::json!({ "data": { "projects": { "nodes": [
            { "mergeRequests": { "nodes": [ { "iid": "4", "pipelines": { "nodes": [] } } ] } } ] } } }));
        assert!(matches!(outcome(answer), Outcome::Read(read) if read.contains_key("4")));
        let hidden = response(serde_json::json!({ "data": { "project": null } }));
        assert!(matches!(outcome(hidden), Outcome::Failed(_)));
    }

    #[test]
    fn errors_say_whether_to_split_give_up_or_fall_back() {
        let error = |message: &str| {
            response(serde_json::json!({
                "data": null, "errors": [{ "message": message }]
            }))
        };
        assert!(matches!(
            outcome(error(
                "Query has complexity of 312, which exceeds max complexity of 250"
            )),
            Outcome::TooBig
        ));
        assert!(matches!(
            outcome(error(
                "Query has depth of 16, which exceeds max depth of 15"
            )),
            Outcome::TooBig
        ));
        assert!(matches!(
            outcome(error(
                "Field 'queuedDuration' doesn't exist on type 'CiJob'"
            )),
            Outcome::Unsupported(_)
        ));
        assert!(matches!(
            outcome(error("Field 'jobs' doesn't accept argument 'retried'")),
            Outcome::Unsupported(_)
        ));
        assert!(matches!(
            outcome(error("Internal server error")),
            Outcome::Failed(_)
        ));
        // Partial data with an error is not trusted.
        let partial = response(serde_json::json!({
            "data": { "project": { "mergeRequests": { "nodes": [] } } },
            "errors": [{ "message": "timeout" }]
        }));
        assert!(matches!(outcome(partial), Outcome::Failed(_)));
    }

    #[test]
    fn what_an_instance_taught_is_kept_for_it_only() {
        let url = "https://learnt.test/group/app";
        assert_eq!(support(url), Support::Batch(INITIAL_BATCH));
        assert_eq!(queries_for(url, 25), Some(3));
        learn("https://learnt.test", Support::Batch(4));
        assert_eq!(support(url), Support::Batch(4));
        assert_eq!(queries_for(url, 9), Some(3));
        learn(url, Support::Off);
        assert_eq!(queries_for(url, 9), None);
        assert_eq!(
            support("https://other-learnt.test"),
            Support::Batch(INITIAL_BATCH)
        );
    }
}
