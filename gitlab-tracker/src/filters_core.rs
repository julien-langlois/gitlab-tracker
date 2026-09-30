use crate::utils::matches_gitlab_username;
use gitlab_tracker_core::{FilterDef, MrSnapshot};

// ── Built-in GitLab filters — priority 0–99 ───────────────────────────────────

inventory::submit!(FilterDef {
    id: "all",
    label: "All (no filter)",
    active_label: "All",
    priority: 0,
    needs_text_input: false,
    apply: |_mr: MrSnapshot<'_>, _query: &str| true,
});

inventory::submit!(FilterDef {
    id: "flagged",
    label: "Flagged ★",
    active_label: "Flagged ★",
    priority: 1,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.flagged,
});

inventory::submit!(FilterDef {
    id: "state_opened",
    label: "State: Opened",
    active_label: "State: Opened",
    priority: 2,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.state == "opened",
});

inventory::submit!(FilterDef {
    id: "state_merged",
    label: "State: Merged",
    active_label: "State: Merged",
    priority: 3,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.state == "merged",
});

inventory::submit!(FilterDef {
    id: "state_closed",
    label: "State: Closed",
    active_label: "State: Closed",
    priority: 4,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.state == "closed",
});

inventory::submit!(FilterDef {
    id: "mergeability_mergeable",
    label: "Mergeability: Mergeable",
    active_label: "Mergeability: Mergeable",
    priority: 5,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "Mergeable",
});

inventory::submit!(FilterDef {
    id: "mergeability_conflict",
    label: "Mergeability: Conflict",
    active_label: "Mergeability: Conflict",
    priority: 6,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "Conflict",
});

inventory::submit!(FilterDef {
    id: "mergeability_rebase",
    label: "Mergeability: Needs Rebase",
    active_label: "Mergeability: Needs Rebase",
    priority: 7,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "NeedsRebase",
});

inventory::submit!(FilterDef {
    id: "mergeability_not_approved",
    label: "Mergeability: Not Approved",
    active_label: "Mergeability: Not Approved",
    priority: 8,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "NotApproved",
});

inventory::submit!(FilterDef {
    id: "mergeability_requested_changes",
    label: "Mergeability: Requested Changes",
    active_label: "Mergeability: Requested Changes",
    priority: 9,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "RequestedChanges",
});

inventory::submit!(FilterDef {
    id: "mergeability_draft",
    label: "Mergeability: Draft",
    active_label: "Mergeability: Draft",
    priority: 10,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "Draft",
});

inventory::submit!(FilterDef {
    id: "mergeability_discussions",
    label: "Mergeability: Discussions",
    active_label: "Mergeability: Discussions",
    priority: 11,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == "DiscussionsNotResolved",
});

inventory::submit!(FilterDef {
    id: "has_notes",
    label: "Has comments 💬",
    active_label: "Has comments 💬",
    priority: 12,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.user_notes_count > 0,
});

inventory::submit!(FilterDef {
    id: "ci_failing",
    label: "CI failing ❌",
    active_label: "CI failing ❌",
    priority: 13,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.pipeline_status == Some("Failed"),
});

// ── "Me" filters — only shown when gitlab_username is configured (priority 14–15) ──
//
// These filters match the currently configured GitLab username against the MR's
// assignee or reviewer list. They are always registered in the inventory but
// suppressed from the picker UI at runtime by `App::visible_filter_defs()` when
// `config.gitlab_username` is `None` — so they never appear for users who have
// not set their username in `projects.toml`.
//
// The `query` argument carries the runtime username value (injected by the
// `App::visible_filter_defs` / `apply_filter` path — see `app.rs`).
// For non-parametric "me" filters `needs_text_input` is `false`; the predicate
// reads from `mr.gitlab_username` directly instead.

inventory::submit!(FilterDef {
    id: "assigned_to_me",
    label: "Assigned to me 👤",
    active_label: "Assigned to me 👤",
    priority: 14,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| {
        // Visible only when gitlab_username is configured; predicate is a no-op
        // otherwise (the filter is hidden from the picker before it can be selected).
        let Some(username) = mr.gitlab_username else {
            return false;
        };
        matches_gitlab_username(mr.assignee, username)
    },
});

inventory::submit!(FilterDef {
    id: "reviewer_me",
    label: "Reviewer: me 👁️",
    active_label: "Reviewer: me 👁️",
    priority: 15,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| {
        // Visible only when gitlab_username is configured; predicate is a no-op
        // otherwise (the filter is hidden from the picker before it can be selected).
        let Some(username) = mr.gitlab_username else {
            return false;
        };
        mr.reviewers
            .iter()
            .any(|reviewer| matches_gitlab_username(reviewer, username))
    },
});

// ── Effort filters (priority 16–17) ──────────────────────────────────────────
//
// Match on the pre-computed `diff_difficulty` score in [0.0, 1.0].
// Thresholds mirror the colour bands rendered in the Inspector panel:
//   score < 0.33  → Easy  (green)
//   score < 0.66  → Medium (yellow)
//   score ≥ 0.66  → Complex (red)
// MRs whose diff stats have not been fetched yet (score = None) are excluded.

inventory::submit!(FilterDef {
    id: "effort_easy",
    label: "Effort: Easy 🟢",
    active_label: "Effort: Easy 🟢",
    priority: 16,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| { mr.diff_difficulty.is_some_and(|d| d < 0.33) },
});

inventory::submit!(FilterDef {
    id: "effort_complex",
    label: "Effort: Complex 🔴",
    active_label: "Effort: Complex 🔴",
    priority: 17,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| { mr.diff_difficulty.is_some_and(|d| d >= 0.66) },
});

// Parametric filters — need_text_input = true, priority 50+

inventory::submit!(FilterDef {
    id: "milestone",
    label: "Milestone… (type below)",
    active_label: "Milestone:",
    priority: 50,
    needs_text_input: true,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        if query.is_empty() {
            return true;
        }
        mr.milestone.to_lowercase().contains(&query.to_lowercase())
    },
});

inventory::submit!(FilterDef {
    id: "target_branch",
    label: "Target branch… (type below)",
    active_label: "Branch:",
    priority: 52,
    needs_text_input: true,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        if query.is_empty() {
            return true;
        }
        mr.target_branch
            .to_lowercase()
            .contains(&query.to_lowercase())
    },
});

inventory::submit!(FilterDef {
    id: "assignee",
    label: "Assignee… (type below)",
    active_label: "Assignee:",
    priority: 51,
    needs_text_input: true,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        if query.is_empty() {
            return true;
        }
        let q_lower = query.to_lowercase();
        let gitlab_match = mr.assignee.to_lowercase().contains(&q_lower);
        let tracker_match = mr
            .linked_ticket
            .and_then(|t| t.assignee.as_deref())
            .map(|a| a.to_lowercase().contains(&q_lower))
            .unwrap_or(false);
        gitlab_match || tracker_match
    },
});
