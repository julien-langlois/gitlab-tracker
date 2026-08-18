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

    /// Short label shown in the table header when this filter is active.
    /// For parametric filters (Milestone, Assignee) this is a prefix — the runtime
    /// appends the query value: `"Milestone: sprint-42"`.
    pub active_label: &'static str,

    /// Display order — lower values appear first in the picker list.
    pub priority: u16,

    /// Whether this filter requires a free-text input field below the list.
    ///
    /// When `true`, the picker renders an extra text input row and the runtime
    /// passes the input value to `apply` as the `query` argument.
    pub needs_text_input: bool,

    /// Pure predicate — returns `true` when the MR should be visible.
    ///
    /// `mr_*` fields mirror the flat fields of `TrackedMr` without importing the
    /// type directly, keeping `core` free of any UI/app dependency.
    ///
    /// `linked_ticket` is `Option<&LinkedTicket>` so tracker-aware filters can
    /// inspect the resolved ticket without adding a separate callback.
    ///
    /// `query` is the trimmed text-input value for parametric filters (empty string
    /// for non-parametric ones — the predicate should ignore it in that case).
    #[allow(clippy::type_complexity)]
    pub apply: fn(mr: MrSnapshot<'_>, query: &str) -> bool,
}

/// A lightweight, borrow-based snapshot of the fields a filter predicate may inspect.
///
/// Avoids importing `TrackedMr` (which lives in `gitlab-tracker` and depends on
/// `ratatui`) into `core`. The orchestrator constructs this on each filter call.
pub struct MrSnapshot<'a> {
    pub flagged: bool,
    pub state: &'a str,
    pub mergeability: &'a str,
    pub user_notes_count: u32,
    pub milestone: &'a str,
    pub assignee: &'a str,
    pub linked_ticket: Option<&'a LinkedTicket>,
    pub pipeline_status: Option<&'a str>,
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
