//! Change detection on MR loads: which branches and fields changed since the last
//! known state, logged and surfaced as desktop notifications. Split out of `app.rs`.

use super::*;
use crate::models::MrLoadedData;

/// Branches an MR newly appeared on, for `mr_on_new_branch` notifications.
///
/// An MR with no persisted branch set (`previously_known == None`) seen during the
/// startup sync is a first sighting, not a change: its branches are recorded
/// silently instead of sending one notification per branch (first-launch avalanche).
/// Once known — even with an empty set, e.g. an open MR — every new branch notifies,
/// including changes that happened while the app was closed.
pub(super) fn branches_to_notify<'a>(
    previously_known: Option<&HashSet<String>>,
    branches: &'a HashSet<String>,
    notify_allowed: bool,
) -> Vec<&'a String> {
    if previously_known.is_none() && !notify_allowed {
        return Vec::new();
    }
    branches
        .iter()
        .filter(|b| !previously_known.is_some_and(|known| known.contains(*b)))
        .collect()
}

/// Logs field-level changes between the tracked MR (`old`) and a fresh load (`new`)
/// and, when `notify_allowed`, sends the matching desktop notifications
/// (`updated_at`, mergeability, milestone, complexity category). Must run before
/// `new` is written into `old`.
pub(super) fn log_and_notify_changes(
    old: &TrackedMr,
    new: &MrLoadedData,
    was_updated: bool,
    notify_allowed: bool,
    profile: &crate::models::DifficultyProfile,
) {
    // Trace field-level changes so they are visible in the log file.
    // All comparisons happen before the fields are overwritten below.
    if was_updated {
        tracing::info!(
            mr_id = %new.id,
            old = %old.updated_at.as_deref().unwrap_or("none"),
            new = %new.data.updated_at.as_deref().unwrap_or("none"),
            "MR updated_at changed",
        );
        if notify_allowed {
            notify::mr_updated(
                &new.id,
                &new.data.title,
                new.data.updated_at.as_deref(),
                &new.data.web_url,
            );
        }
    }
    if old.mergeability != new.mergeability {
        tracing::info!(
            mr_id = %new.id,
            old = ?old.mergeability,
            new = ?new.mergeability,
            "MR mergeability changed",
        );
        if notify_allowed {
            notify::mr_mergeability_changed(
                &new.id,
                &new.data.title,
                // Same labels as the badges ("REBASE", "CONFLICT"…).
                crate::ui::table::mergeability_badge(&old.mergeability).0,
                crate::ui::table::mergeability_badge(&new.mergeability).0,
                &new.data.web_url,
            );
        }
    }
    if old.milestone != new.data.milestone {
        let old_milestone = old.milestone.as_deref().unwrap_or(NO_VALUE);
        let new_milestone = new.data.milestone.as_deref().unwrap_or(NO_VALUE);
        tracing::info!(mr_id = %new.id, old = old_milestone, new = new_milestone, "MR milestone changed");
        if notify_allowed {
            notify::mr_milestone_changed(
                &new.id,
                &new.data.title,
                old_milestone,
                new_milestone,
                &new.data.web_url,
            );
        }
    }

    // Detect complexity category changes (EASY / MEDIUM / COMPLEX) before
    // overwriting the stored diff_stats. Only fires when both old and new
    // stats are available and the category boundary is actually crossed.
    {
        let complexity_label = |score: f64| -> &'static str {
            if score < 0.33 {
                "🟢 EASY"
            } else if score < 0.66 {
                "🟡 MEDIUM"
            } else {
                "🔴 COMPLEX"
            }
        };
        if let (Some(old_stats), Some(new_stats)) = (&old.diff_stats, &new.data.diff_stats) {
            let old_label = complexity_label(old_stats.difficulty(profile));
            let new_label = complexity_label(new_stats.difficulty(profile));
            if old_label != new_label {
                notify::mr_complexity_changed(
                    &old.id,
                    &new.data.title,
                    old_label,
                    new_label,
                    &new.data.web_url,
                );
            }
        }
    }
}
