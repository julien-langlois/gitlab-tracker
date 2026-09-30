use serde::{Deserialize, Serialize};

// Domain enums live in `core` (no UI dependency); re-exported so `crate::models::…`
// paths keep working.
pub use gitlab_tracker_core::{GitlabMrState, MergeabilityStatus, PipelineState};
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
    /// Free-text description of the milestone, or `None` when not set.
    pub description: Option<String>,
}

/// A single commit as returned by
/// `GET /projects/:id/merge_requests/:merge_request_iid/commits`.
///
/// Reference: <https://docs.gitlab.com/api/merge_requests/#retrieve-merge-request-commits>
///
/// GitLab returns the full commit object; only the fields useful for display
/// are mapped here — unknown fields are silently ignored by serde.
/// Fields are not yet consumed beyond deserialization — the struct is kept as a
/// forward-compatible contract for future UI use (commit list in the side panel, etc.).
#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct GitLabCommit {
    /// Full SHA-1 hash of the commit (40 hex chars).
    pub id: String,
    /// Abbreviated SHA-1 (typically 8 chars) — used for compact display.
    pub short_id: String,
    /// First line of the commit message.
    pub title: String,
    /// Full commit message, including body and trailers. May be `None` when
    /// the API omits it (e.g. lightweight tag commits).
    pub message: Option<String>,
    /// Display name of the commit author.
    pub author_name: String,
    /// Email address of the commit author.
    pub author_email: String,
    /// ISO 8601 timestamp when the commit was authored (author date).
    pub authored_date: String,
    /// Display name of the committer (may differ from author on rebases/merges).
    pub committer_name: String,
    /// Email address of the committer.
    pub committer_email: String,
    /// ISO 8601 timestamp when the commit was committed (committer date).
    pub committed_date: String,
    /// SHA-1 hashes of parent commits. Empty for root commits, two entries
    /// for merge commits.
    #[serde(default)]
    pub parent_ids: Vec<String>,
    /// Web URL to the commit detail page on GitLab.
    pub web_url: String,
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
    /// SHA of the latest commit on the source branch (the MR HEAD).
    /// Always populated by GitLab regardless of merge state — used as a fallback
    /// to detect branch presence when merge_commit_sha / squash_commit_sha are absent
    /// (e.g. MR still open, or merged via fast-forward without a merge commit).
    pub sha: Option<String>,
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
    /// ISO 8601 timestamp when the MR was created — always populated by GitLab.
    pub created_at: Option<String>,
    /// Whether the MR has unresolved merge conflicts (complementary signal from GitLab).
    pub has_conflicts: Option<bool>,
    /// Only populated when the MR request includes `include_diverged_commits_count=true`
    /// (GitLab API parameter). `None` when not requested, not yet computed by GitLab
    /// or when the MR is already merged/closed.
    pub diverged_commits_count: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GitLabRef {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedMr {
    pub id: String,
    pub found_branches: HashSet<String>,
    /// Whether the MR has been manually flagged by the user — persisted across restarts.
    #[serde(default)]
    pub flagged: bool,
    /// Last resolved tracker ticket — persisted to avoid re-fetching the tracker on every restart.
    /// Re-fetched only when the detected ticket ID changes (title/description update).
    /// `None` when no tracker provider is configured or no ticket reference was found.
    #[serde(default)]
    pub linked_ticket: Option<gitlab_tracker_core::LinkedTicket>,
    /// GitLab data, flattened so the JSON layout matches older versions.
    #[serde(flatten)]
    pub data: MrData,
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
    /// RFC 3339 timestamp recorded the first time discovery runs for this project.
    /// All subsequent discovery polls use this value as `created_after` so MRs
    /// that existed before the tool was started are never auto-added.
    /// Absent in older state files — treated as `None` and initialised to `Utc::now()`
    /// on the first poll.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_started_at: Option<String>,
    /// MR IIDs manually removed from the dashboard.
    /// Discovery ignores these ids so auto-polling does not re-add items the user
    /// explicitly cleaned from the dashboard. Manual re-add removes the id from
    /// this deny-list.
    #[serde(default)]
    pub dismissed_mr_ids: HashSet<String>,
}

/// The GitLab-sourced data of a merge request, shared by the in-memory model
/// ([`TrackedMr`]), the persisted state ([`SavedMr`]) and a fetch result
/// ([`MrLoadedData`]) — declared once instead of being copied field by field.
///
/// Serialised flattened inside [`SavedMr`], so the state file keeps its layout.
/// Every field has a default; the ones that older versions stored as `Option`
/// also accept `null` (see [`null_as_default`]).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MrData {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub author: String,
    #[serde(default)]
    pub assignee: Option<String>,
    /// Reviewer display strings — may be empty when no reviewer is assigned.
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub milestone: Option<String>,
    /// Milestone due date in `YYYY-MM-DD` format — `None` when not set.
    #[serde(default)]
    pub milestone_due_date: Option<String>,
    /// Milestone description — `None` when not set or when no milestone is attached.
    /// Persisted across restarts; invalidated when the milestone title changes.
    #[serde(default)]
    pub milestone_description: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub web_url: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: Vec<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Source branch of the MR (the feature branch).
    #[serde(default, deserialize_with = "null_as_default")]
    pub source_branch: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub target_branch: String,
    #[serde(default)]
    pub state: GitlabMrState,
    /// User who merged the MR — None for open/closed MRs.
    #[serde(default)]
    pub merged_by: Option<String>,
    /// ISO 8601 timestamp when the MR was merged — None for open/closed MRs.
    #[serde(default)]
    pub merged_at: Option<String>,
    /// ISO 8601 timestamp when the MR was created — immutable once set, persisted across restarts.
    #[serde(default)]
    pub created_at: Option<String>,
    /// Pipelines fetched alongside the MR data and persisted across restarts.
    #[serde(default)]
    pub pipelines: Vec<Pipeline>,
    /// Total number of user notes (comments + discussion threads) on this MR.
    #[serde(default)]
    pub user_notes_count: u32,
    /// Diff statistics (files changed, additions, deletions) for the review-difficulty badge.
    #[serde(default)]
    pub diff_stats: Option<DiffStats>,
}

