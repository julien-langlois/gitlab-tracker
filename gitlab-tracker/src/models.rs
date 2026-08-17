use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A GitLab label as returned by the `/projects/:id/labels` endpoint.
///
/// Only `name` and `color` are needed — `color` is the hex background colour
/// (e.g. `"#6699cc"`) that GitLab uses when displaying the label badge.
#[derive(Deserialize, Debug, Clone)]
pub struct GitLabLabelDetail {
    pub name: String,
    /// Hex background colour as returned by GitLab (e.g. `"#6699cc"`).
    pub color: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GitLabUser {
    pub username: String,
    pub name: String,
}

/// A GitLab milestone as returned by the milestones API endpoint.
#[derive(Deserialize, Debug, Clone)]
pub struct GitLabMilestone {
    pub title: String,
    /// Due date in `YYYY-MM-DD` format, or `None` when not set.
    pub due_date: Option<String>,
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
    /// The MR is not open (already merged or closed in GitLab).
    NotOpen,
    /// The MR is a draft — intentionally not ready to merge.
    Draft,
    /// There are unresolved discussion threads on the MR.
    DiscussionsNotResolved,
    /// A CI pipeline is required before this MR can be merged.
    CiMustPass,
    /// A CI pipeline is currently running.
    CiStillRunning,
    /// Required approvals are missing.
    NotApproved,
    /// A reviewer has explicitly requested changes before the MR can be merged.
    RequestedChanges,
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
    /// List of reviewers assigned to this MR (GitLab returns an array).
    pub reviewers: Option<Vec<GitLabUser>>,
    /// User who merged the MR — populated by GitLab only when `state == merged`.
    pub merged_by: Option<GitLabUser>,
    /// ISO 8601 timestamp when the MR was merged — `None` for open/closed MRs.
    pub merged_at: Option<String>,
    pub milestone: Option<GitLabMilestone>,
    pub merge_commit_sha: Option<String>,
    pub squash_commit_sha: Option<String>,
    pub web_url: Option<String>,
    pub labels: Option<Vec<String>>,
    pub updated_at: Option<String>,
    /// Source branch of the MR (the feature branch).
    pub source_branch: Option<String>,
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
    /// Reviewer display strings — persisted across restarts.
    #[serde(default)]
    pub reviewers: Vec<String>,
    pub milestone: Option<String>,
    /// Milestone due date in `YYYY-MM-DD` format — persisted across restarts.
    #[serde(default)]
    pub milestone_due_date: Option<String>,
    pub web_url: Option<String>,
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Source branch of the MR (the feature branch) — persisted across restarts.
    #[serde(default)]
    pub source_branch: Option<String>,
    /// Target branch that this MR is intended to be merged into — persisted across restarts.
    #[serde(default)]
    pub target_branch: Option<String>,
    #[serde(default)]
    pub state: GitlabMrState,
    /// User who merged the MR — `None` for open/closed MRs. Persisted across restarts.
    #[serde(default)]
    pub merged_by: Option<String>,
    /// ISO 8601 timestamp when the MR was merged — `None` for open/closed MRs.
    #[serde(default)]
    pub merged_at: Option<String>,
    /// Persisted pipeline snapshots — restored on startup, refreshed on each MR fetch.
    #[serde(default)]
    pub pipelines: Vec<Pipeline>,
    /// Total number of user notes (comments + threads) — persisted across restarts.
    #[serde(default)]
    pub user_notes_count: u32,
    /// Whether the MR has been manually flagged by the user — persisted across restarts.
    #[serde(default)]
    pub flagged: bool,
    /// Last resolved tracker ticket — persisted to avoid re-fetching the tracker on every restart.
    /// Re-fetched only when the detected ticket ID changes (title/description update).
    /// `None` when no tracker provider is configured or no ticket reference was found.
    #[serde(default)]
    pub linked_ticket: Option<gitlab_tracker_core::LinkedTicket>,
    /// Diff statistics (files changed, additions, deletions) — persisted across restarts.
    /// Refreshed only when `updated_at` changes (same cache strategy as pipelines).
    #[serde(default)]
    pub diff_stats: Option<DiffStats>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SavedState {
    pub mrs: Vec<SavedMr>,
    /// Tracked branches — read-only for one-shot silent migration to `projects.toml`.
    /// New writes no longer include this field; it is kept here only so that
    /// existing `tracker_state.json` files are still deserialised correctly.
    #[serde(default, skip_serializing)]
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
    /// Reviewer display strings — may be empty when no reviewer is assigned.
    pub reviewers: Vec<String>,
    pub milestone: String,
    /// Milestone due date in `YYYY-MM-DD` format — `None` when not set.
    pub milestone_due_date: Option<String>,
    pub web_url: String,
    pub labels: Vec<String>,
    pub updated_at: Option<String>,
    /// Source branch of the MR (the feature branch).
    pub source_branch: String,
    pub target_branch: String,
    pub state: GitlabMrState,
    /// User who merged the MR — None for open/closed MRs.
    pub merged_by: Option<String>,
    /// ISO 8601 timestamp when the MR was merged — None for open/closed MRs.
    pub merged_at: Option<String>,
    /// Mergeability state for open MRs — drives the animated status badge.
    pub mergeability: MergeabilityStatus,
    /// Pipelines fetched alongside the MR data and persisted across restarts.
    pub pipelines: Vec<Pipeline>,
    /// Set to `true` when `updated_at` changed during the last refresh cycle.
    /// Drives the row highlight animation in the table. Reset after the fade window expires.
    pub recently_updated: bool,
    /// Total number of user notes (comments + discussion threads) on this MR.
    pub user_notes_count: u32,
    /// Manually flagged by the user (Space key) — persisted across restarts.
    /// Flagged MRs display a coloured chevron and can be isolated via the Flagged filter.
    pub flagged: bool,
    /// Ticket linked to this MR, resolved by the active [`TrackerProvider`].
    /// `None` when no tracker provider is configured or no ticket reference was found.
    pub linked_ticket: Option<gitlab_tracker_core::LinkedTicket>,
    /// Diff statistics (files changed, additions, deletions) for the review-difficulty badge.
    pub diff_stats: Option<DiffStats>,
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
    /// Reviewer display strings resolved from the GitLab API response.
    pub reviewers: Vec<String>,
    pub milestone: String,
    /// Milestone due date in `YYYY-MM-DD` format — `None` when not set.
    pub milestone_due_date: Option<String>,
    pub web_url: String,
    pub labels: Vec<String>,
    pub updated_at: Option<String>,
    /// Source branch of the MR (the feature branch).
    pub source_branch: String,
    pub target_branch: String,
    pub state: GitlabMrState,
    /// User who merged the MR — None for open/closed MRs.
    pub merged_by: Option<String>,
    /// ISO 8601 timestamp when the MR was merged — None for open/closed MRs.
    pub merged_at: Option<String>,
    /// Mergeability resolved from the GitLab API response.
    pub mergeability: MergeabilityStatus,
    /// Pipelines fetched in the same request batch as the MR data.
    pub pipelines: Vec<Pipeline>,
    /// Total number of user notes (comments + discussion threads) on this MR.
    pub user_notes_count: u32,
    /// Diff statistics fetched from the GitLab Changes API.
    /// `None` until the first successful fetch or when the API returns no data.
    pub diff_stats: Option<DiffStats>,
}

/// Diff statistics for a merge request: files changed, lines added, lines deleted.
///
/// Fetched from the GitLab Changes API and used to display a compact summary
/// (e.g. "9 files  +545 -32") alongside a review-difficulty score.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DiffStats {
    /// Number of files touched by this MR.
    pub files_changed: u32,
    /// Total lines added across all changed files.
    pub additions: u32,
    /// Total lines deleted across all changed files.
    pub deletions: u32,
}

impl DiffStats {
    /// Computes a review-difficulty score in [0.0, 1.0] based on the tech-stack profile.
    ///
    /// The formula weights total changed lines against the profile's "easy" and "hard"
    /// thresholds.  Below the easy threshold → 0.0 (trivial).  Above the hard threshold
    /// → 1.0 (very complex).  Between the two the score is interpolated linearly, then
    /// capped to [0.0, 1.0].
    pub fn difficulty(&self, profile: &DifficultyProfile) -> f64 {
        let total_lines = (self.additions + self.deletions) as f64;
        // Files weigh 20 % of the total so a patch that only reorganises many tiny
        // files (e.g. Drupal config YAML) is penalised less than one that rewrites
        // large, logic-heavy files.
        let weighted = total_lines * 0.8 + self.files_changed as f64 * 0.2;
        let range = (profile.hard_threshold - profile.easy_threshold) as f64;
        if range <= 0.0 {
            return 1.0;
        }
        let score = (weighted - profile.easy_threshold as f64) / range;
        score.clamp(0.0, 1.0)
    }
}

/// Tech-stack calibration for the review-difficulty score.
///
/// Different ecosystems have very different "cost per line" — a 500-line Drupal
/// YAML config dump is far cheaper to review than 500 lines of Java business logic.
/// These thresholds let users tune the scoring to their project's reality via
/// `config.json` (`complexity_profile`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DifficultyProfile {
    /// Human-readable name shown in the UI (e.g. "Drupal", "Java", "Generic").
    pub name: String,
    /// Weighted score below which a MR is considered easy (maps to 0.0).
    pub easy_threshold: u32,
    /// Weighted score above which a MR is considered very complex (maps to 1.0).
    pub hard_threshold: u32,
}

