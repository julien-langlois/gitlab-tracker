use gitlab_tracker_core::ColumnDef;

// Redmine-specific column: the linked tracker ticket ID + status badge.
// Auto-registered via `inventory::submit!` — no mention in `config.rs` needed.
// Only rendered when a tracker provider is configured at runtime (`requires_feature`
// is checked by the orchestrator in `render_column_picker`).
inventory::submit!(ColumnDef {
    id: "tracker_ticket",
    label: "Tracker",
    default_visible: false,
    priority: 100,
    requires_feature: Some("tracker"),
});
