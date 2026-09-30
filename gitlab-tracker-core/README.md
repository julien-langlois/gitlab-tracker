# gitlab-tracker-core

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker-core)](https://crates.io/crates/gitlab-tracker-core)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker-core)](https://crates.io/crates/gitlab-tracker-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.89+-orange.svg)](https://www.rust-lang.org/)

Shared domain library for [gitlab-tracker](../README.md).

This crate defines **all extension points** of the tracker — trait contracts, registries and
domain types. It has **zero dependency on `ratatui`, `crossterm`, or any UI crate**,
so every public type can be used and tested in isolation.

| Module | Content |
| :--- | :--- |
| `provider` | `TrackerProvider`, `TicketTransitionProvider`, `TrackerError`, `LinkedTicket`, `TicketChange`, time-tracking types |
| `domain` | `GitlabMrState`, `MergeabilityStatus`, `PipelineState`, `Requirement` — typed enums shared by the app, the plugins and the stats crate |
| `filters` | `FilterDef` + `MrSnapshot` + registry |
| `columns` | `ColumnDef` + registry |
| `shortcuts` | `ShortcutBlock` / `ShortcutEntry` / `ShortcutFactory` + registry |
| `settings` | `ProjectSettingDef` / `ProjectSettingFactory` + registry (settings dashboard `,`) |
| `secrets` *(feature `secrets`)* | `resolve_secret`: env var → OS keyring → hidden prompt saved to the keyring |

### Feature `secrets`

Off by default, so a plugin that needs no credential does not pull `keyring` / `rpassword`.
The binary and `gitlab-tracker-redmine` enable it and share one token lookup chain:

```rust
use gitlab_tracker_core::secrets::{resolve_secret, SecretSource};

let token: Option<zeroize::Zeroizing<String>> = resolve_secret(&SecretSource {
    env_var: "MY_TRACKER_TOKEN",
    keyring_service: "gitlab-tracker-my-tracker",
    keyring_account: &format!("my_token::{url}"),   // one slot per instance
    label: "My Tracker API token",
    prompt_hint: Some("Leave empty to disable the integration."),
});
```

`None` means the prompt was left empty (or could not be read); the token is zeroized on drop.

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

    /// Fetches a ticket by ID. Typed error: Network / Auth / NotFound / Unsupported / Other.
    async fn fetch_ticket(&self, ticket_id: &str) -> Result<LinkedTicket, TrackerError>;

    /// Builds the direct URL to open the ticket in a browser.
    fn ticket_url(&self, ticket_id: &str) -> String;

    // Optional — all have default implementations (empty / 1 / Err(Unsupported)):
    fn label_colors(&self) -> LabelColorMaps { … }
    fn estimate_calls_per_ticket(&self) -> usize { … }
    async fn fetch_activities(&self) -> Result<Vec<Activity>, TrackerError> { … }
    async fn fetch_time_entries(&self, ticket_id: &str) -> Result<Vec<TimeEntry>, TrackerError> { … }
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), TrackerError> { … }
}
```

The orchestrator receives a `Arc<dyn TrackerProvider>` — it never knows which concrete
provider is behind it. See [`gitlab-tracker-redmine`](../gitlab-tracker-redmine/README.md)
for a full implementation example and wiring instructions.

### `TicketTransitionProvider` — optional workflow automation capability

Implement this trait only when a provider can mutate ticket workflow/status state. It is intentionally separate from `TrackerProvider`: read-only providers can remain simple, while providers with workflow support expose the extra capability explicitly.

```rust
#[async_trait]
pub trait TicketTransitionProvider: Send + Sync {
    /// Lists provider-specific statuses/transitions that users can configure.
    async fn fetch_transition_targets(&self) -> Result<Vec<TicketTransitionTarget>, TrackerError>;

