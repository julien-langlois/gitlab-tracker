//! Desktop notification plugin for gitlab-tracker.
//!
//! Compiled with the `desktop` feature (on by default) to send OS notifications
//! via `notify-rust`. Build with `--no-default-features` for a zero-dependency
//! stub suitable for headless / CI environments.
//!
//! When the user clicks a notification, the MR URL is opened in the default browser.
//!
//! The public functions never block: they only queue the notification. A single
//! background worker thread performs the (synchronous) D-Bus calls, so a slow or
//! hung notification daemon can no longer freeze the UI loop.

// ── Internal helper ───────────────────────────────────────────────────────────

/// A notification queued for the worker thread.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
struct Toast {
    summary: String,
    body: String,
    icon: &'static str,
    action_label: &'static str,
    url: String,
}

/// Maximum number of notifications waiting for a click at the same time. Each one
/// needs its own thread blocked in `wait_for_action` until the notification closes.
#[cfg(feature = "desktop")]
const MAX_CLICK_WAITERS: usize = 8;

/// Queues `toast` for the notification worker, started on first use.
#[cfg(feature = "desktop")]
fn send(toast: Toast) {
    use std::sync::{mpsc, OnceLock};

    static WORKER: OnceLock<Option<mpsc::Sender<Toast>>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Toast>();
        std::thread::Builder::new()
            .name("notify".into())
            .spawn(move || {
                for toast in rx {
                    show(toast);
                }
            })
            .ok()
            .map(|_| tx)
    });
    if let Some(tx) = worker {
        let _ = tx.send(toast);
    }
}

#[cfg(not(feature = "desktop"))]
#[inline(always)]
fn send(_toast: Toast) {}

/// Shows `toast` (runs on the worker thread) and, if the user clicks it, opens its URL.
#[cfg(feature = "desktop")]
fn show(toast: Toast) {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CLICK_WAITERS: AtomicUsize = AtomicUsize::new(0);

    let Ok(handle) = notify_rust::Notification::new()
        .summary(&toast.summary)
        .body(&toast.body)
        .icon(toast.icon)
        .action("default", toast.action_label)
        .show()
    else {
        return;
    };

    // ponytail: bounded waiter count; beyond it the notification still shows but a
    // click does nothing. A shared D-Bus signal listener would lift the limit.
    if CLICK_WAITERS.fetch_add(1, Ordering::Relaxed) >= MAX_CLICK_WAITERS {
        CLICK_WAITERS.fetch_sub(1, Ordering::Relaxed);
        return;
    }
    let url = toast.url;
    let spawned = std::thread::Builder::new()
        .name("notify-click".into())
        .spawn(move || {
            handle.wait_for_action(|action| {
                if action == "default" {
                    let _ = open::that(&url);
                }
            });
            CLICK_WAITERS.fetch_sub(1, Ordering::Relaxed);
        });
    if spawned.is_err() {
        CLICK_WAITERS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Queues a "field changed" notification: `<title>\n<old> → <new>`.
fn changed(summary: String, title: &str, old: &str, new: &str, icon: &'static str, url: &str) {
    send(Toast {
        summary,
        body: format!("{title}\n{old} → {new}"),
        icon,
        action_label: "Open MR",
        url: url.to_owned(),
    });
}

// ── MR notification events ───────────────────────────────────────────────────

/// Notify that an MR has appeared on a branch it was not previously seen on.
pub fn mr_on_new_branch(mr_id: &str, title: &str, branch: &str, web_url: &str) {
    send(Toast {
        summary: "GitLab MR Tracker".to_string(),
        body: format!("MR !{mr_id} ({title}) is now present on branch '{branch}'!"),
        icon: "dialog-information",
        action_label: "Open MR",
        url: web_url.to_owned(),
    });
}

/// Notify that an MR's `updated_at` field has changed (i.e. the MR was modified).
pub fn mr_updated(mr_id: &str, title: &str, updated_at: Option<&str>, web_url: &str) {
    send(Toast {
        summary: format!("MR !{mr_id} updated"),
        body: format!("{title}\n{}", updated_at.unwrap_or("unknown date")),
        icon: "dialog-information",
        action_label: "Open MR",
        url: web_url.to_owned(),
    });
}

/// Notify that an MR's mergeability status has changed.
/// Accepts string labels so this crate stays independent of gitlab-tracker model types.
pub fn mr_mergeability_changed(mr_id: &str, title: &str, old: &str, new: &str, web_url: &str) {
    let summary = format!("MR !{mr_id} — mergeability changed");
    changed(summary, title, old, new, "dialog-warning", web_url);
}

/// Notify that an MR's review complexity category has changed (e.g. EASY → COMPLEX).
///
/// Fires when the computed difficulty score crosses a category boundary during a refresh.
/// `old` and `new` are human-readable labels matching the UI badges (e.g. `"🟢 EASY"`,
/// `"🟡 MEDIUM"`, `"🔴 COMPLEX"`).
pub fn mr_complexity_changed(mr_id: &str, title: &str, old: &str, new: &str, web_url: &str) {
    let summary = format!("MR !{mr_id} — complexity changed");
    changed(summary, title, old, new, "dialog-warning", web_url);
}

/// Notify that an MR's milestone has changed.
pub fn mr_milestone_changed(mr_id: &str, title: &str, old: &str, new: &str, web_url: &str) {
    let summary = format!("MR !{mr_id} — milestone changed");
    changed(summary, title, old, new, "dialog-information", web_url);
}

// ── Tracker ticket notification events ───────────────────────────────────────

/// Notify that a tracked field on a linked tracker ticket has changed.
///
/// This is a **single generic entry point** for all ticket field changes.
/// The `field` parameter is a human-readable label (e.g. `"priority"`, `"status"`),
/// sourced from `gitlab_tracker_core::TicketChange::field_label`.
///
/// Using one function instead of per-field functions means that adding a new tracked
/// field in `core` (e.g. `Sprint`) requires **zero changes** to this crate.
///
/// # Icon selection
/// Priority changes use `"dialog-warning"` (yellow); all others use `"dialog-information"`.
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
    send(Toast {
        summary: format!("Ticket #{ticket_id} — {field} changed"),
        body: format!("{mr_title}\n{old} → {new}"),
        icon,
        action_label: "Open ticket",
        url: ticket_url.to_owned(),
    });
}

/// Notify that a linked tracker ticket was automatically transitioned by the app.
pub fn ticket_status_transitioned(
    ticket_id: &str,
    mr_title: &str,
    old_status: &str,
    new_status: &str,
    ticket_url: &str,
) {
    send(Toast {
        summary: format!("Ticket #{ticket_id} — status transitioned"),
        body: format!("{mr_title}\n{old_status} → {new_status}"),
        icon: "dialog-information",
        action_label: "Open ticket",
        url: ticket_url.to_owned(),
    });
}
