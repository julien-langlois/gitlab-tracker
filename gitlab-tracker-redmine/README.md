# gitlab-tracker-redmine

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

  Notifications are suppressed during the initial sync (see [`gitlab-tracker-notify`](../gitlab-tracker-notify/README.md)).

* **Time Log view (`P` × 2):** press `P` twice (when the Tracker pane is focused) to reach the **Time Log** view:
  * A progress bar comparing time spent vs. the ticket's estimate.
  * The full list of time entries (date, user, activity, duration, comment) fetched live from Redmine.
  * Navigating between MRs with `↑`/`↓` while on this view automatically refreshes the entries for the newly selected ticket.

* **Log time (`L`):** open a popup to submit a new time entry directly to Redmine — select the activity category, enter a duration (e.g. `1h30`, `90m`, `1.5h`), optionally add a comment, and confirm with `Enter`.
* **Tracker column** in the main table (toggleable via `C`) — shows ticket ID, status, and spent/estimated time at a glance.

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

On first launch with the feature enabled and no `[project.tracker]` section configured, the app interactively prompts for a Redmine URL:

```text
🌐 No Redmine URL found for this project.
   Leave empty to disable Redmine integration.
Redmine URL: https://redmine.my-company.com
```

Leaving the prompt **empty** silently disables the integration for that project — no error, no impact on the rest of the dashboard. You can also pre-configure via environment variable to skip the prompt entirely:

```env
REDMINE_URL=https://redmine.my-company.com
```

> **Upgrading from a previous version?** If you have a `redmine.yaml` file from an older release, the app performs a **silent one-time migration** on first startup: all settings are read from `redmine.yaml`, written into the `[project.tracker]` section of `projects.toml`, and the old file is no longer used.

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

When you log time via the `L` popup, the app automatically recomputes the **Estimate to Complete (ETC)** on the linked Redmine ticket and writes it back.

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
3. Interactive CLI prompt → saved to OS Keyring under that URL's key
```

Because the keyring entry is keyed by URL, switching between two Redmine instances never clobbers the other's token.

---

## Adding a New Tracker Plugin

The tracker system is designed to be extended without modifying any existing file except `main.rs`. To add a different tracker (Jira, Linear, …):

1. Create a new crate (e.g. `gitlab-tracker-jira`) and implement the `TrackerProvider` trait from `gitlab-tracker-core`.
2. In `projects.toml`, users set `provider = "jira"` in their `[project.tracker]` section — `storage.rs` and `ProjectEntry` require **no changes**.
3. Add a `#[cfg(feature = "jira")]` branch in `gitlab-tracker/src/main.rs` that reads `project.tracker`, deserialises the `extra` fields, and wires up the provider.

### Required methods

```rust
#[async_trait]
impl TrackerProvider for MyProvider {
    fn name(&self) -> &'static str { "My Tracker" }

    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String> { ... }

    async fn fetch_ticket(&self, ticket_id: &str) -> Option<LinkedTicket> { ... }

    fn ticket_url(&self, ticket_id: &str) -> String { ... }
}
```

### Optional overrides (all have default no-op implementations)

```rust
    // Badge colours for Type and Priority labels — read from your config.
    // Return raw (bg, fg) string pairs; the orchestrator converts them to ratatui::Color.
    // Omit to use the hard-coded fallback (dark_gray / white).
    fn label_colors(&self) -> LabelColorMaps { ... }

    // Time-tracking support:
    async fn fetch_activities(&self) -> Vec<Activity> { ... }
    async fn fetch_time_entries(&self, ticket_id: &str) -> Vec<TimeEntry> { ... }
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), String> { ... }
```

### Wiring in `main.rs`

Add a `#[cfg(feature = "my-tracker")]` block that reads `project.tracker`, checks `provider`, deserialises `extra` into your config struct, then instantiates your provider:

```rust
#[cfg(feature = "my-tracker")]
let my_tracker_provider: Option<app::TrackerHandle> = {
    let tracker_cfg = project.tracker.as_ref()
        .filter(|t| t.provider.eq_ignore_ascii_case("my-tracker"));

    if let Some(cfg) = tracker_cfg {
        let mut my_cfg: MyTrackerConfig = cfg.extra.clone().try_into().unwrap_or_default();
        my_cfg.url = cfg.url.clone();
        my_tracker_keyring::get_or_prompt_token(&cfg.url).map(|tok| {
            let provider = MyTrackerProvider::new(my_cfg, tok.to_string());
            Arc::new(provider) as Arc<dyn gitlab_tracker_core::TrackerProvider>
        })
    } else {
        None
    }
};
```

The `LabelColorMaps` flow is the same for every provider:

```text
projects.toml  [project.tracker.*_colors]
  └─ label_colors() in your provider   ← String pairs, no ratatui dependency
       └─ build_tracker_colors()        ← converts to ratatui::Color (orchestrator only)
            └─ TrackerLabelColors       ← passed to the Inspector renderer
```
