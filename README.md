# 🚀 GitLab MR Tracker

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker)](https://crates.io/crates/gitlab-tracker)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker)](https://crates.io/crates/gitlab-tracker)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.94+-orange.svg)](https://www.rust-lang.org/)

**GitLab MR Tracker** follows your team's GitLab merge requests across your environment branches (`main`, `staging`, `production`…): whether each MR reached each branch — by merge, cherry-pick, rebase or a partial copy — what holds the others back, and who has the next move. It runs as a desktop window, or in the terminal.

![The merge requests page: views and filters, each MR with its next step and its presence on every branch, the inspector](https://github.com/julien-langlois/gitlab-tracker/raw/HEAD/docs/assets/desktop/merge-requests.png)

## ✨ Key Features

* 🔍 **Branch presence** — on which branch each MR landed, even cherry-picked, rebased or partly copied.
* 👉 **Next step** — who has the ball on each MR, and what to do: review, fix the CI, answer threads, merge, deploy.
* 🗂️ **MR page** — why it can't merge, where it is, its discussions, changes, commits, pipelines and timeline.
* ☀️ **Today** — your reviews, your MRs that need you, and your stand-up in one click.
* 🚀 **Promote & Environments** — what is ready to go to the next branch, and the commands to bring it there.
* 📅 **Milestones** — will the release make its date? A forecast from your team's real pace.
* 📈 **Delivery, CI, Team** — DORA measures, flaky jobs and CI budget, review load and who reviews whom.
* 🧭 **Journal, Overlaps, Stacked MRs** — what happened, MRs about to conflict, chains of MRs out of step.
* ✍️ **Signed commits** *(optional)* — warns about unsigned commits, forgives GitLab rebases.
* 🔐 **Read-only and secure** — tokens in the OS keyring; a read-only fine-grained token is enough.
* ⌨️ **Terminal UI** — the same dashboard from the keyboard, for terminals and SSH sessions.

![An MR page: why it can't merge, where it is on each branch, people, ticket and lifecycle](https://github.com/julien-langlois/gitlab-tracker/raw/HEAD/docs/assets/desktop/mr-page.png)

| Page | What it covers |
| :--- | :--- |
| [Desktop window](docs/desktop-window.md) | Install and run the window, every page with a screenshot. |
| [Terminal UI](docs/terminal.md) | The terminal dashboard: table, filters, inspector, cockpit, stats, shortcuts. |
| [GitLab token](docs/gitlab-token.md) | Which read-only token to create and why. |
| [Signed commits](docs/signed-commits.md) | Checking that every commit of the open MRs is signed. |

## 🔑 Authentication & Configuration

The application requires a GitLab project configuration and an API Personal Access Token.

### Step 1: Set up environment variables (optional)

> **✨ Zero-config first run:** If no `.env` file or `projects.toml` is present, `gitlab-tracker` will interactively prompt you for the required values on first launch and persist them automatically to `projects.toml`. No manual file setup is needed.

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

---

### 🔄 Settings Resolution Order

**Which project is loaded** (TUI and CLI commands alike):

1. `--project <name | project ID | 1-based index>` on the command line
2. `GITLAB_URL` + `GITLAB_PROJECT_ID` (shell or `.env`, both required) — if `projects.toml` has an entry with that URL and project ID, its per-project settings are used; otherwise the project runs on defaults
3. first `[[project]]` with `active = true` in `projects.toml`, else the first entry
4. one-time migration from a legacy `config.json`, else the first-run prompt

Press `w` in the dashboard to switch to another `projects.toml` entry without restarting: the current project's state is saved, then the chosen one is loaded with its own settings (`GITLAB_TRACKER_*` variables still apply; `GITLAB_URL` / `GITLAB_PROJECT_ID` only select the project at launch). The chosen project becomes the `active` entry of `projects.toml`, so the next launch opens it (`--project` and `GITLAB_URL` / `GITLAB_PROJECT_ID` still take precedence). If the new project has no token in `GITLAB_TOKEN` or the keyring, the dashboard briefly gives way to the token prompt.

Settings edited from the TUI (columns `C`, branches in Insert mode, settings dashboard `,`) are always saved to **the project currently loaded**, never to another `projects.toml` entry.

**Display settings** (refresh interval, branches, label prefixes, activity thresholds, …) are then resolved in this order (highest to lowest priority):

1. **`GITLAB_REFRESH_INTERVAL_SECS`** — for the refresh interval only, wins over everything below
2. **`GITLAB_TRACKER_*` environment variables** — see the table below
3. **The loaded `[[project]]` entry in `projects.toml`** — every field it sets
4. **`config.json`** (`~/.config/gitlab-tracker/config.json`) — legacy global file, a default template is written on first run
5. **Built-in defaults** (`https://gitlab.com`, `["main"]` for default branches, 900 s refresh)

`.env` is read from `~/.config/gitlab-tracker/.env` only (made private to your account, `0700`, at startup). Layers 2, 4 and 5 are merged with [Figment](https://crates.io/crates/figment); each higher-priority source only overrides the keys it explicitly sets.

#### `GITLAB_TRACKER_*` — available overrides

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

#### Load on your GitLab / Redmine

A refresh reads the tracked MRs in lists (100 per request), their pipelines and jobs with one GraphQL query per batch, and a branch's new commits only; linked Redmine tickets are read 100 per request. Projects on the same instance share one limit of 3 simultaneous requests, and every request to it pauses when the server answers `429` (or `503` with `Retry-After`). The status bar shows the requests the next refresh should send and the share saved (`−X%`). Set `GITLAB_TRACKER_GRAPHQL=off` to keep every read on REST (an older GitLab whose schema lacks a field falls back to REST by itself). Details and figures: [`GRAPHQL.md`](GRAPHQL.md).

---

### Step 2: First-Run Interactive Onboarding & Keyring PAT Security Layer

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
   If no project is configured yet, the application will prompt you interactively on first launch:

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

#### Which token to create

`gitlab-tracker` only reads. Create a [fine-grained personal access token](https://docs.gitlab.com/auth/tokens/fine_grained_access_tokens/) (GitLab 19.2+) limited to the projects you track, and give **Read** — nothing else — to:

* **Group and project** tab: Deployment, Job, Pipeline, Label, Work Item, Project, Commit, Merge Request, Repository, Repository Tag.
* **User** tab *(optional)*: User — or set `gitlab_username` in `projects.toml` instead.

Step-by-step creation, what each permission is used for, a script to check the token, replacing it and older GitLab versions: **[docs/gitlab-token.md](docs/gitlab-token.md)**.

---

### Step 3: Project Configuration (`projects.toml`)

All settings — connection details, display preferences, branch lists, and label colours — live in a single TOML file per project:

* **Linux:** `~/.config/gitlab-tracker/projects.toml`
* **macOS:** `~/Library/Application Support/gitlab-tracker/projects.toml`
* **Windows:** `C:\Users\<User>\AppData\Roaming\gitlab-tracker\projects.toml`

The file supports **multiple projects** in a `[[project]]` array. The active project is the first entry with `active = true` (or the first entry overall when none is marked).

#### Full annotated example

```toml
[[project]]
name = "My Company — Backend"
gitlab_url = "https://gitlab.my-company.com"
project_id = "12345678"
active = true

# Branches whose pipeline status appears as columns in the MR table.
default_branches = ["main", "staging"]

# Branches currently tracked in the TUI (managed automatically via Insert mode).
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

#### 🔔 How Desktop Notifications Work

Notifications fire for GitLab MR events (new branch, MR updated, mergeability changed, milestone changed, complexity changed, review reminders) and tracker-ticket events when a provider such as Redmine is active. Tracker notifications include ticket field changes detected during refreshes and successful automatic status transitions triggered by configured workflow mappings.

MR notifications include a clickable **"Open MR"** button; tracker notifications include **"Open ticket"** and open the linked ticket URL instead. Change notifications are suppressed during the initial sync to avoid spurious alerts on restart.

| Event | Trigger |
| :--- | :--- |
| 🌿 New branch | The MR's merge/squash SHA is found on a tracked branch it was not known on |
| 🕐 MR updated | `updated_at` changed since the previous refresh |
| 🔀 Mergeability changed | e.g. `MERGEABLE → CONFLICT` |
| 🏁 Milestone changed | e.g. `v2.4.0 → v2.5.0` |
| ⚠️ Complexity changed | The effort band changed (e.g. `🟢 EASY → 🔴 COMPLEX`) |
| 👁 Review waiting | An MR awaiting your review (same rule as *To review: me*) was requested `review_reminder_hours` ago or more; repeated every `review_reminder_hours`, and once again after a restart |
| 👁 New commits since your review | The author pushed after your last comment, reply or approval on an MR still awaiting your review; once per push |
| 🎫 Ticket field changed *(tracker)* | Priority, status, assignee, version or progress of the linked ticket |
| ✅ Status transitioned *(tracker)* | An automatic tracker status transition succeeded |

* **Never blocks the UI:** notifications are queued to one background worker thread that performs the (synchronous) D-Bus calls, so a slow or hung notification daemon cannot freeze the dashboard. At most 8 notifications wait for a click at once; beyond that they still show, but clicking does nothing.
* **Anti-spam:** change notifications are suppressed during the initial sync. An MR seen for the first time (no branch set persisted yet in `tracker_<hash>.json`) records its branches silently instead of sending one toast per branch; once known, every new branch notifies, including changes made while the app was closed.
* **Platforms:** click-to-open uses D-Bus actions on Linux (GNOME, KDE…); without a notification daemon, clicks are ignored. Build with `--no-default-features` to drop the feature and `notify-rust` (`libdbus` is still needed on Linux, by the OS keyring).

---

#### 🔎 How Branch Presence Is Detected

For each MR and each tracked branch, the checks run in this order and the first match wins:

1. **Same SHA (`🟢 PRESENT`).** The MR's merge commit, else its squash commit, else — for a merged fast-forward MR — its HEAD, is looked up with the GitLab Refs API (`/commits/:sha/refs?type=branch`): the branches *containing* that commit. This covers merges, fast-forwards and `git reset --hard origin/develop`. If GitLab answers 404 (commit garbage-collected after a force-push), the MR's target branch is assumed. If the call fails transiently (rate limit, 5xx, network), the branches found earlier are kept — never shown as absent because of an outage. At most 5 pages (500 branches containing the commit) are read; past that, the branches found earlier are kept too, and the copy scan below still finds the merge SHA on the branch.
2. **Copies (`🍒` / `≈` / `◐`).** After every refresh cycle, each branch that some MR is missing from is scanned: its commits since the oldest such MR was created, at most 90 days back. A branch with nothing missing costs no API call. Each scanned commit gives three keys: its SHA, the SHAs named by `(cherry picked from commit …)` trailers, and its **fingerprint** — author e-mail, author date and title. `git cherry-pick` and `git rebase` keep the author and the author date, so the fingerprint survives a copy even when conflicts changed the patch. Surrounding quotes added to titles by some tools (`'#1410856: …'`) are ignored.
   * First the MR's merge / squash commit is matched (by SHA, trailer, then fingerprint): the whole MR was copied at once.
   * Otherwise its own commits are matched **one by one** (the MR's commit list is fetched with the diff stats, behind the same `updated_at` cache): all found → `PRESENT` / `PICKED` / `COPIED` (the weakest proof wins), some found → `◐ PARTIAL`.
   * Open and closed MRs are checked too: an urgent fix can be cherry-picked from an open MR, or land through another MR.

**Limits.** A copy is not recognised when its author date was reset (`git rebase --ignore-date`, `--reset-author`, `git commit --amend --reset-author`), when its title was reworded, or when it is older than the 90-day scan window. `≈ COPIED` is an inference: a rewritten commit that kept the author, the author date and the title would match too. Squash-merging a branch of several MRs into another (e.g. a release branch) creates a new commit with a new author date, which none of them can be matched to. For an exact `🍒`, cherry-pick with `-x` or with GitLab's *Cherry-pick* button.

A desktop notification fires when an MR newly reaches a branch through a copy. The first scan of each session only records, so restarting never re-notifies.

#### 🌿 How Branch Resolution Works

Tracked branches (the columns shown in the MR table) are resolved in this priority order at startup:

1. **`tracked_branches`** in `projects.toml` — the canonical source, written automatically by the TUI whenever you add or remove a branch in Insert mode.
2. **`branches`** in `tracker_<hash>.json` — legacy field from older versions, migrated silently to `projects.toml` on first startup and never written again.
3. **`default_branches`** in `projects.toml` — used on the very first run before any branch has been tracked interactively.

---

## 📦 Installation

### Prerequisites

Before installing, ensure the following system dependencies are present:

| Platform    | Requirement                   | Notes                                                                  |
| ----------- | ----------------------------- | ---------------------------------------------------------------------- |
| **Linux**   | `libdbus-1-dev`, `pkg-config` | Required for OS Keyring (Secret Service API) and desktop notifications |
| **macOS**   | —                             | Uses native Apple Keychain — no extra dependencies                     |
| **Windows** | —                             | Uses native Windows Credential Manager — no extra dependencies         |

**Linux (Debian / Ubuntu):**

```bash
sudo apt install libdbus-1-dev pkg-config
```

**Linux (Fedora / RHEL):**

```bash
sudo dnf install dbus-devel pkgconf
```

**Linux (Arch):**

```bash
sudo pacman -S dbus pkgconf
```

---

### Recommended — Install from crates.io

The simplest way to install `gitlab-tracker` if you have Rust (1.94+) available:

```bash
cargo install gitlab-tracker
```

This downloads, compiles, and installs the latest published release directly from [crates.io](https://crates.io/crates/gitlab-tracker) into `~/.cargo/bin/`. No cloning required.

### Pre-built Binaries

If you prefer not to compile, download the latest pre-compiled binary for your architecture from the [Releases Page](https://github.com/julien-langlois/gitlab-tracker/releases) and place it somewhere on your `$PATH`.

### Building from Source

For development or to test unreleased changes, clone the repository and build manually:

```bash
git clone git@github.com:julien-langlois/gitlab-tracker.git
cd gitlab-tracker

# Build optimized release executable (builds all workspace members)
cargo build --release

# Optional: install binary globally to ~/.cargo/bin/
cargo install --path gitlab-tracker

# Build without desktop notifications (headless / CI environments)
cargo install --path gitlab-tracker --no-default-features
```

Once installed via any of the methods above, launch the dashboard from any terminal folder:

```bash
gitlab-tracker

# Demo mode: mock data, no GitLab access, read-only (screenshots, trying the UI)
gitlab-tracker --demo

# Verbose log (daily-rotated file in ~/.config/gitlab-tracker/, default level: warn)
RUST_LOG=info gitlab-tracker
```

### 🖥️ Desktop window

The window is behind the `gui` feature and is **not on crates.io yet** (it depends on the unreleased `bevel` UI crates, checked out next to this repository, and needs Rust 1.95+):

```bash
cargo install --path gitlab-tracker --features gui      # add stats,redmine as needed

gitlab-tracker gui            # the active project
gitlab-tracker --demo gui     # demo data, read-only
```

Requirements, projects in tabs, every page and the Linux dock icon: **[docs/desktop-window.md](docs/desktop-window.md)**.

---

## 🧰 CLI Commands

`gitlab-tracker` is primarily a TUI application, but it also exposes utility subcommands for setup and diagnostics. CLI commands run before the TUI starts and can be called from any directory because configuration is loaded from the XDG `projects.toml` file.

### Select a project

CLI commands and the TUI share the same project resolution (see [Settings Resolution Order](#-settings-resolution-order)). For multi-project setups, pass `--project` to target a specific entry without changing `active = true` — it takes precedence over `GITLAB_URL` / `GITLAB_PROJECT_ID`:

```bash
# Match by projects.toml name
gitlab-tracker --project "My Company — Backend" tracker-statuses

# Match by GitLab project ID
gitlab-tracker --project 12345678 tracker-statuses

# Match by 1-based index in projects.toml
gitlab-tracker --project 2 tracker-statuses
```

### `tracker-statuses`

Prints the status/transition IDs exposed by the configured tracker provider for the selected project. This is mainly used to discover provider-specific IDs before configuring GitLab-to-tracker workflow automation.

With the Redmine feature enabled, the command calls Redmine's `GET /issue_statuses.json` endpoint and prints each `status_id` with its human-readable label:

```bash
cargo run -p gitlab-tracker --features redmine -- --project "My Company — Backend" tracker-statuses
```

Installed binary example:

```bash
gitlab-tracker --project "My Company — Backend" tracker-statuses
```

Example output:

```text
Tracker statuses for provider 'redmine':
     1  New
     2  In Progress
     3  Resolved
     5  Closed
```

Requirements:

* the selected project must have a `[project.tracker]` section;
* the tracker provider must support status discovery through `TicketTransitionProvider`;
* for Redmine, the binary must be built with `--features redmine`;
* the Redmine token must be available via `REDMINE_TOKEN` (used only when `REDMINE_URL` names the same instance), OS keyring, or the interactive token prompt.

---

### `checklist`

Prints the Markdown release checklist (the same table as `e` in the dashboard) on stdout, without starting the TUI — handy in a release script, a CI job or a merge-request description:

```bash
gitlab-tracker checklist                                  # every tracked MR
gitlab-tracker checklist --milestone "Sprint 42"          # the milestone's MRs, tracked or not
gitlab-tracker checklist --absent-from production         # what is left to deploy
gitlab-tracker --project backend checklist --milestone "1.2.0" --absent-from production > release.md
```

The project is loaded exactly like the TUI loads it (same settings, branches and token lookup). The command waits for every MR fetch and branch scan, so cells are never `…`; MRs that failed to load are shown with `…`. It is read-only: no state file, stats snapshot or tracker status transition is written, and merged MRs are listed even when `hide_merged` is on. `--absent-from` must name a tracked branch.

## 🏗️ Project Architecture

This project is structured as a **Cargo workspace** with four crates. Contributor-level internals (event bus, data model, persistence) are described in [`ARCHITECTURE.md`](ARCHITECTURE.md).

```text
gitlab-tracker/                  # Binary crate — TUI orchestrator
└── src/
    ├── main.rs          # Entry point: logging, config, project, token, provider wiring, event loop
    ├── cli.rs           # clap arguments (--demo, --project) and subcommands (tracker-statuses)
    ├── app.rs           # App state, apply_event dispatcher, navigation, sort, filters, UiLayout
    ├── app/
    │   ├── mr_changes.rs     # Change detection on MR loads → log + desktop notifications
    │   ├── tracker_sync.rs   # Linked-ticket fetches, status transitions, TimeLog cache
    │   ├── stats_recorder.rs # Stats snapshots on MR loads (feature `stats`)
    │   └── stats_view.rs     # Stats overlay state: tab, window, report, generation guard
    ├── models.rs        # MrData (single MR model), TrackedMr / SavedMr, AppEvent, MrStatus, Effort
    ├── notify.rs        # Desktop notifications: non-blocking notify-rust worker (no-ops without `notifications`)
    ├── gitlab.rs        # Async GitLab client: pagination, caching guards, rate-limit semaphore
    ├── events.rs        # Keyboard & mouse dispatch (Normal / Insert / popups), shared with demo mode
    ├── storage.rs       # projects.toml, config.json layer, state files, token lookup
    ├── config.rs        # AppConfig, label colours & wildcard matching, VisibleColumns, activity badge
    ├── settings.rs      # Settings dashboard editor state ([,])
    ├── settings_core.rs # inventory::submit! — built-in project settings
    ├── utils.rs         # Fuzzy matching, git clone command, terminal restore
    ├── demo.rs          # Demo mode (--demo): mock data through the real key handler, read-only
    ├── actions.rs       # Action + dispatch: what the user asks for, shared by the TUI and the window
    ├── gui/             # Desktop window (feature `gui`): egui views over the same App and actions
    ├── shortcuts_core.rs# inventory::submit! — built-in keyboard shortcut block (Core section)
    ├── filters_core.rs  # inventory::submit! — built-in filter definitions (state, mergeability, …)
    ├── columns_core.rs  # inventory::submit! — built-in column definitions (activity, labels, …)
    └── ui/
        ├── mod.rs             # Root layout renderer & input bar (mode-aware)
        ├── popups.rs          # Log Time, milestone autocomplete, filter / column pickers, settings
        ├── help_popup.rs      # [?] help popup built from the shortcut registry
        ├── theme.rs           # ThemeMode, Palette — dark/light colour sets (OSC 11 detection in main.rs)
        ├── status_bar.rs      # One-line status bar (project, timer, API counts, sort, filter, spinner)
        ├── table.rs           # Main MR table widget
        ├── cockpit.rs         # Lower-left cockpit pane rendering
        ├── cockpit/summary.rs # Cockpit metrics computed from the visible rows (time-zone aware)
        ├── inspector.rs       # Upper-right pane: MR metadata, fetch errors & pipeline history
        ├── tracker.rs         # Lower-right pane: linked ticket details & time log
        ├── stats.rs           # Stats overlay shell: tabs, status bar, refresh (feature `stats`)
        └── stats/             # common, overview, flow, quality, forecasts, correlations tabs

gitlab-tracker-core/             # Library crate — shared contracts, zero UI dependency
└── src/
    ├── lib.rs           # Re-exports
    ├── provider.rs      # TrackerProvider, TicketTransitionProvider, TrackerError, LinkedTicket, TicketChange
    ├── domain.rs        # GitlabMrState, MergeabilityStatus, PipelineState, Requirement
    ├── filters.rs       # FilterDef + MrSnapshot + inventory registry
    ├── columns.rs       # ColumnDef + inventory registry
    ├── shortcuts.rs     # ShortcutBlock / ShortcutFactory + inventory registry
    ├── settings.rs      # ProjectSettingDef / ProjectSettingFactory + inventory registry
    └── secrets.rs       # (feature `secrets`) resolve_secret: env var → OS keyring → prompt

gitlab-tracker-redmine/          # Library crate — optional Redmine integration plugin
└── src/
    ├── lib.rs           # RedmineProvider (from_tracker_section: config + token): TrackerProvider + TicketTransitionProvider
    ├── client.rs        # Async Redmine REST client (issues, paginated time entries, activities, statuses)
    ├── config.rs        # RedmineConfig ([project.tracker] fields), LabelColorConfig, status transitions
    ├── detector.rs      # Regex-based ticket ID detector (title & description)
    ├── keyring.rs       # Redmine token lookup via core::secrets (REDMINE_TOKEN → keyring → prompt)
    ├── shortcuts.rs     # inventory::submit! — Redmine keyboard shortcut block
    ├── filters.rs       # inventory::submit! — "Has linked ticket" filter definition
    └── columns.rs       # inventory::submit! — "Tracker" column definition

gitlab-tracker-stats/            # Library crate — optional analytics & velocity engine
├── migrations/          # Versioned SQL schema (PRAGMA user_version), embedded with include_str!
└── src/
    ├── lib.rs           # Re-exports + crate architecture documentation
    ├── snapshot.rs      # MrStatsSnapshot + SnapshotTrigger, canonical UTC timestamps
    ├── db.rs            # SqliteStatsDb: migrations, upsert/query, startup maintenance (purge, repair + backup, backfill)
    ├── metrics.rs       # PerMrMetrics: cycle_time, pipeline_failure_rate, comment_density (pure fns)
    ├── aggregator.rs    # TimeWindow (LastDays / Range) + AggregatedStats
    ├── correlation.rs   # Spearman ρ with tie-handling, p-value via t-distribution
    ├── poisson.rs       # Forecasts, flow pressure, pressure signals, baseline anomalies
    ├── report.rs        # StatReport::load / build_with_baseline
    ├── settings.rs      # inventory::submit! — [project.stats] settings dashboard entries
    └── shortcuts.rs     # inventory::submit! — Stats keyboard shortcut block
```

### Optional Feature Flags

| Feature flag    | Default     | Effect                                                                                                                   |
| :-------------- | :---------- | :----------------------------------------------------------------------------------------------------------------------- |
| `notifications` | ✅ enabled  | Desktop notifications via `notify-rust` (module `notify.rs`; without it, notifications are no-ops and `notify-rust` is not built)|
| `redmine`       | ❌ disabled | Redmine ticket & time-tracking integration (see [`gitlab-tracker-redmine`](gitlab-tracker-redmine/README.md))            |
| `stats`         | ❌ disabled | MR analytics, velocity metrics, and Spearman correlations (see [`gitlab-tracker-stats`](gitlab-tracker-stats/README.md)) |
| `gui`           | ❌ disabled | Experimental desktop window, `gitlab-tracker gui` (see [Desktop window](docs/desktop-window.md); source builds only, Rust 1.95+) |

### Tracker Plugins

`gitlab-tracker` supports optional external tracker integrations (Redmine, and future providers such as Jira or Linear) through a plugin architecture based on the `TrackerProvider` trait defined in `gitlab-tracker-core`.

When a tracker plugin is configured, the dashboard is enriched with:

* **Linked ticket display** in the Inspector — subject, type, priority, status, assignee, target version, start date, progress bar, and time tracking (estimate / spent / remaining)
* **Coloured badges** for Type and Priority — colours are fully configurable per-label in the plugin's config file (no hardcoded values — works with any language or custom workflow)
* **Time Log view** (`p` on the Tracker pane) — chronological list of time entries for the linked ticket, cached per ticket and refreshed on each refresh cycle or `r`
* **Log time** (`l`) — submit a new time entry directly from the TUI
* **Tracker column** in the table (toggleable via `c`) — ticket ID, status and spent / estimated time

Each plugin lives in its own crate and is activated via a Cargo feature flag. See the plugin's own README for setup instructions:

| Plugin      | Feature flag         | Documentation                                                          |
| :---------- | :------------------- | :--------------------------------------------------------------------- |
| **Redmine** | `--features redmine` | [`gitlab-tracker-redmine/README.md`](gitlab-tracker-redmine/README.md) |
| **Stats**   | `--features stats`   | [`gitlab-tracker-stats/README.md`](gitlab-tracker-stats/README.md)     |

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
