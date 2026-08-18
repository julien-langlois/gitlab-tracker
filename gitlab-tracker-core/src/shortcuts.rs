/// A single keyboard shortcut entry displayed in the help popup.
pub struct ShortcutEntry {
    /// The key combination as shown to the user (e.g. `"?"`, `"j / k"`, `"Tab"`).
    pub key: &'static str,
    /// Short description of what the shortcut does.
    pub description: &'static str,
}

/// A named group of shortcut entries, one per lib (Core, Redmine, …).
///
/// Each lib that registers shortcuts produces exactly one `ShortcutBlock`.
/// The orchestrator collects all blocks and renders them section by section
/// in the help popup, sorted by `priority` (lowest first).
pub struct ShortcutBlock {
    /// Section header rendered above the entries (e.g. `"Core"`, `"Redmine"`).
    pub section: &'static str,
    /// Ordered list of shortcut entries belonging to this section.
    pub entries: &'static [ShortcutEntry],
    /// Display priority — lower values appear first in the help popup.
    ///
    /// Convention:
    ///   - `0`   → Core (always first)
    ///   - `100` → first-party tracker plugins (Redmine, Jira, …)
    ///   - `200` → third-party / community plugins
    pub priority: u8,
}

/// A registered shortcut factory: a plain function pointer that produces a [`ShortcutBlock`].
///
/// # Why a function pointer and not `Box<dyn Trait>`?
///
/// `inventory` requires collected types to be `'static`. A `fn() -> ShortcutBlock` is
/// trivially `'static` and requires no heap allocation at registration time, making it
/// ideal for link-time auto-registration.
///
/// # How to register a new provider (e.g. Jira)
///
/// In `gitlab-tracker-jira/src/shortcuts.rs`, add:
/// ```rust
/// fn jira_shortcuts() -> ShortcutBlock { /* … */ }
/// inventory::submit!(ShortcutFactory(jira_shortcuts));
/// ```
/// That's it — no change to `main.rs` or any other existing file.
pub struct ShortcutFactory(pub fn() -> ShortcutBlock);

// Declare the global registry. Every `inventory::submit!(ShortcutFactory(…))` call
// anywhere in the dependency graph (including optional/feature-gated crates that are
// actually linked) will be collected here at startup.
inventory::collect!(ShortcutFactory);

/// Collects all registered [`ShortcutBlock`]s from every linked crate,
/// sorted by `priority` (ascending) so Core always appears before plugins
/// regardless of link order.
///
/// Call this once at startup to populate `App::shortcut_providers`.
pub fn collect_all_blocks() -> Vec<ShortcutBlock> {
    let mut blocks: Vec<ShortcutBlock> = inventory::iter::<ShortcutFactory>
        .into_iter()
        .map(|factory| (factory.0)())
        .collect();
    // Stable sort preserves relative order of blocks with equal priority.
    blocks.sort_by_key(|b| b.priority);
    blocks
}
