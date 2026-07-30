use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Deserialize, Debug, Clone)]
pub struct GitLabUser {
    pub username: String,
}

/// A GitLab milestone as returned by the milestones API endpoint.
#[derive(Deserialize, Debug, Clone)]
pub struct GitLabMilestone {
    pub title: String,
}

/// Represents the GitLab-side lifecycle state of a merge request.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GitlabMrState {
    #[default]
    Opened,
    Merged,
    Closed,
}

/// Represents the mergeability status of an open merge request as reported by GitLab.
///
/// Only meaningful for MRs in the `Opened` state — ignored for Merged/Closed.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub enum MergeabilityStatus {
    /// GitLab reports the MR can be merged cleanly.
    Mergeable,
    /// The MR has conflicts that must be resolved before merging.
    Conflict,
    /// The MR branch is behind the target branch and needs a rebase.
    NeedsRebase,
    /// Status not yet fetched, not applicable, or an unrecognised value.
    #[default]
    Unknown,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GitLabMr {
    pub title: String,
    pub state: Option<GitlabMrState>,
    pub description: Option<String>,
    pub author: Option<GitLabUser>,
    pub assignee: Option<GitLabUser>,
    pub milestone: Option<GitLabMilestone>,
    pub merge_commit_sha: Option<String>,
    pub squash_commit_sha: Option<String>,
    pub web_url: Option<String>,
    pub labels: Option<Vec<String>>,
    pub updated_at: Option<String>,
    /// Target branch that this MR is intended to be merged into.
    pub target_branch: Option<String>,
    /// Legacy mergeability field (GitLab < 15.6): `"can_be_merged"`, `"cannot_be_merged"`, …
    pub merge_status: Option<String>,
    /// Detailed mergeability field (GitLab ≥ 15.6): `"mergeable"`, `"need_rebase"`,
    /// `"conflict"`, `"checking"`, `"not_open"`, etc. Takes priority over `merge_status`.
    pub detailed_merge_status: Option<String>,
    /// Whether the MR has unresolved merge conflicts (complementary signal from GitLab).
    pub has_conflicts: Option<bool>,
    /// Total number of user notes (comments + discussion threads) on this MR.
    /// Returned natively by the GitLab API — no extra request needed.
    pub user_notes_count: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GitLabRef {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedMr {
    pub id: String,
    pub title: String,
    pub sha: Option<String>,
    pub found_branches: HashSet<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub assignee: Option<String>,
    pub milestone: Option<String>,
    pub web_url: Option<String>,
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Target branch that this MR is intended to be merged into — persisted across restarts.
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub state: GitlabMrState,
    /// Persisted pipeline snapshots — restored on startup, refreshed on each MR fetch.
    #[serde(default)]
    pub pipelines: Vec<Pipeline>,
    /// Total number of user notes (comments + threads) — persisted across restarts.
    #[serde(default)]
    pub user_notes_count: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SavedState {
    pub mrs: Vec<SavedMr>,
    pub branches: Vec<String>,
    #[serde(default)]
    pub last_known_branches: HashMap<String, HashSet<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MrStatus {
    Loading,
    Error,
    MergedIn(HashSet<String>),
}

#[derive(Clone, Debug)]
pub struct TrackedMr {
    pub id: String,
    pub title: String,
    pub status: MrStatus,
    pub sha: Option<String>,
    pub description: String,
    pub author: String,
    pub assignee: String,
    pub milestone: String,
    pub web_url: String,
    pub labels: Vec<String>,
    pub updated_at: Option<String>,
    pub target_branch: String,
    pub state: GitlabMrState,
    /// Mergeability state for open MRs — drives the animated status badge.
    pub mergeability: MergeabilityStatus,
    /// Pipelines fetched alongside the MR data and persisted across restarts.
    pub pipelines: Vec<Pipeline>,
    /// Set to `true` when `updated_at` changed during the last refresh cycle.
    /// Drives the row highlight animation in the table. Reset after the fade window expires.
    pub recently_updated: bool,
    /// Total number of user notes (comments + discussion threads) on this MR.
    pub user_notes_count: u32,
}

#[derive(Debug, Clone)]
pub struct MrLoadedData {
    pub id: String,
    pub title: String,
    pub sha: Option<String>,
    pub branches: HashSet<String>,
    pub description: String,
    pub author: String,
    pub assignee: String,
    pub milestone: String,
    pub web_url: String,
    pub labels: Vec<String>,
    pub updated_at: Option<String>,
    pub target_branch: String,
    pub state: GitlabMrState,
    /// Mergeability resolved from the GitLab API response.
    pub mergeability: MergeabilityStatus,
    /// Pipelines fetched in the same request batch as the MR data.
    pub pipelines: Vec<Pipeline>,
    /// Total number of user notes (comments + discussion threads) on this MR.
    pub user_notes_count: u32,
}

/// Lifecycle state of a GitLab pipeline run.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PipelineState {
    Created,
    Pending,
    Running,
    Success,
    Failed,
    Canceled,
    Skipped,
    #[serde(other)]
    #[default]
    Unknown,
}

/// A single job within a pipeline, as returned by the GitLab API.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PipelineJob {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub status: String,
    /// Duration in seconds, null while running.
    pub duration: Option<f64>,
}

/// A GitLab pipeline attached to a merge request.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Pipeline {
    pub id: u64,
    #[serde(default)]
    pub status: PipelineState,
    /// Jobs are fetched alongside the pipeline and persisted across restarts.
    #[serde(default)]
    pub jobs: Vec<PipelineJob>,
}

pub enum AppEvent {
    MrLoaded(Box<MrLoadedData>),
    MrFailed {
        id: String,
        error: String,
    },
    /// Fired when the milestone list has been fetched from GitLab.
    MilestonesLoaded(Vec<GitLabMilestone>),
    /// Fired when the MR IDs linked to a milestone have been resolved.
    MilestoneMrsLoaded {
        milestone_title: String,
        mr_ids: Vec<String>,
    },
    Tick,
}
