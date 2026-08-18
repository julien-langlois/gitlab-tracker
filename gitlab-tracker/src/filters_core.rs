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
