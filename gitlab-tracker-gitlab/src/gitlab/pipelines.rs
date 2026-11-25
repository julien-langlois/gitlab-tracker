//! An MR's pipelines and their jobs.

use super::*;

/// Fetches the last 5 pipelines for the given MR, then enriches each with
/// its job list (one extra request per pipeline, fired concurrently).
///
/// Returns an empty vec on any network or parse error — pipelines are
/// best-effort and must not block the MR data from being displayed.
pub(super) async fn fetch_pipelines(ctx: &FetchContext, mr_id: &str) -> Vec<Pipeline> {
    let client = http_client();

    // Fetch the last 5 pipeline runs for this MR.
    let pipelines_url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/pipelines?per_page=5",
        ctx.base_url, ctx.project_id, mr_id
    );
    let res = match client
        .get(&pipelines_url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return vec![],
    };

    let mut pipelines: Vec<Pipeline> = match res.json().await {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    // Enrich each pipeline with its jobs (fire requests concurrently).
    let jobs_futures: Vec<_> = pipelines
        .iter()
        .map(|p| {
            let jobs_url = format!(
                "{}/api/v4/projects/{}/pipelines/{}/jobs?per_page=50",
                ctx.base_url, ctx.project_id, p.id
            );
            let client = client.clone();
            let token = ctx.token.clone();
            async move {
                let res = client
                    .get(&jobs_url)
                    .header("PRIVATE-TOKEN", token.as_str())
                    .send_paced()
                    .await
                    .ok()?;
                if res.status().is_success() {
                    match res.json::<Vec<PipelineJob>>().await {
                        Ok(jobs) => Some(jobs),
                        Err(e) => {
                            tracing::warn!("Failed to deserialize jobs: {e}");
                            None
                        }
                    }
                } else {
                    tracing::warn!("Jobs endpoint returned non-2xx: {}", res.status());
                    None
                }
            }
        })
        .collect();

    let jobs_results = futures::future::join_all(jobs_futures).await;

    for (pipeline, jobs) in pipelines.iter_mut().zip(jobs_results) {
        pipeline.jobs = jobs.unwrap_or_default();
    }

    pipelines
}
