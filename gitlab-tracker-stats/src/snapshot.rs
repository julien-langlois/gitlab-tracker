use serde::{Deserialize, Serialize};

/// Discriminant that identifies why a snapshot was recorded.
///
/// Stored as a TEXT column in SQLite so historical data remains readable
/// without schema migrations when new variants are added.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotTrigger {
    /// MR transitioned to the `Merged` state — snapshot contains complete timing data.
    OnMerge,
    /// MR transitioned to the `Closed` state — used to distinguish abandonment from merge.
    OnClose,
    /// Periodic refresh of an open MR (at most once per calendar day).
    /// Enables backlog-age tracking and stagnation detection.
    OnRefresh,
}

impl SnapshotTrigger {
    /// Returns the canonical string stored in the `trigger` DB column.
    pub fn as_str(&self) -> &'static str {
        match self {
            SnapshotTrigger::OnMerge => "on_merge",
            SnapshotTrigger::OnClose => "on_close",
            SnapshotTrigger::OnRefresh => "on_refresh",
        }
    }
}

impl std::fmt::Display for SnapshotTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Owned, borrow-free snapshot of a MR at the moment it is recorded.
///
/// Constructed by the orchestrator (`gitlab-tracker`) from a `TrackedMr` or
/// `MrLoadedData` — this crate never imports those types directly, preserving
/// the zero-TUI-dependency constraint.
///
/// All timestamp fields use ISO 8601 strings (as returned by the GitLab API)
/// so no timezone conversion is required at ingestion time. Conversion to
/// `chrono::DateTime` happens lazily in [`crate::metrics`] when durations are
/// computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MrStatsSnapshot {
    // ── Identity ─────────────────────────────────────────────────────────────
    /// Internal GitLab MR identifier (iid as string, e.g. "42").
    pub mr_id: String,

    /// GitLab numeric project ID (e.g. "12345").
    pub project_id: String,

    /// MR title at the time of recording.
    pub title: String,

    /// Discriminant: why was this snapshot recorded?
    pub trigger: SnapshotTrigger,

    // ── People ───────────────────────────────────────────────────────────────
    /// GitLab username of the MR author.
    pub author: String,

    /// GitLab username of the current assignee, if any.
    pub assignee: Option<String>,

    /// GitLab usernames of all assigned reviewers.
    pub reviewers: Vec<String>,

    /// GitLab username of the user who merged the MR (`on_merge` only).
    pub merged_by: Option<String>,

    // ── Classification ───────────────────────────────────────────────────────
    /// Milestone title associated with this MR, if any.
    pub milestone: Option<String>,

    /// Labels attached to the MR at the time of recording.
    pub labels: Vec<String>,

    /// Target branch (e.g. "main", "develop").
    pub target_branch: String,

    /// Final lifecycle state: "opened" | "merged" | "closed".
    pub state: String,

    // ── Timestamps (ISO 8601) ─────────────────────────────────────────────────
    /// When the MR was created.
    pub created_at: Option<String>,

    /// When the MR was merged (`on_merge` only).
    pub merged_at: Option<String>,

    /// When the MR was last updated.
    pub updated_at: Option<String>,

    // ── Diff statistics ───────────────────────────────────────────────────────
    /// Number of files touched.
    pub files_changed: u32,

    /// Total lines added.
    pub additions: u32,

    /// Total lines deleted.
    pub deletions: u32,

    /// Number of commits in this MR.
    pub commits_count: u32,

    /// Pre-computed review difficulty score in [0.0, 1.0], if available.
    pub diff_difficulty: Option<f64>,

    // ── Review signals ────────────────────────────────────────────────────────
    /// Total number of human notes (comments + threads).
    pub user_notes_count: u32,

    /// Total number of pipeline runs attached to this MR.
    pub pipeline_count: u32,

    /// Number of pipeline runs that ended in `Failed` state.
    pub pipeline_failure_count: u32,
}
