# Configuration

Everything the app reads besides GitLab itself: `projects.toml`, the token, the
`.env` file and environment variables. Most project settings can also be changed
from the app's settings dashboard, without editing any file.

- [projects.toml](#projectstoml)
- [First run and the token](#first-run-and-the-token)
- [.env file](#env-file)
- [Which project is loaded, which setting wins](#which-project-is-loaded-which-setting-wins)
- [Load on your GitLab / Redmine](#load-on-your-gitlab--redmine)
- [Logs and build options](#logs-and-build-options)

## projects.toml

All settings — connection details, display preferences, branch lists, and label colours — live in a single TOML file per project:

* **Linux:** `~/.config/gitlab-tracker/projects.toml`
* **macOS:** `~/Library/Application Support/gitlab-tracker/projects.toml`
* **Windows:** `C:\Users\<User>\AppData\Roaming\gitlab-tracker\projects.toml`

The file supports **multiple projects** in a `[[project]]` array. The active project is the first entry with `active = true` (or the first entry overall when none is marked).

### Annotated example

```toml
[[project]]
name = "My Company — Backend"
gitlab_url = "https://gitlab.my-company.com"
project_id = "12345678"
active = true

# Branches whose pipeline status appears as columns in the MR table.
default_branches = ["main", "staging"]

# Branches currently tracked (managed by the app when you add or remove a branch).
tracked_branches = ["main", "develop", "staging"]

# Label prefixes shown as chips in the "Labels" table column.
table_label_prefixes = ["deploy::", "review::"]

# How often the MR list is refreshed from GitLab (in seconds).
# Editable from the in-app settings dashboard with [,].
refresh_interval_secs = 900

# Activity badge thresholds (in days) shown in the Context Inspector.
activity_recent_days = 2   # 🟢 Active if updated within N days
activity_stale_days  = 7   # 🔴 Stale if not updated for N days

# Review reminders (needs gitlab_username): notify when an MR has waited this many
# hours for your review (repeated every N hours), and when its author pushes after
# your last activity on it. 0 disables both. Default: 24. Editable with [,].
review_reminder_hours = 24

# Show the operational cockpit pane below the MR table when enough vertical space
# is available. Set to false to keep the dashboard table-only for this project.
# Default: true. Editable from the in-app settings dashboard with [,].
show_cockpit = true

# Hide merged MRs from the table to focus on what is left to do. They stay tracked
# (refreshed, notified, recorded in stats). Default: false. Toggle with [m] or [,].
hide_merged = false

# Operational cockpit thresholds. Omit this table to use the defaults below.
[project.cockpit_thresholds]
stale_days = 7                  # Count open MRs as stale after N days without updates
old_open_warning_days = 7        # Turn oldest-open age yellow at N days
old_open_alert_days = 14         # Turn oldest-open age red at N days
due_soon_days = 7                # Count milestones as due soon within N days
complex_score = 0.66             # Count MRs as complex from this difficulty score
many_commits = 10                # Count MRs with at least N commits as many-commit MRs
many_files = 20                  # Count MRs touching at least N files as many-file MRs
hot_threads = 10                 # Count MRs with at least N unresolved threads as hot threads
release_urgent_days = 3          # Urgent release-risk window in days
release_urgent_remaining = 2     # Remaining MR threshold for urgent release risk
release_soon_days = 7            # Soon release-risk window in days
release_soon_remaining = 5       # Remaining MR threshold for soon release risk
max_release_summaries = 3        # Maximum releases shown in the cockpit

# Automatically discover and track all newly created MRs at each refresh cycle.
# This includes MRs already merged between two cycles, so stats coverage remains
# complete. Independent of the stats feature — useful for any reviewer who wants
# the tool to self-populate. When stats are also enabled, discovered MRs are
# snapshotted automatically for team-wide throughput and cycle-time coverage.
# Default: false.
discover_new_mrs = true

# Tech-stack calibration for the review-difficulty score.
# Weighted formula: (additions + deletions) × 0.8 + files_changed × 0.2
[project.complexity_profile]
name            = "Drupal"
easy_threshold  = 300    # score below this → 🟢 Easy
hard_threshold  = 2000   # score above this → 🔴 Complex

# Which optional columns are visible in the MR table.
[project.visible_columns]
activity      = false
target_branch = false
labels        = false
milestone     = true
notes         = false
tracker_ticket = true
diff_stats    = true   # "Effort" column (🟢/🟡/🔴 chip badge based on diff size)
commits_behind  = false  # "Behind" column — commits the source branch is behind the target

# Label colour overrides — exact names or wildcard patterns (e.g. "deploy::*").
# Accepted colour values: named colours ("red", "cyan", "dark_gray", …) or hex codes ("#D32F2F").
[project.label_colors]
"bug"              = { bg = "red",      fg = "white" }
"fix"              = { bg = "red",      fg = "white" }
"deploy::*"        = { bg = "green",    fg = "black" }
"review::*"        = { bg = "cyan",     fg = "black" }
"review::approved" = { bg = "magenta",  fg = "white" }
"size::*"          = { bg = "dark_gray", fg = "white" }

# Stats settings — only relevant when the `stats` feature is compiled in.
[project.stats]
# Keep snapshots for 6 months, then purge automatically on startup. Default: 365.
retention_days = 180
# Sprint duration in weeks — controls the window used in throughput forecasts. Default: 2.
sprint_weeks = 3

# Signed commits — warn when a commit of an open MR is not signed, or not
# verified. Off by default. See docs/signed-commits.md for every option.
[project.signed_commits]
enforce = true
# exempt_bots = "renovate-bot bot@renovate.example"

# Add more projects below — only the one with `active = true` is loaded at startup.
# [[project]]
# name       = "My Company — Frontend"
# gitlab_url = "https://gitlab.my-company.com"
# project_id = "87654321"
# active     = false
```

> **Complexity profile presets:**
>
> | Tech stack | `easy_threshold` | `hard_threshold` | Rationale |
> | :--- | :--- | :--- | :--- |
> | **Drupal** | `300` | `2000` | Lots of YAML/config files — verbose but lightweight to review |
> | **Symfony / PHP** | `200` | `1200` | Denser business logic, typically smaller PRs |
> | **Java / Spring** | `100` | `600` | Highly logic-dense lines; verbosity adds review cost |
> | **TypeScript / React** | `150` | `900` | JSX inflates line counts but remains readable |
> | **Go** | `150` | `800` | Concise but each line carries weight |
> | **Generic** *(default)* | `200` | `1000` | Conservative baseline for mixed stacks |

> **Optional table columns** — all hidden by default. Enable them per project under `[project.visible_columns]`:
>
> | Key | Column shown |
> | :--- | :--- |
> | `activity` | **Activity** — 🟢 Active / 🟡 Slowing / 🔴 Stale badge |
> | `target_branch` | **Target** — the branch the MR merges into |
> | `labels` | **Labels** — filtered label chips (respects `table_label_prefixes`) |
> | `milestone` | **Milestone** — the associated milestone title |
> | `notes` | **Notes** — unresolved review threads (`💬 N` in yellow when non-zero) |
> | `diff_stats` | **Effort** — 🟢 / 🟡 / 🔴 chip badge calibrated to `complexity_profile` |
> | `commits_behind` | **Behind** — commits the source branch is behind the target (`✔ Up to date`, `N behind`, `RETRYING`, `SYNC FAILED`) |
> | `tracker_ticket` | **Tracker** — linked ticket ID, status and spent / estimated time (requires a tracker plugin) |

> **Activity badge thresholds** control the colour-coded indicator next to the `Updated` field in the Context Inspector:
>
> | Badge | Meaning | Condition |
> | :--- | :--- | :--- |
> | 🟢 Active | Updated recently | `elapsed days < activity_recent_days` |
> | 🟡 Slowing | Activity slowing down | between the two thresholds |
> | 🔴 Stale | No recent activity | `elapsed days ≥ activity_stale_days` |
> | ⬛ Unknown | Timestamp unavailable | — |

## First run and the token

Your GitLab personal access token is **never stored in plain text**.

On first launch, `gitlab-tracker` resolves each required value using the following priority order — prompting interactively only as a last resort:

   ```text
 ┌──────────────────────────────────────────────────────────────┐
 │                    SETTINGS LOOKUP ORDER                     │
 ├──────────────────────────────────────────────────────────────┤
 │ GITLAB_PROJECT_ID & GITLAB_URL                               │
 │   1. Environment variable / .env file                        │
 │   2. ~/.config/gitlab-tracker/projects.toml                  │
 │   3. Interactive CLI prompt → saved to projects.toml         │
 ├──────────────────────────────────────────────────────────────┤
 │ GITLAB_TOKEN                                                 │
 │   1. GITLAB_TOKEN environment variable, only for the         │
 │      instance GITLAB_URL names (REDMINE_TOKEN: REDMINE_URL)  │
 │   2. Native OS Keyring — keyed per GitLab instance URL       │
 │      (multi-tenant: each instance has its own slot)          │
 │   3. Interactive CLI prompt → saved to OS Keyring            │
 └──────────────────────────────────────────────────────────────┘
 ```

1. **First-Run Onboarding:**
   If no project is configured yet, the desktop window opens a setup window (GitLab URL, project, token — all checked against GitLab before anything is saved). The terminal UI prompts instead:

   ```text
   ⚙️  No project configured yet. Let's set one up.

   GitLab URL [https://gitlab.com]: https://gitlab.my-company.com
   GitLab Project ID: 12345678
   Project name (optional label): My Project
   ✅ Project saved to projects.toml!

   🔑 No GitLab Personal Access Token found in GITLAB_TOKEN or the OS keyring.
   GitLab Personal Access Token: ••••••••••••  (hidden input)
   ✅ GitLab Personal Access Token securely saved to the OS keyring.
   ```

2. **Secure Token Persistence:**
   The token is encrypted and handed off directly to your operating system's native secret manager:
   * **Linux:** GNOME Keyring / KWallet via Secret Service API
   * **macOS:** Apple Keychain Service
   * **Windows:** Windows Credential Manager

3. **Subsequent Launches:**
   You can delete the `GITLAB_TOKEN` entry from your `.env` completely. On subsequent runs, `gitlab-tracker` retrieves the token silently from the OS Keyring without requiring plain-text files or manual re-entry.

4. **No token, no start:**
   If the prompt is left empty (or cannot be read), the app exits with `A GitLab Personal Access Token is required (GITLAB_TOKEN, OS keyring or prompt)` instead of starting a dashboard that cannot fetch anything. The token is kept in zeroized memory and never written to disk by the app.

5. **Sent only where it belongs:**
   A token is sent over `https://` (an `http://` URL is refused in the window, and warned about in the terminal, unless it points at this machine), and a redirect to another server fails the request instead of handing the token over.

Which token to create: [GitLab token](gitlab-token.md).

## .env file

No file is needed to start: on first launch, the desktop window opens a setup window, and the terminal UI prompts for the values below, then saves them to `projects.toml`.

```text
 ┌──────────────────────────────────────────────────────────┐
 │              FIRST-RUN INTERACTIVE ONBOARDING            │
 ├──────────────────────────────────────────────────────────┤
 │ 🌐 GitLab URL [https://gitlab.com]: _                    │
 │ 🔢 GitLab Project ID: _                                  │
 │ 🏷️  Project name (optional): _                           │
 │ 🔑 GitLab Personal Access Token: _                       │
 └──────────────────────────────────────────────────────────┘
```

For teams and CI pipelines, you can still pre-configure everything via a `.env` file to skip the prompts entirely:

1. Copy the provided template to `~/.config/gitlab-tracker/.env` (the only `.env` read: one in the current directory or a parent is ignored, so a cloned repository cannot point the app — and your token — at another server):

   ```bash
   cp .env.example ~/.config/gitlab-tracker/.env
   ```

2. Open `.env` and specify your project details:

   ```env
   # Required: Your target GitLab Project ID
   GITLAB_PROJECT_ID=12345678

   # Optional: Custom self-hosted GitLab instance (defaults to https://gitlab.com if omitted)
   GITLAB_URL=https://gitlab.my-company.com

   # Optional: Override token via environment variable (not recommended for disk storage)
   # GITLAB_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx
   ```

## Which project is loaded, which setting wins

**Which project is loaded** (window, terminal UI and CLI commands alike):

1. `--project <name | project ID | 1-based index>` on the command line
2. `GITLAB_URL` + `GITLAB_PROJECT_ID` (shell or `.env`, both required) — if `projects.toml` has an entry with that URL and project ID, its per-project settings are used; otherwise the project runs on defaults
3. first `[[project]]` with `active = true` in `projects.toml`, else the first entry
4. one-time migration from a legacy `config.json`, else the first-run prompt

Press `w` in the terminal UI to switch to another `projects.toml` entry without restarting: the current project's state is saved, then the chosen one is loaded with its own settings (`GITLAB_TRACKER_*` variables still apply; `GITLAB_URL` / `GITLAB_PROJECT_ID` only select the project at launch). The chosen project becomes the `active` entry of `projects.toml`, so the next launch opens it (`--project` and `GITLAB_URL` / `GITLAB_PROJECT_ID` still take precedence). If the new project has no token in `GITLAB_TOKEN` or the keyring, the dashboard briefly gives way to the token prompt.

Settings edited in the app (columns, branches, settings dashboard) are always saved to **the project currently loaded**, never to another `projects.toml` entry.

**Display settings** (refresh interval, branches, label prefixes, activity thresholds, …) are then resolved in this order (highest to lowest priority):

1. **`GITLAB_REFRESH_INTERVAL_SECS`** — for the refresh interval only, wins over everything below
2. **`GITLAB_TRACKER_*` environment variables** — see the table below
3. **The loaded `[[project]]` entry in `projects.toml`** — every field it sets
4. **`config.json`** (`~/.config/gitlab-tracker/config.json`) — legacy global file, a default template is written on first run
5. **Built-in defaults** (`https://gitlab.com`, `["main"]` for default branches, 900 s refresh)

`.env` is read from `~/.config/gitlab-tracker/.env` only (made private to your account, `0700`, at startup). Layers 2, 4 and 5 are merged with [Figment](https://crates.io/crates/figment); each higher-priority source only overrides the keys it explicitly sets.

### `GITLAB_TRACKER_*` overrides

| Environment variable                   | `AppConfig` field overridden | Example             |
| :------------------------------------- | :--------------------------- | :------------------ |
| `GITLAB_TRACKER_REFRESH_INTERVAL_SECS` | `refresh_interval_secs`      | `300`               |
| `GITLAB_TRACKER_DEFAULT_BRANCHES`      | `default_branches`           | `[main,staging]`    |
| `GITLAB_TRACKER_TABLE_LABEL_PREFIXES`  | `table_label_prefixes`       | `[deploy::,review::]` |
| `GITLAB_TRACKER_ACTIVITY_RECENT_DAYS`  | `activity_recent_days`       | `3`                 |
| `GITLAB_TRACKER_ACTIVITY_STALE_DAYS`   | `activity_stale_days`        | `14`                |

> List values must use brackets (`[a,b]`). A bare `a,b` is read as a single string: all `GITLAB_TRACKER_*` overrides are then ignored, with a warning in the log (`config.json` and `projects.toml` still apply).

> **Upgrading from an older version?** If `projects.toml` does not exist yet but `config.json` holds `gitlab_url` + `project_id`, the app creates `projects.toml` from it once (connection details and project-scoped settings) and prints `✅ Project settings migrated from config.json to projects.toml`. `config.json` itself is still read afterwards as the lowest-priority layer, so it can simply be left in place.
>
> **Migrating from legacy env vars?** The old bare variable names (`DEFAULT_BRANCHES`, `ACTIVITY_STALE_DAYS`, etc.) are no longer supported. Rename them with the `GITLAB_TRACKER_` prefix in your `.env` or shell profile.

## Load on your GitLab / Redmine

A refresh reads the tracked MRs in lists (100 per request), their pipelines and jobs with one GraphQL query per batch, and a branch's new commits only; linked Redmine tickets are read 100 per request. Projects on the same instance share one limit of 3 simultaneous requests, and every request to it pauses when the server answers `429` (or `503` with `Retry-After`). The status bar shows the requests the next refresh should send and the share saved (`−X%`). Set `GITLAB_TRACKER_GRAPHQL=off` to keep every read on REST (an older GitLab whose schema lacks a field falls back to REST by itself). Details and figures: [`GRAPHQL.md`](https://github.com/julien-langlois/gitlab-tracker/blob/main/GRAPHQL.md).

## Logs and build options

```bash
# Verbose log (daily-rotated file in ~/.config/gitlab-tracker/, default level: warn)
RUST_LOG=info gitlab-tracker gui

# Build without desktop notifications (headless / CI environments)
cargo install --path gitlab-tracker --no-default-features
```
