//! Desktop notification plugin for gitlab-tracker.
//!
//! Compiled with the `desktop` feature (on by default) to send OS notifications
//! via `notify-rust`. Build with `--no-default-features` for a zero-dependency
//! stub suitable for headless / CI environments.

// ── Notification events ──────────────────────────────────────────────────────

/// Notify that an MR has appeared on a branch it was not previously seen on.
#[cfg(feature = "desktop")]
pub fn mr_on_new_branch(mr_id: &str, title: &str, branch: &str) {
    let _ = notify_rust::Notification::new()
        .summary("GitLab MR Tracker")
        .body(&format!(
            "MR !{} ({}) is now present on branch '{}'!",
            mr_id, title, branch
        ))
        .icon("dialog-information")
        .show();
}

/// Notify that an MR's `updated_at` field has changed (i.e. the MR was modified).
#[cfg(feature = "desktop")]
pub fn mr_updated(mr_id: &str, title: &str, updated_at: Option<&str>) {
    let _ = notify_rust::Notification::new()
        .summary(&format!("MR !{} updated", mr_id))
        .body(&format!(
            "{}\n{}",
            title,
            updated_at.unwrap_or("unknown date")
        ))
        .icon("dialog-information")
        .show();
}

/// Notify that an MR's mergeability status has changed.
/// Accepts string labels so this crate stays independent of gitlab-tracker model types.
#[cfg(feature = "desktop")]
pub fn mr_mergeability_changed(mr_id: &str, title: &str, old: &str, new: &str) {
    let _ = notify_rust::Notification::new()
        .summary(&format!("MR !{} — mergeability changed", mr_id))
        .body(&format!("{}\n{} → {}", title, old, new))
        .icon("dialog-warning")
        .show();
}

/// Notify that an MR's milestone has changed.
#[cfg(feature = "desktop")]
pub fn mr_milestone_changed(mr_id: &str, title: &str, old: &str, new: &str) {
    let _ = notify_rust::Notification::new()
        .summary(&format!("MR !{} — milestone changed", mr_id))
        .body(&format!("{}\n{} → {}", title, old, new))
        .icon("dialog-information")
        .show();
}

// ── No-op stubs when the `desktop` feature is disabled ───────────────────────

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_on_new_branch(_mr_id: &str, _title: &str, _branch: &str) {}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_updated(_mr_id: &str, _title: &str, _updated_at: Option<&str>) {}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_mergeability_changed(_mr_id: &str, _title: &str, _old: &str, _new: &str) {}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_milestone_changed(_mr_id: &str, _title: &str, _old: &str, _new: &str) {}
