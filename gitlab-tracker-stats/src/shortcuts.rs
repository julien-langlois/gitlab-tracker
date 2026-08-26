use gitlab_tracker_core::{ShortcutBlock, ShortcutEntry, ShortcutFactory};

/// Produces the Stats shortcut block — keyboard shortcuts for the Stats overlay.
///
/// Auto-registered via `inventory::submit!`: when the `stats` feature is enabled
/// this crate is linked and the submit! runs automatically at startup.
/// No mention of this crate is needed anywhere in `main.rs`.
fn stats_shortcuts() -> ShortcutBlock {
    ShortcutBlock {
        section: "Stats",
        priority: 50,
        entries: &[
            ShortcutEntry {
                key: "g / G",
                description: "Open / close the Stats fullscreen overlay",
                // Appears in the Normal-mode status bar only when this crate is linked.
                status_hint: Some("[G]: Stats"),
            },
            ShortcutEntry {
                key: "w / W",
                description: "Cycle time window (30d → 90d → 365d → All time)",
                status_hint: None,
            },
            ShortcutEntry {
                key: "j / ↓",
                description: "Scroll stats content down",
                status_hint: None,
            },
            ShortcutEntry {
                key: "k / ↑",
                description: "Scroll stats content up",
                status_hint: None,
            },
            ShortcutEntry {
                key: "PgDn / PgUp",
                description: "Scroll stats content by 10 lines",
                status_hint: None,
            },
            ShortcutEntry {
                key: "Esc",
                description: "Close the Stats overlay",
                status_hint: None,
            },
        ],
    }
}

// Auto-registration: executes at startup when this crate is linked (i.e. feature "stats").
inventory::submit!(ShortcutFactory(stats_shortcuts));