impl Default for DifficultyProfile {
    fn default() -> Self {
        // Conservative generic defaults: easy < 200 weighted units, hard > 1 000.
        Self {
            name: "Generic".to_string(),
            easy_threshold: 200,
            hard_threshold: 1000,
        }
    }
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
    /// ISO 8601 timestamp when the pipeline was created (i.e. triggered).
    /// Populated directly from the GitLab API — `None` when not provided.
    pub created_at: Option<String>,
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
    /// Fired when the project label list (with colours) has been fetched from GitLab.
    GitlabLabelsLoaded(Vec<GitLabLabelDetail>),
    /// Fired when the milestone list has been fetched from GitLab.
    MilestonesLoaded(Vec<GitLabMilestone>),
    /// Fired when the MR IDs linked to a milestone have been resolved.
    MilestoneMrsLoaded {
        milestone_title: String,
        mr_ids: Vec<String>,
    },
    /// Fired when a tracker plugin has resolved the ticket linked to a MR.
    /// Emitted by any active tracker provider (Redmine, Jira, Trello, …).
    /// `ticket` is boxed to keep the enum variant size in check (Clippy `large_enum_variant`).
    TrackerTicketLoaded {
        mr_id: String,
        ticket: Box<gitlab_tracker_core::LinkedTicket>,
    },
    /// Fired when the list of time-tracking activity categories has been fetched.
    /// Stored in `App` for use in the Log Time popup selector.
    ActivitiesLoaded(Vec<gitlab_tracker_core::Activity>),
    /// Fired when time entries for a ticket have been fetched from the tracker.
    TimeEntriesLoaded {
        entries: Vec<gitlab_tracker_core::TimeEntry>,
    },
    /// Fired when a time entry has been successfully submitted to the tracker.
    /// Carries both the MR id (to update the right `linked_ticket` in memory) and the
    /// raw ticket id (to re-fetch time entries and the updated spent total).
    TimeLogSubmitted {
        /// The GitLab MR id that owns the ticket (used to route `TrackerTicketLoaded`).
        mr_id: String,
        /// The tracker ticket id that received the new time entry.
        ticket_id: String,
    },
    /// Fired when a time entry submission failed.
    /// The error string is shown inline in the popup.
    TimeLogFailed {
        error: String,
    },
    Tick,
}
