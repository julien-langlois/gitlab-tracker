use gitlab_tracker_core::{ShortcutBlock, ShortcutEntry, ShortcutFactory};

/// Produces the Core shortcut block — all built-in keyboard shortcuts.
///
/// Registered via `inventory::submit!` so it is collected automatically at startup
/// without any explicit call in `main.rs`. Always appears first because this crate
/// is linked before any optional plugin crate.
fn core_shortcuts() -> ShortcutBlock {
    ShortcutBlock {
        section: "Core",
        priority: 0,
        entries: &[
            ShortcutEntry {
                key: "/ or i",
                description: "Enter Insert mode (search / add MR)",
            },
            ShortcutEntry {
                key: "Esc",
                description: "Cancel / confirm quit (press twice)",
            },
            ShortcutEntry {
                key: "Tab",
                description: "Cycle focus: Dashboard → Inspector → Tracker",
            },
            ShortcutEntry {
                key: "j / ↓",
                description: "Move down / scroll pane",
            },
            ShortcutEntry {
                key: "k / ↑",
                description: "Move up / scroll pane",
            },
            ShortcutEntry {
                key: "o / O",
                description: "Open selected MR in browser",
            },
            ShortcutEntry {
                key: "y / Y",
                description: "Copy MR URL to clipboard",
            },
            ShortcutEntry {
                key: "r / R",
                description: "Force refresh",
            },
            ShortcutEntry {
                key: "p / P",
                description: "Cycle Inspector view (MR Info / Pipelines)",
            },
            ShortcutEntry {
                key: "s",
                description: "Cycle sort column (Updated / ID / Milestone / Title)",
            },
            ShortcutEntry {
                key: "S",
                description: "Toggle sort order (ascending / descending)",
            },
            ShortcutEntry {
                key: "f / F",
                description: "Open filter picker",
            },
            ShortcutEntry {
                key: "Space",
                description: "Flag / unflag selected MR ★",
            },
            ShortcutEntry {
                key: "c / C",
                description: "Open column visibility picker",
            },
            ShortcutEntry {
                key: "t / T",
                description: "Focus Tracker pane / open ticket URL",
            },
            ShortcutEntry {
                key: "Del",
                description: "Remove selected MR from tracking",
            },
            ShortcutEntry {
                key: "?",
                description: "Show this help popup",
            },
        ],
    }
}

// Auto-registration: no call needed in main.rs.
// This submit! is executed at program startup by the inventory machinery.
inventory::submit!(ShortcutFactory(core_shortcuts));