/// Deserialises `null` as `T::default()`: older state files wrote `null` for fields
/// that are no longer optional.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, PartialEq)]
pub enum MrStatus {
    Loading,
    /// The last fetch failed; carries the error message (shown in the inspector).
    /// Never persisted, and never written into the MR title.
    Error(String),
    MergedIn(HashSet<String>),
}
#[derive(Clone, Debug)]
pub struct TrackedMr {
    pub id: String,
    pub status: MrStatus,
    /// Mergeability state for open MRs — drives the animated status badge.
    pub mergeability: MergeabilityStatus,
    /// Set to `true` when `updated_at` changed during the last refresh cycle.
    /// Drives the row highlight animation in the table. Reset after the fade window expires.
    pub recently_updated: bool,
    /// Manually flagged by the user (Space key) — persisted across restarts.
    /// Flagged MRs display a coloured chevron and can be isolated via the Flagged filter.
    pub flagged: bool,
    /// Ticket linked to this MR, resolved by the active `TrackerProvider`.
    /// `None` when no tracker provider is configured or no ticket reference was found.
    pub linked_ticket: Option<gitlab_tracker_core::LinkedTicket>,
    /// GitLab data of the MR (title, branches, pipelines…), also reachable directly
    /// through `Deref` (`mr.title`).
    pub data: MrData,
}

// `TrackedMr` *is* an MR's data plus runtime state: `Deref` keeps `mr.title` working
// everywhere instead of spelling `mr.data.title`.
impl std::ops::Deref for TrackedMr {
    type Target = MrData;
    fn deref(&self) -> &MrData {
        &self.data
    }
}

