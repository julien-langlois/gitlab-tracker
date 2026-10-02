//! Change detection on MR loads: which branches and fields changed since the last
//! known state, logged and surfaced as desktop notifications, plus the review
//! reminders for MRs awaiting my review. Split out of `app.rs`.

use super::*;
use crate::models::MrLoadedData;
use chrono::{DateTime, Utc};

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
        let complexity_label = |score: f64| crate::models::Effort::from_score(score).label();
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

/// What was last reminded for one MR awaiting my review (session memory only:
/// after a restart the waiting reminder fires again at the first refresh cycle).
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct ReviewReminded {
    waiting_at: Option<DateTime<Utc>>,
    push_at: Option<DateTime<Utc>>,
}

/// Reminders due for an MR awaiting my review, as `(waiting too long, new push)`,
/// recording them in `prev`. Waiting: at least `threshold_hours` since the request,
/// repeated every `threshold_hours`. Push: the author pushed after my last activity
/// on the MR, once per push. `threshold_hours == 0` disables both.
fn due_review_reminders(
    waiting_since: Option<DateTime<Utc>>,
    last_push: Option<DateTime<Utc>>,
    my_last_activity: Option<DateTime<Utc>>,
    prev: &mut ReviewReminded,
    now: DateTime<Utc>,
    threshold_hours: u64,
) -> (bool, bool) {
    if threshold_hours == 0 {
        return (false, false);
    }
    let threshold = chrono::Duration::hours(i64::try_from(threshold_hours).unwrap_or(i64::MAX));
    let waiting = waiting_since.is_some_and(|since| now - since >= threshold)
        && prev.waiting_at.is_none_or(|at| now - at >= threshold);
    if waiting {
        prev.waiting_at = Some(now);
    }
    let pushed = matches!((last_push, my_last_activity), (Some(push), Some(mine)) if push > mine)
        && prev.push_at != last_push;
    if pushed {
        prev.push_at = last_push;
    }
    (waiting, pushed)
}

/// Sends the review reminders due for `mr` (see [`due_review_reminders`]). An MR no
/// longer awaiting `username` forgets its reminders. Before `notify_allowed` (startup
/// sync) the latest push is only recorded, so restarts do not re-announce old pushes.
pub(super) fn remind_review(
    mr: &TrackedMr,
    username: &str,
    threshold_hours: u64,
    notify_allowed: bool,
    reminders: &mut HashMap<String, ReviewReminded>,
) {
    if !crate::utils::awaiting_review_by(
        mr.state,
        mr.mergeability,
        &mr.reviewers,
        &mr.approved_by,
        username,
    ) {
        reminders.remove(&mr.id);
        return;
    }
    let parse = |at: Option<&str>| {
        at.and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&Utc))
    };
    let review = mr.review.as_ref();
    let last_push = parse(review.and_then(|r| r.last_push_at.as_deref()));
    let entry = reminders.entry(mr.id.clone()).or_default();
    if !notify_allowed {
        entry.push_at = last_push;
        return;
    }
    let now = Utc::now();
    let waiting_since =
        crate::utils::review_waiting_since(review, mr.created_at.as_deref(), username);
    let (waiting, pushed) = due_review_reminders(
        waiting_since,
        last_push,
        parse(review.and_then(|r| r.last_activity_by.get(username).map(String::as_str))),
        entry,
        now,
        threshold_hours,
    );
    if let (true, Some(since)) = (waiting, waiting_since) {
        notify::review_waiting(&mr.id, &mr.title, (now - since).num_hours(), &mr.web_url);
    }
    if pushed {
        notify::review_new_push(&mr.id, &mr.title, &mr.web_url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_reminders_respect_threshold_repeat_and_push() {
        let t0 = DateTime::parse_from_rfc3339("2026-10-01T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let h = |n: i64| t0 + chrono::Duration::hours(n);
        let mut prev = ReviewReminded::default();

        // Under the threshold, then over it, then not again before another 24 h.
        assert_eq!(
            due_review_reminders(Some(t0), None, None, &mut prev, h(23), 24),
            (false, false)
        );
        assert_eq!(
            due_review_reminders(Some(t0), None, None, &mut prev, h(25), 24),
            (true, false)
        );
        assert_eq!(
            due_review_reminders(Some(t0), None, None, &mut prev, h(30), 24),
            (false, false)
        );
        assert_eq!(
            due_review_reminders(Some(t0), None, None, &mut prev, h(49), 24),
            (true, false)
        );
        // Disabled.
        let mut fresh = ReviewReminded::default();
        assert_eq!(
            due_review_reminders(Some(t0), Some(h(2)), Some(h(1)), &mut fresh, h(99), 0),
            (false, false)
        );

        // A push after my last activity notifies once; without my activity, never.
        let mut prev = ReviewReminded::default();
        assert_eq!(
            due_review_reminders(None, Some(h(2)), Some(h(1)), &mut prev, h(3), 24),
            (false, true)
        );
        assert_eq!(
            due_review_reminders(None, Some(h(2)), Some(h(1)), &mut prev, h(4), 24),
            (false, false)
        );
        assert_eq!(
            due_review_reminders(None, Some(h(5)), Some(h(1)), &mut prev, h(6), 24),
            (false, true)
        );
        assert_eq!(
            due_review_reminders(
                None,
                Some(h(2)),
                Some(h(3)),
                &mut ReviewReminded::default(),
                h(4),
                24
            ),
            (false, false)
        );
        assert_eq!(
            due_review_reminders(
                None,
                Some(h(2)),
                None,
                &mut ReviewReminded::default(),
                h(4),
                24
            ),
            (false, false)
        );
    }
}
