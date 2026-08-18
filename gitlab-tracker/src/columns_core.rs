use gitlab_tracker_core::ColumnDef;

// ── Built-in columns — priority 0–99 ─────────────────────────────────────────

inventory::submit!(ColumnDef {
    id: "activity",
    label: "Activity",
    default_visible: false,
    priority: 0,
    requires_feature: None,
});

inventory::submit!(ColumnDef {
    id: "target_branch",
    label: "Target branch",
    default_visible: false,
    priority: 1,
    requires_feature: None,
});

inventory::submit!(ColumnDef {
    id: "labels",
    label: "Labels",
    default_visible: false,
    priority: 2,
    requires_feature: None,
});

inventory::submit!(ColumnDef {
    id: "milestone",
    label: "Milestone",
    default_visible: false,
    priority: 3,
    requires_feature: None,
});

inventory::submit!(ColumnDef {
    id: "notes",
    label: "Notes",
    default_visible: false,
    priority: 4,
    requires_feature: None,
});

inventory::submit!(ColumnDef {
    id: "diff_stats",
    label: "Complexity",
    default_visible: false,
    priority: 5,
    requires_feature: None,
});
