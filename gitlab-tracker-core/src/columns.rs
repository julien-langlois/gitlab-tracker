/// A single optional column in the MR table.
///
/// Plugins register their columns via `inventory::submit!(ColumnDef { … })` — no
/// change to `config.rs`, `mod.rs` or `storage.rs` is needed when a new column
/// is added by a plugin crate.
///
/// # Display order
/// Columns are sorted by `priority` (ascending) when `collect_all_columns` is called.
/// Convention:
///   - `0–99`   → built-in columns (Activity, Target, Labels, Milestone, Notes, Diff)
///   - `100–199` → first-party tracker plugin columns (Redmine Ticket, Jira Issue, …)
///   - `200+`   → community / third-party plugin columns
pub struct ColumnDef {
    /// Unique machine-readable identifier (e.g. `"activity"`, `"tracker_ticket"`).
    /// Used as the persistence key in `projects.toml` — must be stable across versions.
    pub id: &'static str,

    /// Human-readable label shown in the column picker popup (e.g. `"Activity"`).
    pub label: &'static str,

    /// Whether this column is visible by default on a fresh install.
    pub default_visible: bool,

    /// Display order — lower values appear first in the column picker.
    pub priority: u16,

    /// When `Some`, the column is only shown when the runtime condition is met
    /// (e.g. a tracker provider is configured). The closure receives a single `bool`
    /// context value whose meaning is defined per-column in the orchestrator.
    ///
    /// `None` means the column is always available regardless of runtime state.
    pub requires_feature: Option<&'static str>,
}

// Global registry — every `inventory::submit!(ColumnDef { … })` anywhere in the
// dependency graph is collected here at startup.
inventory::collect!(ColumnDef);

/// Collects all registered [`ColumnDef`]s from every linked crate,
/// sorted by `priority` (ascending).
///
/// Call this once at startup to build the ordered column list.
pub fn collect_all_columns() -> Vec<&'static ColumnDef> {
    let mut cols: Vec<&'static ColumnDef> = inventory::iter::<ColumnDef>.into_iter().collect();
    cols.sort_by_key(|c| c.priority);
    cols
}
