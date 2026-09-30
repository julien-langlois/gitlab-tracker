# gitlab-tracker-redmine

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker-redmine)](https://crates.io/crates/gitlab-tracker-redmine)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker-redmine)](https://crates.io/crates/gitlab-tracker-redmine)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.89+-orange.svg)](https://www.rust-lang.org/)

Optional Redmine integration plugin for [gitlab-tracker](../README.md).

Implements the `TrackerProvider` trait from `gitlab-tracker-core` to detect Redmine ticket references in MR titles and descriptions, and enrich the TUI with ticket details and time tracking.

---

## Features

* **Linked ticket display** in the Inspector's MR Info view — the following fields are shown when available:

  | Field | Source (`GET /issues/{id}.json`) |
  | :--- | :--- |
  | Ticket ID + Subject | `id`, `subject` |
  | Type badge | `tracker.name` (e.g. "Bug", "Evolution") |
  | Priority badge | `priority.name` (e.g. "Normal", "High") |
  | Status | `status.name` |
  | Author / Assignee | `author.name`, `assigned_to.name` |
  | Target version | `fixed_version.name` |
  | Start date | `start_date` |
  | Progress bar | `done_ratio` (0–100 %) |
  | Estimate / Spent / Remaining | `estimated_hours`, `spent_hours`, `remaining_hours` |
  | Direct URL | built from `url` + ticket ID |

  Type and Priority are rendered as **coloured badges** — colours are fully configurable in `projects.toml` (see [Configuration](#configuration) below).

* **Multi-tenant:** each GitLab project in `projects.toml` can point to a **different** Redmine instance. The API token for each instance is stored separately in the OS keyring, keyed by the Redmine URL — no token collision between tenants.

* **Change notifications:** when a ticket field changes between two refresh cycles, a native desktop notification is raised with the **before → after** values. Tracked fields:

  | Field | Notification icon |
  | :--- | :--- |
  | Priority | ⚠️ `dialog-warning` |
  | Status | ℹ️ `dialog-information` |
  | Assignee | ℹ️ `dialog-information` |
  | Target version | ℹ️ `dialog-information` |
  | Progress (`done_ratio`) | ℹ️ `dialog-information` — fires on both increase **and** decrease |

  Notifications are suppressed during the initial sync (see [How Desktop Notifications Work](../README.md#-how-desktop-notifications-work)).

* **Time Log view (`p` on the Tracker pane):** focus the Tracker pane (`Tab` or `t`), then press `p` to toggle Ticket Info ↔ **Time Log**:
  * A progress bar comparing time spent vs. the ticket's estimate.
  * The full list of time entries (date, user, activity, duration, comment), read page by page from Redmine (`limit=100&offset=…`) so tickets with many entries are complete.
  * Entries are cached per ticket: navigating between MRs shows the cached list instantly; the cache is refreshed on each refresh cycle, on `r`, and after logging time.

* **Log time (`l`):** open a popup to submit a new time entry directly to Redmine — select the activity category, enter a duration (e.g. `1h30`, `90m`, `1.5h`), optionally add a comment, and confirm with `Enter`.
* **Tracker column** in the main table (toggleable via `c`, only offered when the integration is active) — shows ticket ID, status, and spent/estimated time at a glance.
* **Status discovery CLI:** list Redmine issue status IDs from the terminal via `gitlab-tracker tracker-statuses`, backed by Redmine's `GET /issue_statuses.json` endpoint. This helps configure GitLab-to-Redmine status transition mappings without guessing numeric IDs.
* **Safe automatic status transitions:** when configured, a GitLab MR transition from `Opened` to `Merged` or `Closed` can update the linked Redmine issue status. The update is guarded by an optimistic concurrency check: Redmine is changed only if its live status still matches the last status known locally, preventing accidental overwrite of manual workflow changes.
* **Transition notifications:** after a successful automatic Redmine status transition, the desktop notification plugin emits an "Open ticket" notification showing the old and new statuses.

---

## Enabling at Build Time

The `redmine` feature flag must be explicitly passed when building or installing:

```bash
# Build from source
cargo build --release --features redmine

# Install from source
cargo install --path gitlab-tracker --features redmine

# Install from crates.io
cargo install gitlab-tracker --features redmine
```

> Without the flag, this crate is not compiled and there is zero runtime overhead.

---

## Configuration

Configuration lives directly inside `projects.toml` under a `[project.tracker]` section — **no separate file needed**. Each `[[project]]` entry can point to a different Redmine instance.

The integration is **opt-in**: without a `[project.tracker]` section whose `provider = "redmine"`, it stays silently inactive (no prompt). The `REDMINE_URL` environment variable overrides the configured `url`; a section with an empty URL disables the integration with a warning in the log.

Invalid provider-specific fields (e.g. a malformed `status_transitions` table) are logged and replaced by their defaults instead of aborting startup.

> `redmine.yaml` from very old releases is no longer read: move its content to `[project.tracker]` as shown below.

### `projects.toml` — full Redmine reference

The Redmine section uses `provider = "redmine"` as its discriminant. All other fields are forwarded to the plugin.

```toml
[[project]]
name       = "My Company — Backend"
gitlab_url = "https://gitlab.my-company.com"
project_id = "12345678"
active     = true

[project.tracker]
# Discriminant — selects the Redmine plugin.
provider = "redmine"

# Base URL of your Redmine instance. No trailing slash.
# Can also be set via the REDMINE_URL environment variable.
url = "https://redmine.my-company.com"

# Regex patterns used to detect ticket IDs in MR titles and descriptions.
# Each pattern must expose the numeric ID in capture group 1.
# The first match wins; patterns are tried in order.
# Defaults shown below — omit the key to keep them.
ticket_patterns = [
  "#(\\d+)",                                         # plain #1234
  "(?i)(?:refs|fixes|closes|resolves)\\s+#(\\d+)",  # refs #1234, fixes #1234, …
  "/issues/(\\d+)",                                  # full Redmine URL in description
]

# Badge colours for the "Type" field (tracker.name in the Redmine API).
# Keys are matched CASE-INSENSITIVELY. Use "*" as a catch-all fallback.
# Accepted colour values: named (red, cyan, dark_gray, …) or hex (#ff6600).
[project.tracker.tracker_type_colors]
"Bug"       = { bg = "red",       fg = "white" }
"Evolution" = { bg = "cyan",      fg = "black" }
"Support"   = { bg = "yellow",    fg = "black" }
"*"         = { bg = "dark_gray", fg = "white" }

# Badge colours for the "Priority" field (priority.name in the Redmine API).
[project.tracker.priority_colors]
"Low"    = { bg = "dark_gray", fg = "white" }
"Normal" = { bg = "dark_gray", fg = "white" }
"High"   = { bg = "yellow",    fg = "black" }
"Urgent" = { bg = "red",       fg = "white" }
"*"      = { bg = "dark_gray", fg = "white" }

# Optional workflow automation. Values are Redmine status IDs discovered with:
#   gitlab-tracker --project "My Company — Backend" tracker-statuses
# Each key is optional: configure only merged, only closed, both, or neither.
[project.tracker.status_transitions.gitlab_state]
merged = "3"
closed = "5"
```

#### Multi-tenant example — two projects, two Redmine instances

```toml
[[project]]
name       = "Client A — Backend"
gitlab_url = "https://gitlab.com"
project_id = "12345678"
active     = true

[project.tracker]
provider = "redmine"
url      = "https://redmine-a.example.com"

[[project]]
name       = "Client B — Backend"
gitlab_url = "https://gitlab.my-company.com"
project_id = "87654321"

[project.tracker]
provider = "redmine"
url      = "https://redmine-b.example.com"

[project.tracker.tracker_type_colors]
"Bug" = { bg = "red", fg = "white" }
```

Each instance has its own token stored independently in the OS keyring (keyed by URL). Switching the active project automatically uses the correct credentials.

#### Field reference

| Field | Required | Description |
| :--- | :--- | :--- |
| `provider` | ✅ | Must be `"redmine"` to activate this plugin |
| `url` | ✅ | Base URL of your Redmine instance (no trailing slash) |
| `ticket_patterns` | ❌ | Regex list to detect ticket IDs — capture group 1 must match the numeric ID. Defaults to `#1234`, `refs #1234`, and full URL patterns |
| `tracker_type_colors` | ❌ | Badge colour map for the `tracker.name` field. Keys are case-insensitive; `"*"` is a catch-all. Omit to use the default (dark_gray / white) |
| `priority_colors` | ❌ | Badge colour map for the `priority.name` field. Same rules as above |
| `status_transitions.gitlab_state.merged` | ❌ | Redmine `status_id` applied when a tracked MR transitions from `Opened` to `Merged` |
| `status_transitions.gitlab_state.closed` | ❌ | Redmine `status_id` applied when a tracked MR transitions from `Opened` to `Closed` |

### Automatic Redmine status transitions

Status transitions are opt-in and configured per project. You can map either `merged`, `closed`, both, or neither:

```toml
[project.tracker.status_transitions.gitlab_state]
merged = "3"
closed = "5"
```

When a tracked MR transitions from `Opened` to `Merged` or `Closed`, the app attempts to transition the linked Redmine issue through `RedmineProvider::transition_ticket_status()`, which delegates to `PUT /issues/{id}.json` with `issue.status_id`.

To avoid overwriting manual workflow changes, the orchestrator performs an optimistic concurrency check before writing:

1. keep the last locally known Redmine status from the cached `LinkedTicket`;
2. fetch the live Redmine issue before transitioning;
3. transition only when the live Redmine status still equals the locally known status;
4. if the live status differs, skip the transition and update the local cache instead.

After a successful transition, the issue is fetched again and the notification plugin emits a desktop notification showing the old and new statuses. Clicking it opens the Redmine ticket.

### Discover Redmine status IDs from the CLI

Redmine issue statuses are configured per instance and exposed to the API as numeric IDs. The displayed labels (`New`, `Resolved`, `Closed`, `Deployed in production`, etc.) are not enough for workflow automation because Redmine expects a `status_id` when updating an issue.

Use the `tracker-statuses` CLI command to print the full status list for the selected project:

```bash
# Development build
gitlab-tracker --project "Client A — Backend" tracker-statuses

# From the workspace, when testing the feature locally
cargo run -p gitlab-tracker --features redmine -- --project "Client A — Backend" tracker-statuses
```

The `--project` selector accepts:

* the project `name` from `projects.toml`;
* the GitLab `project_id`;
* the 1-based project index in `projects.toml`.

Example output:

```text
Tracker statuses for provider 'redmine':
     1  New
     2  In Progress
     3  Resolved
     5  Closed
```

Under the hood, the command uses:

* `TicketTransitionProvider::fetch_transition_targets()` from `gitlab-tracker-core`;
* `RedmineProvider::fetch_transition_targets()` from this crate;
* `GET /issue_statuses.json` on the configured Redmine instance.

The Redmine token is resolved the same way as the TUI: `REDMINE_TOKEN`, then OS keyring keyed by Redmine URL, then interactive prompt.

Redmine errors are typed (`TrackerError` from `gitlab-tracker-core`): HTTP 401/403 become `Auth`, 404 becomes `NotFound`, transport failures `Network`. Any failed ticket fetch is logged and keeps the previously cached ticket instead of wiping it; a failed time-entries fetch is shown as an error in the Time Log view.

> **How to discover your Redmine's label values**
>
> Label names (tracker types, priorities) are instance-specific and may be in any language. Run these commands against any existing issue to see what your instance returns:
>
> ```bash
> # Tracker type and priority of issue #1234
> curl -s -H "X-Redmine-API-Key: YOUR_TOKEN" \
>   "https://your-redmine.com/issues/1234.json" \
>   | jq '.issue | {tracker: .tracker.name, priority: .priority.name}'
>
> # All priorities defined in your instance
> curl -s -H "X-Redmine-API-Key: YOUR_TOKEN" \
>   "https://your-redmine.com/enumerations/issue_priorities.json" \
>   | jq '[.issue_priorities[].name]'
> ```

---

## Estimate to Complete (ETC) — automatic update on time entry submission

When you log time via the `l` popup, the app automatically recomputes the **Estimate to Complete (ETC)** on the linked Redmine ticket and writes it back.

> **ETC** (Estimate to Complete) is the standard project management term for the remaining effort needed to finish a task. It is sometimes labelled _Remaining time_ or _Reste à faire_ (RAF) in French Redmine instances.

### How it works

The computation uses fields returned directly by the Redmine issue API (`GET /issues/{id}.json`) — no admin access required:

| Priority | Field used | Formula | Available on |
| :--- | :--- | :--- | :--- |
| 1 (preferred) | `remaining_hours` | `remaining_hours − new_hours` | Redmine instances with a budget/planning plugin |
| 2 (fallback) | `estimated_hours` + `spent_hours` | `estimated_hours − spent_hours − new_hours` | All standard Redmine instances |

Both strategies clamp the result to `0.0` — ETC cannot be negative.

The ETC and budget are submitted **directly on the time entry** (`POST /time_entries.json`), which is how the Redmine Budget plugin tracks them. The `remaining_hours` field on the issue is then automatically recalculated by the plugin — no separate write to the issue is needed. If your Redmine instance does not expose these fields, they are simply omitted from the payload — the time entry is still created successfully.

### No configuration needed

The ETC update is **fully automatic** and requires no changes to `projects.toml`. It activates whenever the issue returns usable time fields, and degrades gracefully otherwise.

**Verify your instance exposes the fields** by running:

```bash
curl -s -H "X-Redmine-API-Key: YOUR_TOKEN" \
  "https://your-redmine.com/issues/YOUR_ISSUE_ID.json" \
  | jq '.issue | {estimated_hours, spent_hours, remaining_hours}'
```

If `remaining_hours` appears in the output, Strategy 1 is used. If only `estimated_hours` and `spent_hours` appear, Strategy 2 (fallback) is used.

---

## API Token

The Redmine personal API token follows the same secure lookup chain as the GitLab token — it is **never stored in plain text**:

```text
1. REDMINE_TOKEN environment variable (if set — shared across all instances, useful for CI)
2. Native OS Keyring — keyed by Redmine URL (per-instance, multi-tenant safe)
3. Hidden interactive prompt → saved to OS Keyring under that URL's key
   (leave it empty to disable Redmine for this project and session)
```

Because the keyring entry is keyed by URL, switching between two Redmine instances never clobbers the other's token. The lookup chain is the shared `gitlab_tracker_core::secrets::resolve_secret` helper (feature `secrets` of `gitlab-tracker-core`), also used for the GitLab token; the token is held in a `Zeroizing<String>` and wiped from memory on drop.

---

## Adding a New Tracker Plugin

The tracker system is designed to be extended without modifying any existing file except `main.rs` and `cli.rs` (one branch each). To add a different tracker (Jira, Linear, …):

1. Create a new crate (e.g. `gitlab-tracker-jira`) and implement the `TrackerProvider` trait from `gitlab-tracker-core`.
2. In `projects.toml`, users set `provider = "jira"` in their `[project.tracker]` section — `storage.rs` and `ProjectEntry` require **no changes**.
3. Expose a constructor from the `[project.tracker]` section in your crate (see below), and add a `#[cfg(feature = "jira")]` branch in `gitlab-tracker/src/main.rs` (and `cli.rs` for status discovery) that calls it.

### Required methods

```rust
#[async_trait]
impl TrackerProvider for MyProvider {
    fn name(&self) -> &'static str { "My Tracker" }

    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String> { ... }

    // Typed errors: NotFound / Auth / Network / Unsupported / Other.
    async fn fetch_ticket(&self, ticket_id: &str) -> Result<LinkedTicket, TrackerError> { ... }

    fn ticket_url(&self, ticket_id: &str) -> String { ... }
}
```

### Optional overrides (all have default no-op implementations)

```rust
    // Badge colours for Type and Priority labels — read from your config.
    // Return raw (bg, fg) string pairs; the orchestrator converts them to ratatui::Color.
    // Omit to use the hard-coded fallback (dark_gray / white).
    fn label_colors(&self) -> LabelColorMaps { ... }

    // Number of HTTP calls per fetch_ticket, for the API-call counter (default 1).
    fn estimate_calls_per_ticket(&self) -> usize { ... }

    // Time-tracking support (defaults: empty lists, log_time → Err(Unsupported)):
    async fn fetch_activities(&self) -> Result<Vec<Activity>, TrackerError> { ... }
    async fn fetch_time_entries(&self, ticket_id: &str) -> Result<Vec<TimeEntry>, TrackerError> { ... }
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), TrackerError> { ... }
```

Status automation is a separate, optional trait (`TicketTransitionProvider`) so read-only providers do not have to implement it:

```rust
#[async_trait]
impl TicketTransitionProvider for MyProvider {
    async fn fetch_transition_targets(&self) -> Result<Vec<TicketTransitionTarget>, TrackerError> { ... }
    async fn transition_ticket_status(&self, ticket_id: &str, target_id: &str) -> Result<(), TrackerError> { ... }
}
```

### Wiring in `main.rs`

Keep the setup (config parsing, token lookup, construction) **in your crate**, like
`RedmineProvider::from_tracker_section(url, extra) -> Result<Self, SetupError>`: the TUI and
the CLI commands then share it, and `main.rs` only maps the provider name to the crate.
Token lookup may hit the OS keyring and a stdin prompt, which block: call it through
`spawn_blocking`.

```rust
#[cfg(feature = "my-tracker")]
let my_tracker_provider: Option<app::TrackerHandle> = match project
    .tracker
    .as_ref()
    .filter(|t| t.provider.eq_ignore_ascii_case("my-tracker"))
{
    Some(cfg) => {
        let (url, extra) = (cfg.url.clone(), cfg.extra.clone());
        match tokio::task::spawn_blocking(move || MyTrackerProvider::from_tracker_section(&url, extra)).await? {
            Ok(provider) => Some(Arc::new(provider) as Arc<dyn gitlab_tracker_core::TrackerProvider>),
            Err(e) => {
                tracing::warn!(error = %e, "My tracker integration disabled");
                None
            }
        }
    }
    None => None,
};
```

The `LabelColorMaps` flow is the same for every provider:

```text
projects.toml  [project.tracker.*_colors]
  └─ label_colors() in your provider   ← String pairs, no ratatui dependency
       └─ build_tracker_colors()        ← converts to ratatui::Color (orchestrator only)
            └─ TrackerLabelColors       ← passed to the Inspector renderer
```
