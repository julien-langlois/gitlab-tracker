use crate::models::Effort;
use crate::utils::matches_gitlab_username;
use gitlab_tracker_core::{
    FilterDef, GitlabMrState, MergeabilityStatus, MrSnapshot, PipelineState, Requirement,
};

// ── Built-in GitLab filters — priority 0–99 ───────────────────────────────────

inventory::submit!(FilterDef {
    id: "all",
    label: "All (no filter)",
    active_label: Some("All"),
    priority: 0,
    needs_text_input: false,
    requires: None,
    apply: |_mr: MrSnapshot<'_>, _query: &str| true,
});

inventory::submit!(FilterDef {
    id: "flagged",
    label: "Flagged ★",
    active_label: None,
    priority: 1,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.flagged,
});

inventory::submit!(FilterDef {
    id: "state_opened",
    label: "State: Opened",
    active_label: None,
    priority: 2,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.state == GitlabMrState::Opened,
});

inventory::submit!(FilterDef {
    id: "state_merged",
    label: "State: Merged",
    active_label: None,
    priority: 3,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.state == GitlabMrState::Merged,
});

inventory::submit!(FilterDef {
    id: "state_closed",
    label: "State: Closed",
    active_label: None,
    priority: 4,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.state == GitlabMrState::Closed,
});

inventory::submit!(FilterDef {
    id: "mergeability_mergeable",
    label: "Mergeability: Mergeable",
    active_label: None,
    priority: 5,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::Mergeable,
});

inventory::submit!(FilterDef {
    id: "mergeability_conflict",
    label: "Mergeability: Conflict",
    active_label: None,
    priority: 6,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::Conflict,
});

inventory::submit!(FilterDef {
    id: "mergeability_rebase",
    label: "Mergeability: Needs Rebase",
    active_label: None,
    priority: 7,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::NeedsRebase,
});

inventory::submit!(FilterDef {
    id: "mergeability_not_approved",
    label: "Mergeability: Not Approved",
    active_label: None,
    priority: 8,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::NotApproved,
});

inventory::submit!(FilterDef {
    id: "mergeability_requested_changes",
    label: "Mergeability: Requested Changes",
    active_label: None,
    priority: 9,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::RequestedChanges,
});

inventory::submit!(FilterDef {
    id: "mergeability_draft",
    label: "Mergeability: Draft",
    active_label: None,
    priority: 10,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::Draft,
});

inventory::submit!(FilterDef {
    id: "mergeability_discussions",
    label: "Mergeability: Discussions",
    active_label: None,
    priority: 11,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.mergeability == MergeabilityStatus::DiscussionsNotResolved,
});

inventory::submit!(FilterDef {
    id: "has_notes",
    label: "Has comments 💬",
    active_label: None,
    priority: 12,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.user_notes_count > 0,
});

inventory::submit!(FilterDef {
    id: "ci_failing",
    label: "CI failing ❌",
    active_label: None,
    priority: 13,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| mr.pipeline_status == Some(PipelineState::Failed),
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
    active_label: None,
    priority: 14,
    needs_text_input: false,
    requires: Some(Requirement::GitlabUsername),
    apply: |mr: MrSnapshot<'_>, _| {
        // Visible only when gitlab_username is configured; predicate is a no-op
        // otherwise (the filter is hidden from the picker before it can be selected).
        let Some(username) = mr.gitlab_username else {
            return false;
        };
        mr.assignee
            .is_some_and(|assignee| matches_gitlab_username(assignee, username))
    },
});

inventory::submit!(FilterDef {
    id: "reviewer_me",
    label: "Reviewer: me 👁️",
    active_label: None,
    priority: 15,
    needs_text_input: false,
    requires: Some(Requirement::GitlabUsername),
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
    active_label: None,
    priority: 16,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| {
        mr.diff_difficulty
            .is_some_and(|d| Effort::from_score(d) == Effort::Easy)
    },
});

inventory::submit!(FilterDef {
    id: "effort_complex",
    label: "Effort: Complex 🔴",
    active_label: None,
    priority: 17,
    needs_text_input: false,
    requires: None,
    apply: |mr: MrSnapshot<'_>, _| {
        mr.diff_difficulty
            .is_some_and(|d| Effort::from_score(d) == Effort::Complex)
    },
});

// Parametric filters — need_text_input = true, priority 50+

inventory::submit!(FilterDef {
    id: "milestone",
    label: "Milestone… (type below)",
    active_label: Some("Milestone:"),
    priority: 50,
    needs_text_input: true,
    requires: None,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        if query.is_empty() {
            return true;
        }
        mr.milestone
            .is_some_and(|m| m.to_lowercase().contains(&query.to_lowercase()))
    },
});

inventory::submit!(FilterDef {
    id: "target_branch",
    label: "Target branch… (type below)",
    active_label: Some("Branch:"),
    priority: 52,
    needs_text_input: true,
    requires: None,
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
    active_label: Some("Assignee:"),
    priority: 51,
    needs_text_input: true,
    requires: None,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        if query.is_empty() {
            return true;
        }
        let q_lower = query.to_lowercase();
        let gitlab_match = mr
            .assignee
            .is_some_and(|a| a.to_lowercase().contains(&q_lower));
        let tracker_match = mr
            .linked_ticket
            .and_then(|t| t.assignee.as_deref())
            .map(|a| a.to_lowercase().contains(&q_lower))
            .unwrap_or(false);
        gitlab_match || tracker_match
    },
});
