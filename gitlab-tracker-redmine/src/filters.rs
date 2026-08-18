use gitlab_tracker_core::{FilterDef, MrSnapshot};

// Redmine-specific filter: shows only MRs that have a resolved tracker ticket.
// Auto-registered via `inventory::submit!` — no mention in `app.rs` needed.
// Only linked (and thus active) when the `redmine` feature is enabled.
inventory::submit!(FilterDef {
    id: "has_linked_ticket",
    label: "Has linked ticket 🎫",
    active_label: "Has linked ticket 🎫",
    priority: 100,
    needs_text_input: false,
    apply: |mr: MrSnapshot<'_>, _| mr.linked_ticket.is_some(),
});
