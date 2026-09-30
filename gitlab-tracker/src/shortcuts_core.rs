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
                status_hint: Some("[i]/[/]: Insert"),
            },
            ShortcutEntry {
                key: "Tab",
                description: "Cycle focus: Dashboard → Inspector → Tracker",
                // The Tab hint is dynamic (shows current pane), handled separately in ui/mod.rs.
                status_hint: None,
            },
            ShortcutEntry {
                key: "s / S",
                description: "Cycle sort column / toggle sort order",
                // The sort hint is dynamic (shows current column+order), handled separately.
                status_hint: None,
            },
            ShortcutEntry {
                key: "f / F",
                description: "Open filter picker",
                status_hint: Some("[F]: Filter"),
            },
            ShortcutEntry {
                key: "Space",
                description: "Flag / unflag selected MR ★",
                status_hint: Some("[Space]: Flag"),
            },
            ShortcutEntry {
                key: "c / C",
                description: "Open column visibility picker",
                status_hint: Some("[C]: Columns"),
            },
            ShortcutEntry {
                key: "j / ↓ / k / ↑",
                description: "Move down / up, scroll pane",
                status_hint: Some("[▲/▼]: Scroll"),
            },
            ShortcutEntry {
                key: "o / O",
                description: "Open selected MR in browser",
                status_hint: Some("[O]: Open"),
            },
            ShortcutEntry {
                key: "r / R",
                description: "Force refresh",
                status_hint: Some("[R]: Refresh"),
            },
            ShortcutEntry {
                key: "Del",
                description: "Remove selected MR from tracking",
                status_hint: Some("[Del]: Delete"),
            },
            ShortcutEntry {
                key: "?",
                description: "Show this help popup",
                status_hint: Some("[?]: Help"),
            },
            ShortcutEntry {
                key: "Esc",
                description: "Cancel / confirm quit (press twice)",
                status_hint: Some("[Esc]: Quit"),
            },
            ShortcutEntry {
                key: "y / Y",
                description: "Copy `git clone` command for the MR branch",
                status_hint: None,
            },
            ShortcutEntry {
                key: "p / P",
                description: "Cycle Inspector view (MR Info / Pipelines)",
                status_hint: None,
            },
            ShortcutEntry {
                key: "t / T",
                description: "Focus Tracker pane / open ticket URL",
                status_hint: None,
            },
        ],
    }
}

// Auto-registration: no call needed in main.rs.
// This submit! is executed at program startup by the inventory machinery.
inventory::submit!(ShortcutFactory(core_shortcuts));
