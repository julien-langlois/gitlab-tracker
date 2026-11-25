//! What the window's MR page reads from GitLab when it opens, on top of what the
//! refresh cycle keeps: why the MR cannot merge (with the files in conflict), its
//! approval rules, its threads in full, and the deployments of the tracked
//! branches (`gui` feature).
//!
//! Every read is best-effort: a failure leaves its part of the page out (or says
//! so), never the rest. Approval rules need GitLab Premium: on other tiers the
//! endpoint answers 403/404 and the page falls back to the approvers.

use crate::gitlab::http_client;
use gitlab_tracker_core::pacing::SendPaced;

mod history_reads;
mod mr_reads;
mod project_reads;
mod project_types;

use history_reads::*;
pub use mr_reads::*;
pub use project_reads::*;
pub use project_types::*;

/// The MR as GitLab sees it now: the merge checks the list does not keep.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Overview {
    /// `mergeable`, `conflict`, `ci_must_pass`, `discussions_not_resolved`,
    /// `not_approved`, `need_rebase`, `draft_status`, `requested_changes`…
    #[serde(default)]
    pub detailed_merge_status: String,
    #[serde(default)]
    pub has_conflicts: bool,
    #[serde(default)]
    pub blocking_discussions_resolved: bool,
    #[serde(default)]
    pub draft: bool,
    /// Commits the target branch has that the source lacks.
    pub diverged_commits_count: Option<u32>,
    #[serde(default)]
    pub force_remove_source_branch: Option<bool>,
}

/// An approval rule and where it stands.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Rule {
    pub name: String,
    #[serde(default)]
    pub approvals_required: u32,
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub approved_by: Vec<User>,
    #[serde(default)]
    pub eligible_approvers: Vec<User>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct User {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub username: String,
}

/// A thread with all its notes.
#[derive(Debug, Clone)]
pub struct Thread {
    /// `false` for a plain comment (GitLab's "Comment", not "Start a thread"):
    /// it cannot be resolved, so it is never open nor resolved.
    pub resolvable: bool,
    pub resolved: bool,
    /// The file and line it is about; `None` for a general thread.
    pub path: Option<String>,
    pub line: Option<u32>,
    pub notes: Vec<Note>,
}

impl Thread {
    /// A thread waiting to be resolved (a plain comment never is).
    pub fn is_open(&self) -> bool {
        self.resolvable && !self.resolved
    }
}

#[derive(Debug, Clone)]
pub struct Note {
    pub author: String,
    pub username: String,
    pub body: String,
    pub created_at: String,
}

/// The last successful deployment of an environment.
#[derive(Debug, Clone)]
pub struct Deployment {
    pub environment: String,
    /// The branch (or tag) deployed.
    pub reference: String,
    pub sha: String,
    pub created_at: String,
    /// Whether the MR's merge commit is in what was deployed (`None`: unknown,
    /// e.g. the MR reached the branch as a copy).
    pub includes: Option<bool>,
}

/// A file the MR and its target branch both changed since they parted.
///
/// GitLab's API does not list the files in conflict (its "Resolve conflicts"
/// screen reads a route that wants a browser session): they are worked out
/// from the two sides' patches instead, as git would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
    /// Both sides changed the same or adjacent lines, one side deleted the file,
    /// both added it, or a patch is missing (binary, too large): git stops on
    /// it. Otherwise the file most likely merges on its own.
    pub clash: bool,
}

/// Everything the page reads when it opens.
#[derive(Debug, Clone)]
pub struct PageData {
    pub overview: Option<Overview>,
    /// The files changed on both sides, clashes first; `None` when the MR is
    /// not in conflict or the patches could not be read.
    pub conflicts: Option<Vec<ConflictFile>>,
    /// `None`: no approval rules on this tier, or not readable.
    pub rules: Option<Vec<Rule>>,
    pub threads: Result<Vec<Thread>, String>,
    pub deployments: Vec<Deployment>,
    /// What the target branch has that the source lacks, on the target's first
    /// parents (its merges and direct pushes), newest first; `None` when the MR
    /// is not behind or the commits could not be read.
    pub behind: Option<Vec<BehindCommit>>,
}

/// A commit of the target branch the source branch does not have yet.
#[derive(Debug, Clone, Default)]
pub struct BehindCommit {
    pub sha: String,
    /// The commit's title; for a merge commit, the MR's title it names.
    pub title: String,
    pub author: String,
    pub created_at: String,
    pub web_url: String,
    /// The MR it merged (`See merge request group/project!12`), as its iid.
    pub mr_iid: Option<String>,
}

/// How many commits the "behind" list reads at most.
const BEHIND_READ: usize = 100;

/// Where to read: the instance, the token, the project and the MR.
#[derive(Clone)]
pub struct Target {
    pub base_url: String,
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
    pub mr_id: String,
    pub source_branch: String,
    pub target_branch: String,
    /// The MR's merge (or squash) commit, for the deployments.
    pub sha: Option<String>,
}

impl Target {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/api/v4/projects/{}/{}",
            self.base_url.trim_end_matches('/'),
            urlencoding::encode(&self.project_id),
            path
        )
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let res = http_client()
            .get(self.url(path))
            .header("PRIVATE-TOKEN", self.token.as_str())
            .send_paced()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if !res.status().is_success() {
            return Err(format!("GitLab answered {}", res.status()));
        }
        res.json().await.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
