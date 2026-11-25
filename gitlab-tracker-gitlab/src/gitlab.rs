use crate::events::GitlabEvent;
use crate::models::{
    BranchCommits, CommitFingerprint, DiffStats, GitLabLabelDetail, GitLabMilestone, GitLabMr,
    GitLabRef, GitlabMrState, MergeabilityStatus, MrCommit, MrLoadedData, MrThread, Pipeline,
    PipelineJob, ReviewActivity,
};
use gitlab_tracker_core::pacing::SendPaced;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};

mod branch_scan;
mod context;
mod diffs;
mod estimate;
mod fetch;
mod lists;
mod mr;
mod on_demand;
mod pages;
mod pipelines;
mod review;

pub use branch_scan::*;
pub use context::*;
pub use diffs::*;
pub use estimate::*;
pub use fetch::*;
pub use lists::*;
pub use mr::*;
pub use on_demand::*;
pub use pages::*;
use pipelines::*;
use review::*;

/// Why fetching a MR from GitLab failed. Its message is what the Inspector shows
/// (`MrStatus::Error`), so each variant embeds the underlying detail.
#[derive(Debug, thiserror::Error)]
pub enum GitlabError {
    /// The request could not be sent or the connection failed.
    #[error("network error: {0}")]
    Network(reqwest::Error),
    /// GitLab answered with a non-2xx status (e.g. `401 Unauthorized`, `404 Not Found`).
    #[error("HTTP {0}")]
    Http(reqwest::StatusCode),
    /// The response body is not the expected JSON.
    #[error("invalid MR JSON: {0}")]
    Decode(reqwest::Error),
    /// The concurrency limiter was closed (app shutting down).
    #[error("fetch cancelled: concurrency limiter was closed")]
    Cancelled,
}

pub const MAX_CONCURRENT_REQUESTS: usize = 3;
pub const MERGEABILITY_RETRY_ATTEMPTS: usize = 3;
const MERGEABILITY_RETRY_DELAY_SECS: u64 = 2;

/// Process-wide HTTP client: one connection pool (keep-alive + TLS session reuse)
/// shared by every GitLab call, with timeouts so a hung server cannot hold a
/// semaphore permit forever.
pub fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(gitlab_tracker_core::http::same_origin_redirects())
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
}

/// What GitLab says about a project, read when the window adds it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectInfo {
    pub id: u64,
    pub name: String,
    pub path_with_namespace: String,
    pub default_branch: Option<String>,
}

/// Why a project could not be checked.
#[derive(Debug, thiserror::Error)]
pub enum ProjectCheckError {
    #[error("GitLab refused the token (it may be expired, or lack the read_api scope)")]
    Token,
    #[error("No such project on this instance, or the token cannot see it")]
    NotFound,
    #[error("GitLab answered {0}")]
    Status(reqwest::StatusCode),
    #[error("Could not reach GitLab: {0}")]
    Network(String),
}

/// Reads `project` (numeric id or `group/project` path) with `token`: checks both
/// the token and the project before the window saves them.
pub async fn check_project(
    base_url: &str,
    token: &str,
    project: &str,
) -> Result<ProjectInfo, ProjectCheckError> {
    let url = format!(
        "{}/api/v4/projects/{}",
        base_url.trim_end_matches('/'),
        urlencoding::encode(project)
    );
    let res = http_client()
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .send_paced()
        .await
        .map_err(|e| ProjectCheckError::Network(e.without_url().to_string()))?;
    match res.status() {
        s if s.is_success() => res
            .json()
            .await
            .map_err(|e| ProjectCheckError::Network(e.to_string())),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            Err(ProjectCheckError::Token)
        }
        reqwest::StatusCode::NOT_FOUND => Err(ProjectCheckError::NotFound),
        s => Err(ProjectCheckError::Status(s)),
    }
}

/// The username of the token's owner (`GET /user`), `None` on any failure: it
/// only feeds the "me" views.
pub async fn current_username(base_url: &str, token: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct User {
        username: String,
    }
    let url = format!("{}/api/v4/user", base_url.trim_end_matches('/'));
    let res = http_client()
        .get(&url)
        .header("PRIVATE-TOKEN", token)
        .send_paced()
        .await
        .ok()
        .filter(|r| r.status().is_success())?;
    res.json::<User>().await.ok().map(|u| u.username)
}

/// Whether a person's display string or login looks like a bot account
/// (Renovate, Dependabot, a project or group access token, a `*-bot`): its
/// notes are not a review.
pub fn is_bot(display: &str) -> bool {
    let lower = display.to_lowercase();
    ["bot", "renovate", "dependabot", "gitlab-ci", "project_"]
        .iter()
        .any(|word| lower.contains(word))
}

#[cfg(test)]
pub mod tests;