impl std::ops::DerefMut for TrackedMr {
    fn deref_mut(&mut self) -> &mut MrData {
        &mut self.data
    }
}

impl TrackedMr {
    /// A not-yet-fetched MR shown while its first GitLab fetch is in flight.
    pub fn placeholder(id: String, title: String, milestone: Option<String>) -> Self {
        Self {
            id,
            status: MrStatus::Loading,
            mergeability: MergeabilityStatus::Unknown,
            recently_updated: false,
            flagged: false,
            linked_ticket: None,
            data: MrData {
                title,
                author: "Loading".to_string(),
                milestone,
                source_branch: "unknown".to_string(),
                target_branch: "unknown".to_string(),
                ..MrData::default()
            },
        }
    }
}

/// Drops the display sentinels (`"None"`, `"none"`, `"Loading"`, blank) that older
/// versions stored in place of a missing assignee or milestone, so state files
/// written before `Option` was used restore as `None`.
pub fn without_sentinel(value: Option<String>) -> Option<String> {
    value.filter(|s| !matches!(s.trim(), "" | "None" | "none" | "Loading"))
}

#[cfg(feature = "stats")]
impl TrackedMr {
    /// Builds the stats snapshot for this MR. Shared by the startup backfill and the
    /// live `MrLoaded` recorder so both always store the same fields the same way.
    pub fn stats_snapshot(
        &self,
        trigger: gitlab_tracker_stats::snapshot::SnapshotTrigger,
        project_id: &str,
        profile: &DifficultyProfile,
    ) -> gitlab_tracker_stats::snapshot::MrStatsSnapshot {
        let diff = self.diff_stats.as_ref();
        gitlab_tracker_stats::snapshot::MrStatsSnapshot {
            mr_id: self.id.clone(),
            project_id: project_id.to_string(),
            title: self.title.clone(),
            trigger,
            author: self.author.clone(),
            assignee: self.assignee.clone(),
            reviewers: self.reviewers.clone(),
            merged_by: self.merged_by.clone(),
            milestone: self.milestone.clone(),
            labels: self.labels.clone(),
            target_branch: self.target_branch.clone(),
            state: self.state.as_str().to_string(),
            created_at: self.created_at.clone(),
            merged_at: self.merged_at.clone(),
            updated_at: self.updated_at.clone(),
            files_changed: diff.map(|d| d.files_changed).unwrap_or(0),
            additions: diff.map(|d| d.additions).unwrap_or(0),
            deletions: diff.map(|d| d.deletions).unwrap_or(0),
            commits_count: diff.map(|d| d.commits_count).unwrap_or(0),
            diff_difficulty: diff.map(|d| d.difficulty(profile)),
            user_notes_count: self.user_notes_count,
            pipeline_count: self.pipelines.len() as u32,
            pipeline_failure_count: self
                .pipelines
                .iter()
                .filter(|p| p.status == PipelineState::Failed)
                .count() as u32,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MrLoadedData {
    pub id: String,
    pub branches: HashSet<String>,
    /// Mergeability resolved from the GitLab API response.
    pub mergeability: MergeabilityStatus,
    /// Fresh GitLab data of the MR.
    pub data: MrData,
}

/// Diff statistics for a merge request: files changed, lines added, lines deleted.
///
/// Fetched from the GitLab Changes API and used to display a compact summary
/// (e.g. \\\"9 files  +545 -32\\\") alongside a review-difficulty score.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DiffStats {
    /// Number of files touched by this MR.
    pub files_changed: u32,
    /// Total lines added across all changed files.
    pub additions: u32,
    /// Total lines deleted across all changed files.
    pub deletions: u32,
    /// Number of commits in this MR, as returned by the GitLab Changes API.
    #[serde(default)]
    pub commits_count: u32,
    /// Number of commits the source branch is behind the target branch.
    ///
    /// - `None`  → not applicable (MR merged/closed, or status is Mergeable/Unknown)
    /// - `Some(0)` → up to date (Mergeable)
    /// - `Some(n)` → n commits behind the target branch
    ///
    /// Fetched from `GET /repository/compare?from=<target>&to=<source>` only for
    /// open MRs whose mergeability is not `Mergeable`.
    #[serde(default)]
    pub commits_behind: Option<u32>,
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

/// Review-effort band of a [`DiffStats::difficulty`] score — the single place that
/// knows the 0.33 / 0.66 boundaries (table, inspector, filters, notifications).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Easy,
    Medium,
    Complex,
}

impl Effort {
    pub fn from_score(score: f64) -> Self {
        if score < 0.33 {
            Effort::Easy
        } else if score < 0.66 {
            Effort::Medium
        } else {
            Effort::Complex
        }
    }

    /// Badge label, e.g. `"🟢 EASY"`.
    pub fn label(self) -> &'static str {
        match self {
            Effort::Easy => "🟢 EASY",
            Effort::Medium => "🟡 MEDIUM",
            Effort::Complex => "🔴 COMPLEX",
        }
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
        error: crate::gitlab::GitlabError,
    },
    /// Fired while GitLab is still computing mergeability and the fetcher is retrying.
    MrMergeabilityRetrying {
        id: String,
    },
    /// Fired when the user requests adding a new MR to the tracking list (by ID).
    /// `apply_event` is the single place that pushes to `app.mrs` and recomputes
    /// the API call estimate — `events.rs` must never mutate `app.mrs` directly.
    MrAdded(String),
    /// Fired when the user removes a MR: Delete key on the selected row, or `-<id>`
    /// typed in the input field.
    MrRemovedById(String),
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
    /// `Err` is shown in the Log Time popup.
    ActivitiesLoaded(Result<Vec<gitlab_tracker_core::Activity>, gitlab_tracker_core::TrackerError>),
    /// Fired when time entries for a ticket have been fetched from the tracker.
    /// Keyed by ticket id so a late response can never land on another ticket.
    TimeEntriesLoaded {
        ticket_id: String,
        /// `Err` is shown in the TimeLog view.
        entries: Result<Vec<gitlab_tracker_core::TimeEntry>, gitlab_tracker_core::TrackerError>,
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
    /// The error is shown inline in the popup.
    TimeLogFailed {
        error: gitlab_tracker_core::TrackerError,
    },
    /// Fired when an async stats aggregation finishes. `generation` identifies the
    /// request (see `StatsViewState::apply_result`); the report is boxed to keep the
    /// enum variant size in check (Clippy `large_enum_variant`).
    #[cfg(feature = "stats")]
    StatsReportLoaded {
        generation: u64,
        result: Result<Box<gitlab_tracker_stats::StatReport>, gitlab_tracker_stats::StatsError>,
    },
    /// Fired after a snapshot was written to the stats DB; marks the report stale.
    #[cfg(feature = "stats")]
    StatsSnapshotRecorded,
    /// Fired when an MR discovery poll completes: `mr_ids` are the IIDs not yet in the
    /// tracking list (possibly none), `next_anchor` the advanced `created_after` anchor.
    /// Only emitted when `discover_new_mrs = true` in `[project.stats]`.
    NewMrsDiscovered {
        mr_ids: Vec<String>,
        next_anchor: String,
    },
    Tick,
}

impl AppEvent {
    /// Whether applying this event changes the tracked-MR list or its data, so the
    /// state file must be rewritten (once per drained batch, see the main loop).
    /// A failed fetch does not persist: errors are not saved to disk.
    pub fn persists_state(&self) -> bool {
        matches!(
            self,
            AppEvent::MrAdded(_) | AppEvent::MrRemovedById(_) | AppEvent::MrLoaded(_)
        )
    }
}
