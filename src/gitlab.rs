use crate::models::{
    AppEvent, GitLabMr, GitLabRef, MergeabilityStatus, MrLoadedData, Pipeline, PipelineJob,
};
use crate::utils::{calculate_relevance, RELEVANCE_THRESHOLD};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub const MAX_CONCURRENT_REQUESTS: usize = 3;

#[derive(Clone)]
pub struct FetchContext {
    pub base_url: String,
    pub token: String,
    pub project_id: String,
    pub branches: Vec<String>,
}

#[derive(Clone, Default)]
pub struct CachedMrData {
    pub title: Option<String>,
    pub sha: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub assignee: Option<String>,
    pub milestone: Option<String>,
    pub web_url: Option<String>,
    pub labels: Option<Vec<String>>,
    /// Last known `updated_at` timestamp — used to skip pipeline re-fetch when
    /// the MR has not changed since the previous refresh cycle.
    pub updated_at: Option<String>,
    /// Pipelines from the previous fetch — reused when `updated_at` is unchanged.
    pub pipelines: Vec<Pipeline>,
}

/// Fetches the last 5 pipelines for the given MR, then enriches each with
/// its job list (one extra request per pipeline, fired concurrently).
///
/// Returns an empty vec on any network or parse error — pipelines are
/// best-effort and must not block the MR data from being displayed.
async fn fetch_pipelines(ctx: &FetchContext, mr_id: &str) -> Vec<Pipeline> {
    let client = reqwest::Client::new();

    // Fetch the last 5 pipeline runs for this MR.
    let pipelines_url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/pipelines?per_page=5",
        ctx.base_url, ctx.project_id, mr_id
    );
    let res = match client
        .get(&pipelines_url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
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
                    .header("PRIVATE-TOKEN", &token)
                    .send()
                    .await
                    .ok()?;
                if res.status().is_success() {
                    res.json::<Vec<PipelineJob>>().await.ok()
                } else {
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

pub fn spawn_mr_fetch(
    ctx: FetchContext,
    mr_id: String,
    cached: CachedMrData,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let _permit = semaphore.acquire().await.unwrap();
        match fetch_gitlab_data(&ctx, &mr_id, cached).await {
            Ok(data) => {
                let _ = tx.send(AppEvent::MrLoaded(Box::new(data)));
            }
            Err(err_msg) => {
                let _ = tx.send(AppEvent::MrFailed {
                    id: mr_id,
                    error: err_msg,
                });
            }
        }
    });
}

