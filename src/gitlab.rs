use crate::models::{AppEvent, GitLabMr, GitLabRef, MrLoadedData};
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
            if !t.contains("⚠️ ERROR") && !t.starts_with("[Open]") && !w.is_empty() =>
        {
            (t, Some(s), d, a, asg, m, w, lbls)
        }
        _ => {
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

            let sha = mr.merge_commit_sha.or(mr.squash_commit_sha);
            let title = if sha.is_none() {
                format!("[Open] {}", mr.title)
            } else {
                mr.title
            };
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
    })
}
