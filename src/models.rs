use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Deserialize, Debug, Clone)]
pub struct GitLabUser {
    pub username: String,
}

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
    #[serde(default)]
    pub state: GitlabMrState,
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
    pub state: GitlabMrState,
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
    pub state: GitlabMrState,
}

pub enum AppEvent {
    MrLoaded(Box<MrLoadedData>),
    MrFailed { id: String, error: String },
    Tick,
}
