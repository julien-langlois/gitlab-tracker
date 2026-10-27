use crate::LinkedTicket;

/// A single entry in the filter picker popup.
///
/// Each filter is self-contained: it carries its own label and predicate.
/// Plugins register their filters via `inventory::submit!(FilterDef { … })` — no
/// change to `app.rs` or `mod.rs` is needed when a new filter is added.
///
/// # Display order
/// Entries are sorted by `priority` (ascending) when `collect_all_filters` is called.
/// Convention:
///   - `0–99`   → built-in GitLab filters (state, mergeability, flags, …)
///   - `100–199` → first-party tracker plugin filters (Redmine, Jira, …)
///   - `200+`   → community / third-party plugin filters
pub struct FilterDef {
    /// Unique machine-readable identifier (e.g. `"all"`, `"flagged"`, `"has_linked_ticket"`).
    /// Must be stable across versions — it is used as the persistence key.
    pub id: &'static str,

    /// Label displayed in the filter picker popup (e.g. `"All (no filter)"`).
    pub label: &'static str,

    /// Short label shown in the table header when this filter is active, when it
    /// differs from `label` (`None`: the header shows `label`). For parametric
    /// filters (Milestone, Assignee) this is a prefix — the runtime appends the
    /// query value: `"Milestone: sprint-42"`. Read it through [`FilterDef::active_label`].
    pub active_label: Option<&'static str>,

    /// Display order — lower values appear first in the picker list.
    pub priority: u16,

    /// Whether this filter requires a free-text input field below the list.
    ///
    /// When `true`, the picker renders an extra text input row and the runtime
    /// passes the input value to `apply` as the `query` argument.
    pub needs_text_input: bool,

    /// When `Some`, the filter is only offered while this runtime condition holds
    /// (e.g. the "me" filters need `gitlab_username`). `None`: always available.
    pub requires: Option<crate::Requirement>,

    /// Pure predicate — returns `true` when the MR should be visible.
    ///
    /// `mr` is a borrowed [`MrSnapshot`] of the MR's fields, so `core` never depends
    /// on the app's `TrackedMr`. Its `linked_ticket` lets tracker-aware filters
    /// inspect the resolved ticket without a separate callback.
    ///
    /// `query` is the trimmed text-input value for parametric filters (empty string
    /// for non-parametric ones — the predicate should ignore it in that case).
    #[allow(clippy::type_complexity)]
    pub apply: fn(mr: MrSnapshot<'_>, query: &str) -> bool,
}

impl FilterDef {
    /// Label shown in the table header while this filter is active.
    pub fn active_label(&self) -> &'static str {
        self.active_label.unwrap_or(self.label)
    }
}

/// A lightweight, borrow-based snapshot of the fields a filter predicate may inspect.
///
/// Avoids importing `TrackedMr` (which lives in `gitlab-tracker` and depends on
/// `ratatui`) into `core`. The orchestrator constructs this on each filter call.
///
/// The struct is `#[non_exhaustive]` so that a new field is not a breaking change:
/// outside this crate, build it with [`MrSnapshot::new`] and assign the fields you
/// need (plugin tests typically do this).
///
/// ```
/// use gitlab_tracker_core::{GitlabMrState, MergeabilityStatus, MrSnapshot};
///
/// let mut mr = MrSnapshot::new(GitlabMrState::Opened, MergeabilityStatus::Mergeable, "main");
/// mr.flagged = true;
/// mr.assignee = Some("alice");
/// assert!(mr.reviewers.is_empty());
/// ```
#[non_exhaustive]
pub struct MrSnapshot<'a> {
    pub flagged: bool,
    pub state: crate::GitlabMrState,
    pub mergeability: crate::MergeabilityStatus,
    pub user_notes_count: u32,
    pub milestone: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub linked_ticket: Option<&'a LinkedTicket>,
    /// Status of the most recent pipeline, if any.
    pub pipeline_status: Option<crate::PipelineState>,
    /// Reviewer display strings for this MR (e.g. "Alice (@alice)").
    pub reviewers: &'a [String],
    /// GitLab username of the currently logged-in user, as configured in `projects.toml`.
    /// `None` when `gitlab_username` is not set for the active project.
    pub gitlab_username: Option<&'a str>,
    /// Review-difficulty score in [0.0, 1.0] computed from diff stats, or `None`
    /// when diff stats have not been fetched yet for this MR.
    pub diff_difficulty: Option<f64>,
    /// The Git branch this MR targets (e.g. "main", "develop", "release/1.x").
    pub target_branch: &'a str,
    /// Tracked branches this MR is known to be missing from: neither its merge
    /// commit nor a cherry-pick of it is there. Branches still loading are omitted.
    pub absent_branches: &'a [&'a str],
    /// Usernames of the MR's current approvers.
    pub approved_by: &'a [String],
    /// GitLab labels on the MR, as returned by the API (free-form like
    /// `"Redesign - Sprint 4"` or scoped like `"review::ongoing"`).
    pub labels: &'a [String],
}

impl<'a> MrSnapshot<'a> {
    /// Builds a snapshot from the fields that have no neutral value; every other
    /// field starts unset (`false`, `0`, `None` or empty) and is assigned afterwards.
    pub fn new(
        state: crate::GitlabMrState,
        mergeability: crate::MergeabilityStatus,
        target_branch: &'a str,
    ) -> Self {
        Self {
            flagged: false,
            state,
            mergeability,
            user_notes_count: 0,
            milestone: None,
            assignee: None,
            linked_ticket: None,
            pipeline_status: None,
            reviewers: &[],
            gitlab_username: None,
            diff_difficulty: None,
            target_branch,
            absent_branches: &[],
            approved_by: &[],
            labels: &[],
        }
    }
}

// Global registry — every `inventory::submit!(FilterDef { … })` anywhere in the
// dependency graph is collected here at startup.
inventory::collect!(FilterDef);

/// Collects all registered [`FilterDef`]s from every linked crate,
/// sorted by `priority` (ascending).
///
/// Call this once at startup to build the ordered filter list.
pub fn collect_all_filters() -> Vec<&'static FilterDef> {
    let mut filters: Vec<&'static FilterDef> = inventory::iter::<FilterDef>.into_iter().collect();
    filters.sort_by_key(|f| f.priority);
    filters
}
