use crate::models::Effort;
use crate::utils::{awaiting_review_by, matches_gitlab_username};
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
    // The id predates the "to review" meaning: kept, it is the persistence key.
    id: "reviewer_me",
    label: "To review: me 👁️",
    active_label: None,
    priority: 15,
    needs_text_input: false,
    requires: Some(Requirement::GitlabUsername),
    // Reviews still waiting for me (open, not draft, not approved by me yet); the
    // table lists them oldest request first (`App::compute_visible_indices`).
    apply: |mr: MrSnapshot<'_>, _| {
        // Visible only when gitlab_username is configured; predicate is a no-op
        // otherwise (the filter is hidden from the picker before it can be selected).
        let Some(username) = mr.gitlab_username else {
            return false;
        };
        awaiting_review_by(
            mr.state,
            mr.mergeability,
            mr.reviewers,
            mr.approved_by,
            username,
        )
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

inventory::submit!(FilterDef {
    id: "absent_from_branch",
    label: "Absent from branch… (type below)",
    active_label: Some("Absent from:"),
    priority: 53,
    needs_text_input: true,
    requires: None,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        let query = query.trim();
        mr.absent_branches
            .iter()
            .any(|b| b.eq_ignore_ascii_case(query))
    },
});

inventory::submit!(FilterDef {
    id: "label",
    label: "Label… (type below)",
    active_label: Some("Label:"),
    priority: 54,
    needs_text_input: true,
    requires: None,
    apply: |mr: MrSnapshot<'_>, query: &str| {
        let query = normalize_label(query);
        if query.is_empty() {
            return true;
        }
        mr.labels
            .iter()
            .any(|label| label_matches(&normalize_label(label), &query))
    },
});

/// Lowercases a label and drops the spaces around `::`, so `"Review :: Ongoing"`
/// and `"review::ongoing"` compare equal.
pub(crate) fn normalize_label(label: &str) -> String {
    label
        .trim()
        .split("::")
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("::")
        .to_lowercase()
}

/// The label starts with the query and the query ends on a word boundary:
/// `"Project Foo"` picks `"Project Foo - Sprint 4"` and `"Project Foo - Sprint 5"`,
/// `"review"` or `"review::"` the whole `review::` scope, but `"Sprint 4"` never
/// `"Sprint 40"` nor `"Project F"` `"Project Foo"`. Both sides normalized.
fn label_matches(label: &str, query: &str) -> bool {
    let Some(rest) = label.strip_prefix(query) else {
        return false;
    };
    let ends_word = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    ends_word(query.chars().next_back()) || ends_word(rest.chars().next())
}

#[cfg(test)]
mod tests {
    use gitlab_tracker_core::{
        collect_all_filters, FilterDef, GitlabMrState, LinkedTicket, MergeabilityStatus,
        MrSnapshot, PipelineState, Requirement, LINKED_TICKET_SCHEMA_VERSION,
    };

