use chrono::{DateTime, Utc};

use crate::db::StoredSnapshot;
use crate::snapshot::SnapshotTrigger;

/// Per-MR metrics derived from a single [`StoredSnapshot`].
///
/// All duration fields are expressed in **fractional hours** so they can be
/// averaged, ranked, and correlated without unit-conversion boilerplate at
/// the call site. `None` means the required timestamps were absent.
#[derive(Debug, Clone)]
pub struct PerMrMetrics {
    pub mr_id: String,
    pub project_id: String,
    pub author: String,
    pub milestone: Option<String>,
    pub trigger: SnapshotTrigger,

    /// Total elapsed time from MR creation to merge, in hours.
    /// `None` when `created_at` or `merged_at` is absent.
    pub cycle_time_hours: Option<f64>,

    /// Total changed lines (additions + deletions).
    pub diff_size: u32,

    /// Pre-computed review difficulty in [0.0, 1.0], if available.
    pub diff_difficulty: Option<f64>,

    /// Raw comment count.
    pub user_notes_count: u32,

    /// Number of commits in the MR.
    pub commits_count: u32,

    /// Fraction of pipeline runs that failed: `failure_count / total_count`.
    /// `None` when no pipelines were recorded.
    pub pipeline_failure_rate: Option<f64>,

    /// Comments per 100 changed lines — normalised discussion density.
    /// `None` when `diff_size` is 0.
    pub comment_density: Option<f64>,
}

impl PerMrMetrics {
    /// Derives all metrics from a stored snapshot.
    pub fn from_snapshot(snap: &StoredSnapshot) -> Self {
        let cycle_time_hours = compute_duration_hours(
            snap.snapshot.created_at.as_deref(),
            snap.snapshot.merged_at.as_deref(),
        );

        let diff_size = snap.snapshot.additions + snap.snapshot.deletions;

        let pipeline_failure_rate = if snap.snapshot.pipeline_count > 0 {
            Some(snap.snapshot.pipeline_failure_count as f64 / snap.snapshot.pipeline_count as f64)
        } else {
            None
        };

        let comment_density = if diff_size > 0 {
            Some(snap.snapshot.user_notes_count as f64 / diff_size as f64 * 100.0)
        } else {
            None
        };

        Self {
            mr_id: snap.snapshot.mr_id.clone(),
            project_id: snap.snapshot.project_id.clone(),
            author: snap.snapshot.author.clone(),
            milestone: snap.snapshot.milestone.clone(),
            trigger: snap.snapshot.trigger,
            cycle_time_hours,
            commits_count: snap.snapshot.commits_count,
            diff_size,
            diff_difficulty: snap.snapshot.diff_difficulty,
            user_notes_count: snap.snapshot.user_notes_count,
            pipeline_failure_rate,
            comment_density,
        }
    }
}

/// Parses two ISO 8601 timestamps and returns their difference in fractional hours.
/// Returns `None` when either string is missing or unparseable.
fn compute_duration_hours(from: Option<&str>, to: Option<&str>) -> Option<f64> {
    let from: DateTime<Utc> = from?.parse().ok()?;
    let to: DateTime<Utc> = to?.parse().ok()?;
    let delta = to.signed_duration_since(from);
    // Negative durations (data anomaly) are treated as absent.
    if delta.num_seconds() < 0 {
        return None;
    }
    Some(delta.num_seconds() as f64 / 3600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_hours_basic() {
        let hours =
            compute_duration_hours(Some("2024-01-01T08:00:00Z"), Some("2024-01-01T10:30:00Z"));
        assert!((hours.unwrap() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn duration_hours_negative_returns_none() {
        let hours =
            compute_duration_hours(Some("2024-01-01T10:00:00Z"), Some("2024-01-01T08:00:00Z"));
        assert!(hours.is_none());
    }

    #[test]
    fn duration_hours_missing_returns_none() {
        assert!(compute_duration_hours(None, Some("2024-01-01T10:00:00Z")).is_none());
    }
}
