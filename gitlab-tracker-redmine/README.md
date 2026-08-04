# gitlab-tracker-redmine

Optional Redmine integration plugin for [gitlab-tracker](../README.md).

Implements the `TrackerProvider` trait from `gitlab-tracker-core` to detect Redmine ticket references in MR titles and descriptions, and enrich the TUI with ticket details and time tracking.

---

## Features

* **Linked ticket display:** subject, status, author, and assignee shown in the Inspector's MR Info view.
* **Time Log view (`P`):** press `P` twice to reach the **Time Log** view:
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

The generated `redmine.yaml` (edit to customise ticket detection patterns):

```yaml
redmine_url: "https://redmine.my-company.com"
ticket_patterns:
  - "#(\\d+)"
  - "(?i)(?:refs|fixes|closes|resolves)\\s+#(\\d+)"
  - "/issues/(\\d+)"
```

| Field | Description |
| :--- | :--- |
| `redmine_url` | Base URL of your Redmine instance (no trailing slash) |
| `ticket_patterns` | Regex patterns used to detect ticket IDs in MR titles/descriptions — capture group 1 must match the numeric ID |

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

To add a different tracker (Jira, Linear, …), create a new crate that implements the `TrackerProvider` trait from `gitlab-tracker-core`:

```rust
#[async_trait]
impl TrackerProvider for MyProvider {
    fn name(&self) -> &'static str { "My Tracker" }
    fn detect_ticket_id(&self, title: &str, description: &str) -> Option<String> { ... }
    async fn fetch_ticket(&self, ticket_id: &str) -> Option<LinkedTicket> { ... }
    fn ticket_url(&self, ticket_id: &str) -> String { ... }
    // Optional — override for time tracking support:
    async fn fetch_activities(&self) -> Vec<Activity> { ... }
    async fn fetch_time_entries(&self, ticket_id: &str) -> Vec<TimeEntry> { ... }
    async fn log_time(&self, ticket_id: &str, entry: TimeEntryRequest) -> Result<(), String> { ... }
}
```

Then wire it in `gitlab-tracker/src/main.rs` under a new feature flag — no other file in the orchestrator needs to change.
