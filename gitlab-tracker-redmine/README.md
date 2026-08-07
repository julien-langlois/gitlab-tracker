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
  | Direct URL | built from `redmine_url` + ticket ID |

  Type and Priority are rendered as **coloured badges** — colours are fully configurable in `redmine.yaml` (see [Configuration](#configuration) below).

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

On first launch with the feature enabled, the app interactively prompts for your Redmine URL:

```text
🌐 No Redmine URL found in config or environment.
   Leave empty to disable Redmine integration.
Redmine URL: https://redmine.my-company.com
```

Leaving the prompt **empty** silently disables the integration — no error, no impact on the rest of the dashboard.

The configuration is persisted as a YAML file:

* **Linux:** `~/.config/gitlab-tracker/gitlab-tracker/redmine.yaml`
* **macOS:** `~/Library/Application Support/gitlab-tracker/gitlab-tracker/redmine.yaml`
* **Windows:** `C:\Users\<User>\AppData\Roaming\gitlab-tracker\gitlab-tracker\redmine.yaml`

You can also pre-configure via environment variable to skip the prompt entirely:

```env
REDMINE_URL=https://redmine.my-company.com
```

### `redmine.yaml` — full reference

```yaml
# ── Connection ────────────────────────────────────────────────────────────────

# Base URL of your Redmine instance. No trailing slash.
# Can also be set via the REDMINE_URL environment variable.
redmine_url: "https://redmine.my-company.com"

# ── Ticket detection ──────────────────────────────────────────────────────────

# Regex patterns used to detect ticket IDs in MR titles and descriptions.
# Each pattern must expose the numeric ID in capture group 1.
# The first match wins; patterns are tried in order.
ticket_patterns:
  - "#(\\d+)"                                        # plain #1234
  - "(?i)(?:refs|fixes|closes|resolves)\\s+#(\\d+)" # refs #1234, fixes #1234, …
  - "/issues/(\\d+)"                                 # full Redmine URL in description

# ── Badge colours ─────────────────────────────────────────────────────────────
#
# Colour both the "Type" and "Priority" badges displayed in the Inspector panel.
# Keys are matched CASE-INSENSITIVELY against the value returned by the Redmine API.
# Use "*" as a catch-all fallback for any label not listed explicitly.
#
# Accepted colour values:
#   Named:  red, green, yellow, blue, magenta, cyan, white, black, dark_gray, light_gray
#   Hex:    #rrggbb  (e.g. #ff6600)

# tracker_type_colors maps the "tracker.name" field (ticket category in Redmine).
# Discover your instance's values with:
#   curl -s -H "X-Redmine-API-Key: YOUR_TOKEN" \
#     "https://your-redmine.com/issues/ANY_ID.json" | jq '.issue.tracker'
tracker_type_colors:
  "Bug":       { bg: "red",      fg: "white" }
  "Evolution": { bg: "cyan",     fg: "black" }
  "Support":   { bg: "yellow",   fg: "black" }
  "*":         { bg: "dark_gray", fg: "white" } # catch-all for unlisted types

# priority_colors maps the "priority.name" field.
# Discover your instance's values with:
#   curl -s -H "X-Redmine-API-Key: YOUR_TOKEN" \
#     "https://your-redmine.com/issues/ANY_ID.json" | jq '.issue.priority'
priority_colors:
  "Low":   { bg: "dark_gray", fg: "white" }
  "Normal": { bg: "dark_gray", fg: "white" }
  "High":   { bg: "yellow",    fg: "black" }
  "Urgent": { bg: "red",       fg: "white" }
  "*":       { bg: "dark_gray", fg: "white" } # catch-all for unlisted priorities
```

| Field | Required | Description |
| :--- | :--- | :--- |
| `redmine_url` | ✅ | Base URL of your Redmine instance (no trailing slash) |
| `ticket_patterns` | ✅ | Regex list to detect ticket IDs in MR titles/descriptions — capture group 1 must match the numeric ID |
| `tracker_type_colors` | ❌ | Badge colour map for the `tracker.name` field. Keys are case-insensitive; `"*"` is a catch-all fallback. Omit the section entirely to use the default (dark_gray / white) |
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

The ETC update is **fully automatic** and requires no changes to `redmine.yaml`. It activates whenever the issue returns usable time fields, and degrades gracefully otherwise.

**Verify your instance exposes the fields** by running:

> ```bash
> curl -s -H "X-Redmine-API-Key: YOUR_TOKEN" \
>   "https://your-redmine.com/issues/YOUR_ISSUE_ID.json" \
>   | jq '.issue | {estimated_hours, spent_hours, remaining_hours}'
> ```

If `remaining_hours` appears in the output, Strategy 1 is used. If only `estimated_hours` and `spent_hours` appear, Strategy 2 (fallback) is used.

---

## API Token

The Redmine personal API token follows the same secure lookup chain as the GitLab token — it is **never stored in plain text**:

```text
1. REDMINE_TOKEN environment variable (if set)
2. Native OS Keyring (GNOME Keyring / macOS Keychain / Windows Credential Manager)
3. Interactive CLI prompt → saved to OS Keyring
```

---

## Adding a New Tracker Plugin

To add a different tracker (Jira, Linear, …), create a new crate and implement the `TrackerProvider` trait from `gitlab-tracker-core`.

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
    // Badge colours for Type and Priority labels — read from your config file.
    // Return raw (bg, fg) string pairs; the orchestrator converts them to ratatui::Color.
    // Omit to use the hard-coded fallback (dark_gray / white).
    fn label_colors(&self) -> LabelColorMaps { ... }

    // Time-tracking support:
    async fn fetch_activities(&self) -> Vec<Activity> { ... }
    async fn fetch_time_entries(&self, ticket_id: &str) -> Vec<TimeEntry> { ... }
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), String> { ... }
```

### Wiring in `main.rs`

Add a `#[cfg(feature = "my-tracker")]` block in `gitlab-tracker/src/main.rs` — the `build_tracker_colors` helper is already generic over any `TrackerProvider`, so no further changes are needed in the orchestrator:

```rust
#[cfg(feature = "my-tracker")]
if let Some(ref provider) = my_tracker_provider {
    app.tracker_colors = build_tracker_colors(provider.as_ref());
    app.tracker = my_tracker_provider;
}
```

The `LabelColorMaps` flow:

```text
Your config file (YAML/JSON/…)
  └─ label_colors() in your provider   ← String pairs, no ratatui dependency
       └─ build_tracker_colors()        ← converts to ratatui::Color (orchestrator only)
            └─ TrackerLabelColors       ← passed to the Inspector renderer
```
