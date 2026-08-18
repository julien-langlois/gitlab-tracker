use gitlab_tracker_core::{ShortcutBlock, ShortcutEntry, ShortcutFactory};

/// Produces the Redmine shortcut block — shortcuts specific to the Redmine integration.
///
/// Auto-registered via `inventory::submit!`: when the `redmine` feature is enabled
/// this crate is linked and the submit! runs automatically at startup.
/// No mention of Redmine is needed anywhere in `main.rs`.
fn redmine_shortcuts() -> ShortcutBlock {
    ShortcutBlock {
        section: "Redmine",
        priority: 100,
        entries: &[
            ShortcutEntry {
                key: "l / L",
                description: "Open Log Time popup on the linked ticket",
            },
            ShortcutEntry {
                key: "p / P",
                description: "Cycle Tracker pane view (Ticket Info / Time Log)",
            },
            ShortcutEntry {
                key: "t / T",
                description: "Open linked Redmine ticket in browser",
            },
        ],
    }
}

// Auto-registration: executes at startup when this crate is linked (i.e. feature "redmine").
inventory::submit!(ShortcutFactory(redmine_shortcuts));