    /// Transitions one ticket to the provider-native target ID.
    async fn transition_ticket_status(
        &self,
        ticket_id: &str,
        target_id: &str,
    ) -> Result<(), TrackerError>;
}
```

#### `TicketTransitionTarget`

`TicketTransitionTarget` is the display/configuration shape returned by `fetch_transition_targets()`:

| Field | Type | Description |
| :--- | :--- | :--- |
| `id` | `String` | Provider-native status or transition identifier. Redmine returns numeric IDs as strings; other trackers may return UUIDs, slugs, or transition keys. |
| `label` | `String` | Human-readable label shown to the user, e.g. `"Resolved"`, `"Done"`, or `"Deployed to production"`. |

The binary uses this contract for the `tracker-statuses` CLI command. With Redmine enabled, `RedmineProvider::fetch_transition_targets()` calls `GET /issue_statuses.json` and prints the returned IDs and labels so users can configure mappings without guessing IDs.

#### Implementation guidance

* Keep `fetch_transition_targets()` side-effect free: it should discover available targets, not mutate tickets.
* Validate `target_id` inside `transition_ticket_status()` and return `TrackerError` instead of panicking on invalid configuration.
* Treat workflow refusal as a normal provider error: trackers may reject a transition depending on the current ticket status, tracker type, role permissions, or required fields.
* Do not hardcode labels such as `"Closed"` or `"Resolved"` in shared code. Labels are instance-specific and may be translated; mappings should use the provider-native `id`.

---

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
| `tracker_type` | `Option<String>` | Type label (e.g. `"Bug"`, `"Evolution"`) |
| `priority` | `Option<String>` | Priority label (e.g. `"Normal"`, `"High"`) |
| `version` | `Option<String>` | Target release/version |
| `start_date` | `Option<String>` | Start date (`YYYY-MM-DD`) |
| `done_ratio` | `Option<u32>` | Completion percentage (0–100) |
| `schema_version` | `u32` | Cache invalidation guard — see `LINKED_TICKET_SCHEMA_VERSION` |

> **Cache invalidation:** increment `LINKED_TICKET_SCHEMA_VERSION` whenever you add or
> remove fields from `LinkedTicket`. The orchestrator will automatically re-fetch any
> cached ticket whose `schema_version` is lower than the constant.

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
    requires:        None,         // or Some(Requirement::Tracker / GitlabUsername)
    apply:           |mr, _query| mr.state == GitlabMrState::Opened,
});
```

Filters are collected at startup via `collect_all_filters()` and sorted by `priority`.
`apply` receives an `MrSnapshot` — a borrowed, UI-free view of the MR whose `state`,
`mergeability` and `pipeline_status` are the typed enums of `domain`, so a typo in a
variant is a compile error rather than a silently empty filter. An entry whose
`requires` is not met at runtime is hidden from the picker.

---

### `ColumnDef` — table column extension point

Register a new optional table column:

```rust
inventory::submit!(ColumnDef {
    id:              "my_column",
    label:           "My Column",
    default_visible: false,
    priority:        100,
    requires:        Some(Requirement::Tracker),  // hidden from the picker otherwise
});
```

Columns are collected at startup via `collect_all_columns()` and sorted by `priority`.
The user toggles visibility via the `c` picker — the active set is persisted to `projects.toml`.

---

### `ShortcutBlock` — help popup extension point

Register a block of keyboard shortcuts to appear in the `?` help popup:

```rust
fn my_shortcuts() -> ShortcutBlock {
    ShortcutBlock {
        section:  "My Plugin",
        priority: 100,
        entries:  &[ShortcutEntry {
            key:         "x",
            description: "Do something",
            status_hint: None,          // Some("[x]: Thing") for a status-bar slot
        }],
    }
}
inventory::submit!(ShortcutFactory(my_shortcuts));
```

Blocks are collected via `collect_all_blocks()` and displayed in priority order.

---

### `ProjectSettingDef` — settings dashboard extension point

A crate can expose its own settings in the `,` dashboard. `read` / `write` work on the
active `[[project]]` TOML table, so the crate owns its nested section (e.g. `[project.stats]`):

```rust
fn my_setting() -> ProjectSettingDef {
    ProjectSettingDef {
        id: "my.enabled", section: "My Plugin", label: "Enabled",
        help: "Turns the plugin on for this project", priority: 100,
        kind: ProjectSettingKind::Bool,
        default_value: ProjectSettingValue::Bool(true),
        read:  |t| ProjectSettingValue::Bool(t.get("my_enabled").and_then(|v| v.as_bool()).unwrap_or(true)),
        write: |t, v| if let ProjectSettingValue::Bool(b) = v { t.insert("my_enabled".into(), b.into()); },
    }
}
inventory::submit!(ProjectSettingFactory(my_setting));
```

---

## Priority conventions

All extension points (`FilterDef`, `ColumnDef`, `ShortcutBlock`, `ProjectSettingDef`) share the same
priority band convention:

| Range | Owner |
| :--- | :--- |
| `0 – 99` | Built-in (shipped with `gitlab-tracker`) |
| `100 – 199` | First-party tracker plugins (Redmine, future Jira/Linear) |
| `200+` | Community / third-party plugins |
