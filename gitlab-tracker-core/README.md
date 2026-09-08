# gitlab-tracker-core

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker-core)](https://crates.io/crates/gitlab-tracker-core)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker-core)](https://crates.io/crates/gitlab-tracker-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.97+-orange.svg)](https://www.rust-lang.org/)

Shared domain library for [gitlab-tracker](../README.md).

This crate defines **all extension points** of the tracker — trait contracts, domain types,
and policy interfaces. It has **zero dependency on `ratatui`, `crossterm`, or any UI crate**,
so every public type can be used and tested in isolation.

Plugin crates (`gitlab-tracker-redmine`, future Jira/Linear integrations) depend only on
this crate — never on the binary crate.

---

## Public contracts

### `TrackerProvider` — tracker plugin interface

The single trait to implement when adding a new external tracker integration.

```rust
#[async_trait]
pub trait TrackerProvider: Send + Sync {
    /// Short human-readable name shown in the UI (e.g. "Redmine", "Jira").
    fn name(&self) -> &'static str;

    /// Detects a ticket ID from an MR title and/or description.
    /// Returns `None` when no reference is found.
    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String>;

    /// Fetches a ticket by ID. Returns `None` on any error (network, auth, not found).
    async fn fetch_ticket(&self, ticket_id: &str) -> Option<LinkedTicket>;

    /// Builds the direct URL to open the ticket in a browser.
    fn ticket_url(&self, ticket_id: &str) -> String;

    // Optional — all have default no-op implementations:
    fn label_colors(&self) -> LabelColorMaps { … }
    async fn fetch_activities(&self) -> Vec<Activity> { … }
    async fn fetch_time_entries(&self, ticket_id: &str) -> Vec<TimeEntry> { … }
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), TrackerError> { … }
}
```

The orchestrator receives a `Arc<dyn TrackerProvider>` — it never knows which concrete
provider is behind it. See [`gitlab-tracker-redmine`](../gitlab-tracker-redmine/README.md)
for a full implementation example and wiring instructions.

#### `LinkedTicket` — the only data type crossing the boundary

A flat, display-oriented struct. The orchestrator does not need to understand the tracker's
internal model — it only renders these fields.

| Field | Type | Description |
| :--- | :--- | :--- |
| `id` | `String` | Ticket identifier (e.g. `"1234"`, `"PROJ-42"`) |
| `subject` | `String` | Short title |
| `status` | `String` | Human-readable status label |
| `url` | `String` | Direct URL to open in a browser |
| `author` | `Option<String>` | Creator display name |
| `assignee` | `Option<String>` | Assignee display name |
| `time_estimate` | `Option<u32>` | Estimated time in seconds |
| `time_spent` | `Option<u32>` | Time already spent in seconds |
| `time_remaining` | `Option<u32>` | Remaining time (ETC) in seconds |
| `ticket_type` | `Option<String>` | Type label (e.g. `"Bug"`, `"Evolution"`) |
| `priority` | `Option<String>` | Priority label (e.g. `"Normal"`, `"High"`) |
| `target_version` | `Option<String>` | Target release/version |
| `start_date` | `Option<String>` | Start date (`YYYY-MM-DD`) |
| `done_ratio` | `Option<u32>` | Completion percentage (0–100) |
| `schema_version` | `u32` | Cache invalidation guard — see `LINKED_TICKET_SCHEMA_VERSION` |

> **Cache invalidation:** increment `LINKED_TICKET_SCHEMA_VERSION` whenever you add or
> remove fields from `LinkedTicket`. The orchestrator will automatically re-fetch any
> cached ticket whose `schema_version` is lower than the constant.

---

### `MrLifecycleEvent` + `MrEventPolicy` — reaction policy

Defines **what the application should do** when a MR transitions between states,
fully decoupled from the UI event loop.

#### `MrLifecycleEvent`

| Variant | When it fires |
| :--- | :--- |
| `Added` | User requested tracking a new MR |
| `Deleted` | User removed a MR from the list |
| `Refreshed` | A periodic refresh returned updated GitLab data |
| `Merged` | State transition to `merged` detected in a refresh |
| `Closed` | State transition to `closed` detected in a refresh |
| `FetchFailed` | A GitLab API call for this MR failed |

#### `MrEventPolicy` trait

```rust
pub trait MrEventPolicy: Send + Sync {
    fn needs_refetch(&self, event: &MrLifecycleEvent) -> bool;
    fn should_remove(&self, event: &MrLifecycleEvent) -> bool;
    fn should_notify(&self, event: &MrLifecycleEvent) -> bool;
    fn needs_persist(&self, event: &MrLifecycleEvent) -> bool;
}
```

#### Default policy (`DefaultMrEventPolicy`)

| Event | `needs_refetch` | `should_remove` | `should_notify` | `needs_persist` |
| :--- | :---: | :---: | :---: | :---: |
| `Added` | ✅ | ❌ | ❌ | ✅ |
| `Deleted` | ❌ | ✅ | ❌ | ✅ |
| `Refreshed` | ❌ | ❌ | ❌ | ✅ |
| `Merged` | ❌ | ❌ | ✅ | ✅ |
| `Closed` | ❌ | ❌ | ✅ | ✅ |
| `FetchFailed` | ❌ | ❌ | ✅ | ❌ |

To inject a custom policy (e.g. suppress all persists in demo mode):

```rust
app.event_policy = Arc::new(MyCustomPolicy);
```

No changes to `apply_event` are needed — the policy is the single place where
these rules live.

---

### `FilterDef` — filter picker extension point

Register a new filter without modifying any existing file:

```rust
inventory::submit!(FilterDef {
    id:              "my_filter",
    label:           "My filter",
    active_label:    "My filter",
    priority:        100,          // 0–99: built-in, 100–199: plugins, 200+: community
    needs_text_input: false,
    apply:           |mr, _query| { /* return true when MR should be visible */ },
});
```

Filters are collected at startup via `collect_all_filters()` and sorted by `priority`.

---

### `ColumnDef` — table column extension point

Register a new optional table column:

```rust
inventory::submit!(ColumnDef {
    id:       "my_column",
    label:    "My Column",
    priority: 100,
});
```

Columns are collected at startup via `collect_all_columns()` and sorted by `priority`.
The user toggles visibility via the `C` picker — the active set is persisted to `projects.toml`.

---

### `ShortcutBlock` — help popup extension point

Register a block of keyboard shortcuts to appear in the `?` help popup:

```rust
inventory::submit!(ShortcutBlock {
    title:    "My Plugin",
    priority: 100,
    entries:  ShortcutFactory(&|| vec![
        ShortcutEntry { key: "X", description: "Do something" },
    ]),
});
```

Blocks are collected via `collect_all_blocks()` and displayed in priority order.

---

## Priority conventions

All three extension points (`FilterDef`, `ColumnDef`, `ShortcutBlock`) share the same
priority band convention:

| Range | Owner |
| :--- | :--- |
| `0 – 99` | Built-in (shipped with `gitlab-tracker`) |
| `100 – 199` | First-party tracker plugins (Redmine, future Jira/Linear) |
| `200+` | Community / third-party plugins |
