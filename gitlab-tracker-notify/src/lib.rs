//! Desktop notification plugin for gitlab-tracker.
//!
//! Compiled with the `desktop` feature (on by default) to send OS notifications
//! via `notify-rust`. Build with `--no-default-features` for a zero-dependency
//! stub suitable for headless / CI environments.
//!
//! When the user clicks a notification, the MR URL is opened in the default browser.

// ── Internal helper ───────────────────────────────────────────────────────────

/// Show a notification and, if the user clicks it, open `url` in the default browser.
/// The D-Bus action wait is performed in a detached thread to avoid blocking the caller.
#[cfg(feature = "desktop")]
fn show_with_url(notification: notify_rust::Notification, url: String) {
    if let Ok(handle) = notification.show() {
        std::thread::spawn(move || {
            handle.wait_for_action(|action| {
                if action == "default" {
                    let _ = open::that(&url);
                }
            });
        });
    }
}

// ── MR notification events ───────────────────────────────────────────────────

/// Notify that an MR has appeared on a branch it was not previously seen on.
#[cfg(feature = "desktop")]
pub fn mr_on_new_branch(mr_id: &str, title: &str, branch: &str, web_url: &str) {
    let notification = notify_rust::Notification::new()
        .summary("GitLab MR Tracker")
        .body(&format!(
            "MR !{} ({}) is now present on branch '{}'!",
            mr_id, title, branch
        ))
        .icon("dialog-information")
        .action("default", "Open MR")
        .finalize();
    show_with_url(notification, web_url.to_owned());
}

/// Notify that an MR's `updated_at` field has changed (i.e. the MR was modified).
#[cfg(feature = "desktop")]
pub fn mr_updated(mr_id: &str, title: &str, updated_at: Option<&str>, web_url: &str) {
    let notification = notify_rust::Notification::new()
        .summary(&format!("MR !{} updated", mr_id))
        .body(&format!(
            "{}\n{}",
            title,
            updated_at.unwrap_or("unknown date")
        ))
        .icon("dialog-information")
        .action("default", "Open MR")
        .finalize();
    show_with_url(notification, web_url.to_owned());
}

/// Notify that an MR's mergeability status has changed.
/// Accepts string labels so this crate stays independent of gitlab-tracker model types.
#[cfg(feature = "desktop")]
pub fn mr_mergeability_changed(mr_id: &str, title: &str, old: &str, new: &str, web_url: &str) {
    let notification = notify_rust::Notification::new()
        .summary(&format!("MR !{} — mergeability changed", mr_id))
        .body(&format!("{}\n{} → {}", title, old, new))
        .icon("dialog-warning")
        .action("default", "Open MR")
        .finalize();
    show_with_url(notification, web_url.to_owned());
}

/// Notify that an MR's milestone has changed.
#[cfg(feature = "desktop")]
pub fn mr_milestone_changed(mr_id: &str, title: &str, old: &str, new: &str, web_url: &str) {
    let notification = notify_rust::Notification::new()
        .summary(&format!("MR !{} — milestone changed", mr_id))
        .body(&format!("{}\n{} → {}", title, old, new))
        .icon("dialog-information")
        .action("default", "Open MR")
        .finalize();
    show_with_url(notification, web_url.to_owned());
}

// ── Tracker ticket notification events ───────────────────────────────────────

/// Notify that a tracked field on a linked tracker ticket has changed.
///
/// This is a **single generic entry point** for all ticket field changes.
/// The `field` parameter is a human-readable label (e.g. `"priority"`, `"status"`),
/// sourced from [`gitlab_tracker_core::TicketChange::field_label`].
///
/// Using one function instead of per-field functions means that adding a new tracked
/// field in `core` (e.g. `Sprint`) requires **zero changes** to this crate.
/// The orchestrator maps `TicketChange` variants to this function directly.
///
/// # Icon selection
/// Priority changes use `"dialog-warning"` (yellow); all others use `"dialog-information"`.
#[cfg(feature = "desktop")]
pub fn ticket_field_changed(
    ticket_id: &str,
    mr_title: &str,
    field: &str,
    old: &str,
    new: &str,
    ticket_url: &str,
) {
    let icon = if field == "priority" {
        "dialog-warning"
    } else {
        "dialog-information"
    };
    let notification = notify_rust::Notification::new()
        .summary(&format!("Ticket #{} — {} changed", ticket_id, field))
        .body(&format!("{}\n{} → {}", mr_title, old, new))
        .icon(icon)
        .action("default", "Open ticket")
        .finalize();
    show_with_url(notification, ticket_url.to_owned());
}

// ── No-op stubs when the `desktop` feature is disabled ───────────────────────

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_on_new_branch(_mr_id: &str, _title: &str, _branch: &str, _web_url: &str) {}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_updated(_mr_id: &str, _title: &str, _updated_at: Option<&str>, _web_url: &str) {}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_mergeability_changed(_mr_id: &str, _title: &str, _old: &str, _new: &str, _web_url: &str) {
}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn mr_milestone_changed(_mr_id: &str, _title: &str, _old: &str, _new: &str, _web_url: &str) {}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
pub fn ticket_field_changed(
    _ticket_id: &str,
    _mr_title: &str,
    _field: &str,
    _old: &str,
    _new: &str,
    _ticket_url: &str,
) {
}