pub async fn fetch_gitlab_data(
    ctx: &FetchContext,
    mr_id: &str,
    cached: CachedMrData,
) -> Result<MrLoadedData, String> {
    let client = reqwest::Client::new();

    // updated_at is always fetched fresh — never served from cache — so we always
    // know the real last-update timestamp regardless of the cache hit path.
    let mr_url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}",
        ctx.base_url, ctx.project_id, mr_id
    );
    let mr_res = client
        .get(&mr_url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
        .map_err(|e| format!("MR network error: {}", e))?;

    if !mr_res.status().is_success() {
        return Err(format!("HTTP {} on MR", mr_res.status()));
    }

    let mr: GitLabMr = mr_res
        .json()
        .await
        .map_err(|e| format!("Error reading MR JSON: {}", e))?;

    let updated_at = mr.updated_at.clone();
    // Always read the state fresh from the API response — never served from cache.
    let state = mr.state.clone().unwrap_or_default();

    // Resolve mergeability with the following priority:
    //   1. `has_conflicts: true` always wins — manual intervention is required regardless
    //      of what `detailed_merge_status` says. GitLab can return "need_rebase" AND
    //      has_conflicts: true simultaneously when the rebase would produce conflicts.
    //   2. `detailed_merge_status` (GitLab ≥ 15.6) for the remaining cases:
    //        • "need_rebase"    → branch is behind target, rebase is conflict-free
    //        • "merge_conflict" → conflicts detected, manual resolution required
    //        • "mergeable"      → ready to merge cleanly
    //   3. `merge_status` (legacy, GitLab < 15.6) — coarse-grained last resort.
    let mergeability = if mr.has_conflicts == Some(true) {
        MergeabilityStatus::Conflict
    } else {
        match mr.detailed_merge_status.as_deref() {
            Some("mergeable") => MergeabilityStatus::Mergeable,
            Some("need_rebase") => MergeabilityStatus::NeedsRebase,
            Some("merge_conflict") | Some("conflict") => MergeabilityStatus::Conflict,
            // "checking", "not_open", "jira_association_missing", "broken_status", etc.
            _ => match mr.merge_status.as_deref() {
                Some("can_be_merged") => MergeabilityStatus::Mergeable,
                Some("cannot_be_merged") => MergeabilityStatus::Conflict,
                _ => MergeabilityStatus::Unknown,
            },
        }
    };

    let (title, sha, description, author, assignee, milestone, web_url, labels) = match (
        cached.title,
        cached.sha,
        cached.description,
        cached.author,
        cached.assignee,
        cached.milestone,
        cached.web_url,
        cached.labels,
    ) {
        (Some(t), Some(s), Some(d), Some(a), Some(asg), Some(m), Some(w), Some(lbls))
            if !t.contains("⚠️ ERROR") && !w.is_empty() =>
        {
            (t, Some(s), d, a, asg, m, w, lbls)
        }
        _ => {
            let sha = mr.merge_commit_sha.or(mr.squash_commit_sha);
            let title = mr.title;
            let desc = mr.description.unwrap_or_default();
            let auth = mr
                .author
                .map(|u| u.username)
                .unwrap_or_else(|| "unknown".to_string());
            let asg = mr
                .assignee
                .map(|u| u.username)
                .unwrap_or_else(|| "none".to_string());
            let mile = mr
                .milestone
                .map(|m| m.title)
                .unwrap_or_else(|| "None".to_string());
            let web_url = mr.web_url.unwrap_or_default();
            let labels = mr.labels.unwrap_or_default();

            (title, sha, desc, auth, asg, mile, web_url, labels)
        }
    };

    let mut found_branches = HashSet::new();

    if let Some(ref commit_sha) = sha {
        let refs_url_standard = format!(
            "{}/api/v4/projects/{}/repository/commits/{}/refs?type=branch",
            ctx.base_url, ctx.project_id, commit_sha
        );
        if let Ok(refs_res) = client
            .get(&refs_url_standard)
            .header("PRIVATE-TOKEN", &ctx.token)
            .send()
            .await
        {
            if refs_res.status().is_success() {
                if let Ok(refs) = refs_res.json::<Vec<GitLabRef>>().await {
                    for r in refs {
                        let cleaned = r.name.replace("refs/heads/", "");
                        if ctx.branches.contains(&cleaned) {
                            found_branches.insert(cleaned);
                        }
                    }
                }
            }
        }
    }

    let gitlab_search_query = title
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| {
            w.len() > 3
                && !["feat", "fix", "refactor", "chore", "open"]
                    .contains(&w.to_lowercase().as_str())
        })
        .take(3)
        .collect::<Vec<&str>>()
        .join(" ");

    for branch in &ctx.branches {
        if found_branches.contains(branch) {
            continue;
        }

        let commits_url = format!(
            "{}/api/v4/projects/{}/repository/commits?ref_name={}&search={}&per_page=15",
            ctx.base_url,
            ctx.project_id,
            branch,
            urlencoding::encode(&gitlab_search_query)
        );

        if let Ok(commits_res) = client
            .get(&commits_url)
            .header("PRIVATE-TOKEN", &ctx.token)
            .send()
            .await
        {
            if commits_res.status().is_success() {
                if let Ok(commits) = commits_res.json::<Vec<serde_json::Value>>().await {
                    for commit in commits {
                        if let Some(commit_message) = commit.get("message").and_then(|v| v.as_str())
                        {
                            let score = calculate_relevance(&title, commit_message);
                            if score >= RELEVANCE_THRESHOLD {
                                found_branches.insert(branch.clone());
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    // Only re-fetch pipelines if the MR has been updated since the last cycle.
    // If `updated_at` is unchanged, reuse the cached pipeline data to avoid
    // hammering the GitLab API with redundant requests (rate-limit friendly).
    let pipelines = if updated_at.is_some()
        && updated_at == cached.updated_at
        && !cached.pipelines.is_empty()
    {
        cached.pipelines
    } else {
        fetch_pipelines(ctx, mr_id).await
    };

    Ok(MrLoadedData {
        id: mr_id.to_string(),
        title,
        sha,
        branches: found_branches,
        description,
        author,
        assignee,
        milestone,
        web_url,
        labels,
        updated_at,
        state,
        mergeability,
        pipelines,
    })
}
