use serde::{Deserialize, Serialize};

/// Discriminant that identifies why a snapshot was recorded.
///
/// Stored as a TEXT column in SQLite so historical data remains readable
/// without schema migrations when new variants are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::str::FromStr for SnapshotTrigger {
    type Err = String;

    /// Parses the `trigger` DB column. An unknown value is an error, never a silent
    /// `OnRefresh`: a row written by a newer version must not be miscounted.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "on_merge" => Ok(SnapshotTrigger::OnMerge),
            "on_close" => Ok(SnapshotTrigger::OnClose),
            "on_refresh" => Ok(SnapshotTrigger::OnRefresh),
            other => Err(format!("unknown snapshot trigger {other:?}")),
        }
    }
}

/// Formats an instant the way every `recorded_at` value is stored and compared:
/// UTC, second precision, `Z` suffix (`2026-09-29T10:00:00Z`). With a single fixed-width
/// format, the lexicographic order used by SQL filters is the chronological order.
pub fn format_timestamp(instant: chrono::DateTime<chrono::Utc>) -> String {
    instant.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Normalises any RFC 3339 timestamp (GitLab's `…T10:00:00.000Z`, `to_rfc3339()`'s
/// `…+00:00`, other offsets) to [`format_timestamp`]'s form. Unparseable input is
/// returned unchanged rather than dropped.
pub fn normalize_timestamp(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| format_timestamp(dt.with_timezone(&chrono::Utc)))
        .unwrap_or_else(|_| ts.to_string())
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

impl MrStatsSnapshot {
    /// When the event this snapshot records happened, normalised.
    ///
    /// Terminal events are dated at the real merge / close time (`updated_at` is the
    /// best available proxy for a close), so re-recording the same MR on every refresh
    /// hits the `UNIQUE(…, recorded_date, trigger)` constraint and is a no-op — instead
    /// of adding a fresh row dated today that inflates recent throughput.
    /// Open-MR refreshes are dated now.
    pub fn event_recorded_at(&self) -> String {
        let event_time = match self.trigger {
            SnapshotTrigger::OnMerge => self.merged_at.as_deref(),
            SnapshotTrigger::OnClose => self.updated_at.as_deref(),
            SnapshotTrigger::OnRefresh => None,
        };
        event_time
            .map(normalize_timestamp)
            .unwrap_or_else(|| format_timestamp(chrono::Utc::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_share_one_sortable_format() {
        // GitLab, `to_rfc3339()` and another offset all land on the same form.
        for ts in [
            "2026-09-29T10:00:00.000Z",
            "2026-09-29T10:00:00.123456789+00:00",
            "2026-09-29T12:00:00+02:00",
        ] {
            assert_eq!(normalize_timestamp(ts), "2026-09-29T10:00:00Z", "{ts}");
        }
        // The UTC day can differ from the local one: recorded_date follows UTC.
        assert_eq!(
            normalize_timestamp("2026-09-30T01:30:00+02:00"),
            "2026-09-29T23:30:00Z"
        );
        assert_eq!(normalize_timestamp("not a date"), "not a date");
    }

    #[test]
    fn unknown_trigger_is_an_error() {
        for trigger in [
            SnapshotTrigger::OnMerge,
            SnapshotTrigger::OnClose,
            SnapshotTrigger::OnRefresh,
        ] {
            assert_eq!(trigger.as_str().parse::<SnapshotTrigger>(), Ok(trigger));
        }
        assert!("on_reopen".parse::<SnapshotTrigger>().is_err());
    }
}