    fn filter(id: &str) -> &'static FilterDef {
        collect_all_filters()
            .into_iter()
            .find(|f| f.id == id)
            .unwrap_or_else(|| panic!("filter `{id}` is not registered"))
    }

    fn snapshot<'a>(state: GitlabMrState, mergeability: MergeabilityStatus) -> MrSnapshot<'a> {
        MrSnapshot::new(state, mergeability, "main")
    }

    fn open<'a>() -> MrSnapshot<'a> {
        snapshot(GitlabMrState::Opened, MergeabilityStatus::Mergeable)
    }

    #[test]
    fn registry_is_sorted_with_unique_ids() {
        let filters = collect_all_filters();
        assert_eq!(filters[0].id, "all");
        assert!(filters.windows(2).all(|w| w[0].priority <= w[1].priority));
        let mut ids: Vec<&str> = filters.iter().map(|f| f.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate filter id");

        // Parametric filters show a short prefix next to the typed value.
        for id in [
            "milestone",
            "assignee",
            "target_branch",
            "absent_from_branch",
            "label",
        ] {
            let def = filter(id);
            assert!(def.needs_text_input && def.active_label.is_some(), "{id}");
        }
        // The "me" filters are hidden until a GitLab username is configured.
        for id in ["assigned_to_me", "reviewer_me"] {
            assert_eq!(
                filter(id).requires,
                Some(Requirement::GitlabUsername),
                "{id}"
            );
        }
    }

    #[test]
    fn state_and_mergeability_filters_match_their_variant_only() {
        use GitlabMrState::{Closed, Merged, Opened};
        use MergeabilityStatus as M;

        assert!((filter("all").apply)(snapshot(Closed, M::Conflict), ""));
        for (id, state) in [
            ("state_opened", Opened),
            ("state_merged", Merged),
            ("state_closed", Closed),
        ] {
            for candidate in [Opened, Merged, Closed] {
                let matched = (filter(id).apply)(snapshot(candidate, M::Mergeable), "");
                assert_eq!(matched, candidate == state, "{id} on {candidate:?}");
            }
        }
        let by_mergeability = [
            ("mergeability_mergeable", M::Mergeable),
            ("mergeability_conflict", M::Conflict),
            ("mergeability_rebase", M::NeedsRebase),
            ("mergeability_not_approved", M::NotApproved),
            ("mergeability_requested_changes", M::RequestedChanges),
            ("mergeability_draft", M::Draft),
            ("mergeability_discussions", M::DiscussionsNotResolved),
        ];
        for (id, wanted) in by_mergeability {
            for (_, candidate) in by_mergeability {
                let matched = (filter(id).apply)(snapshot(Opened, candidate), "");
                assert_eq!(matched, candidate == wanted, "{id} on {candidate:?}");
            }
        }
    }

    #[test]
    fn flag_threads_and_pipeline_filters() {
        let mut mr = open();
        assert!(!(filter("flagged").apply)(open(), ""));
        mr.flagged = true;
        assert!((filter("flagged").apply)(mr, ""));

        // `user_notes_count` holds the unresolved review threads.
        let mut mr = open();
        assert!(!(filter("has_notes").apply)(open(), ""));
        mr.user_notes_count = 2;
        assert!((filter("has_notes").apply)(mr, ""));

        for (status, failing) in [
            (None, false),
            (Some(PipelineState::Success), false),
            (Some(PipelineState::Running), false),
            (Some(PipelineState::Failed), true),
        ] {
            let mut mr = open();
            mr.pipeline_status = status;
            assert_eq!((filter("ci_failing").apply)(mr, ""), failing, "{status:?}");
        }
    }

    #[test]
    fn me_filters_need_the_username_and_match_the_at_handle() {
        let reviewers = ["Alice (@alice)".to_string(), "Bob (@bob)".to_string()];
        let approved = ["bob".to_string()];
        let assigned = filter("assigned_to_me");
        let to_review = filter("reviewer_me");

        let mr = |username: Option<&'static str>| {
            let mut mr = open();
            mr.assignee = Some("Alice (@alice)");
            mr.reviewers = &reviewers;
            mr.approved_by = &approved;
            mr.gitlab_username = username;
            mr
        };
        // No username configured: the predicates match nothing.
        assert!(!(assigned.apply)(mr(None), ""));
        assert!(!(to_review.apply)(mr(None), ""));

        assert!((assigned.apply)(mr(Some("alice")), ""));
        assert!(!(assigned.apply)(mr(Some("bob")), ""));
        // The display name alone is not the handle.
        assert!(!(assigned.apply)(mr(Some("Alice")), ""));

        // alice still has to review; bob already approved; carol is not a reviewer.
        assert!((to_review.apply)(mr(Some("alice")), ""));
        assert!(!(to_review.apply)(mr(Some("bob")), ""));
        assert!(!(to_review.apply)(mr(Some("carol")), ""));
        // Drafts and merged MRs are not waiting for a review.
        for (state, mergeability) in [
            (GitlabMrState::Opened, MergeabilityStatus::Draft),
            (GitlabMrState::Merged, MergeabilityStatus::Mergeable),
        ] {
            let mut mr = snapshot(state, mergeability);
            mr.reviewers = &reviewers;
            mr.gitlab_username = Some("alice");
            assert!(!(to_review.apply)(mr, ""), "{state:?} / {mergeability:?}");
        }
    }

    #[test]
    fn effort_filters_follow_the_inspector_bands() {
        let matches = |id: &str, score: Option<f64>| {
            let mut mr = open();
            mr.diff_difficulty = score;
            (filter(id).apply)(mr, "")
        };
        assert!(matches("effort_easy", Some(0.0)) && matches("effort_easy", Some(0.32)));
        assert!(!matches("effort_easy", Some(0.33)));
        assert!(!matches("effort_complex", Some(0.65)));
        assert!(matches("effort_complex", Some(0.66)) && matches("effort_complex", Some(1.0)));
        // Medium is in neither; an MR without diff stats is excluded from both.
        assert!(!matches("effort_easy", Some(0.5)) && !matches("effort_complex", Some(0.5)));
        assert!(!matches("effort_easy", None) && !matches("effort_complex", None));
    }

    #[test]
    fn text_filters_are_case_insensitive_and_pass_everything_when_empty() {
        let ticket = LinkedTicket {
            schema_version: LINKED_TICKET_SCHEMA_VERSION,
            id: "12".into(),
            subject: "Subject".into(),
            status: "New".into(),
            url: "https://tracker.example/12".into(),
            author: None,
            assignee: Some("Thomas Dubosc".into()),
            time_estimate: None,
            time_spent: None,
            time_remaining: None,
            tracker_type: None,
            priority: None,
            version: None,
            start_date: None,
            done_ratio: None,
        };
        let absent = ["production", "staging"];
        let mr = || {
            let mut mr = MrSnapshot::new(
                GitlabMrState::Opened,
                MergeabilityStatus::Mergeable,
                "release/1.x",
            );
            mr.milestone = Some("Sprint 12");
            mr.assignee = Some("Alice (@alice)");
            mr.linked_ticket = Some(&ticket);
            mr.absent_branches = &absent;
            mr
        };

        for id in ["milestone", "assignee", "target_branch"] {
            assert!((filter(id).apply)(open(), ""), "{id}: empty query");
        }
        assert!((filter("milestone").apply)(mr(), "sprint"));
        assert!(!(filter("milestone").apply)(mr(), "sprint 13"));
        assert!(
            !(filter("milestone").apply)(open(), "sprint"),
            "no milestone"
        );

        assert!((filter("target_branch").apply)(mr(), "RELEASE/"));
        assert!(!(filter("target_branch").apply)(mr(), "main"));

        // The assignee filter also searches the linked ticket's assignee.
        assert!((filter("assignee").apply)(mr(), "ALICE"));
        assert!((filter("assignee").apply)(mr(), "dubosc"));
        assert!(!(filter("assignee").apply)(mr(), "carol"));

        // Exact branch name (trimmed, any case) — and nothing for an empty query.
        let absent_from = filter("absent_from_branch");
        assert!((absent_from.apply)(mr(), " Production "));
        assert!(!(absent_from.apply)(mr(), "prod"));
        assert!(!(absent_from.apply)(mr(), "main"));
        assert!(!(absent_from.apply)(mr(), ""));
    }

    #[test]
    fn label_filter_matches_label_prefixes_ending_on_a_word() {
        let labels = [
            "Project Foo - Sprint 4".to_string(),
            "review::ongoing".to_string(),
            "priority::high::p1".to_string(),
        ];
        let matches = |query: &str| {
            let mut mr = open();
            mr.labels = &labels;
            (filter("label").apply)(mr, query)
        };

        assert!(matches(""), "empty query");
        // Whole label, any case, surrounding spaces ignored.
        assert!(matches(" project foo - sprint 4 "));
        assert!(matches("REVIEW::ONGOING"));
        assert!(matches("review :: ongoing"), "spaces around ::");
        // The start of a label, up to a word boundary.
        assert!(matches("project foo"));
        assert!(matches("Project Foo -"));
        assert!(!matches("Project F"), "cuts a word");
        assert!(!matches("Sprint 4"), "not the start of the label");
        assert!(!matches("review::done"));
        // A scope, with or without its `::`, nested scopes included.
        assert!(matches("review"));
        assert!(matches("review::"));
        assert!(matches("Priority::"));
        assert!(matches("priority::high"));
        assert!(!matches("view::"));
        assert!(!matches("priority::hi"));
        // An MR without labels never matches a non-empty query.
        assert!(!(filter("label").apply)(open(), "review::"));
    }

    #[test]
    fn label_prefix_stops_at_the_end_of_a_word() {
        let labels = ["Project Foo - Sprint 40".to_string()];
        let matches = |query: &str| {
            let mut mr = open();
            mr.labels = &labels;
            (filter("label").apply)(mr, query)
        };
        // Picking "Sprint 4" (a suggestion or typed) leaves "Sprint 40" out.
        assert!(!matches("Project Foo - Sprint 4"));
        assert!(matches("Project Foo - Sprint 40"));
        assert!(matches("Project Foo - Sprint"));
    }
}
