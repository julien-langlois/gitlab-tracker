# 🚀 GitLab MR Tracker

[![CI Quality Gate](https://github.com/julien-langlois/gitlab-tracker/actions/workflows/ci.yml/badge.svg)](https://github.com/julien-langlois/gitlab-tracker/actions)
[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker)](https://crates.io/crates/gitlab-tracker)
[![Crates.io Total Downloads](https://img.shields.io/crates/d/gitlab-tracker)](https://crates.io/crates/gitlab-tracker)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.97+-orange.svg)](https://www.rust-lang.org/)

**GitLab MR Tracker** is a fast, asynchronous Terminal User Interface (TUI) dashboard designed for engineering teams. It provides real-time verification of GitLab Merge Requests across target environment branches (`main`, `preproduction`, `staging`, etc.), handling strict SHA verification as well as cherry-picked commit identification.

![gitlab-tracker demo](assets/demo.gif)

## ✨ Key Features

* 🔐 **OS Keyring Integration (Zero Plain-Text Secrets):** Personal Access Tokens (PAT) can be securely stored directly in your OS secret manager (GNOME Keyring, KWallet, macOS Keychain, or Windows Credential Manager).
* 🏷️ **Dynamic Scoped Labels & Custom Chips:**
  * **Smart Filtering:** Configure specific label prefixes (e.g., `deploy::`, `review::`) to display cleanly as colored chips in the main table grid, while keeping **all** attached tags visible in the side inspector panel.
  * **Customizable Palette:** Map label names or wildcard patterns (e.g., `deploy::*`) to custom terminal colors or standard HEX codes (`#FF5733`) via an XDG-compliant JSON config. Labels without a config override automatically fall back to their **GitLab-side colour** (fetched at startup), with foreground computed for legibility.
* ⚡ **High Performance & Asynchronous:** Powered by `tokio` and `reqwest`, utilizing non-blocking event loops and bounded concurrent requests via semaphores to protect GitLab API rate limits.
* 🛡️ **Pass-Through Pass Caching:** Core MR metadata (author, milestone, assignee, description, labels) is fetched once and cached locally. Fully deployed MRs bypass network re-queries entirely ("Green Pass").
* 🔍 **Dual Match Verification Engine:**
  * **System 1 (Strict SHA):** Validates precise merge/squash commit SHAs on target branches (resistant to `git reset --hard`).
  * **System 2 (Intelligent Fuzzy Matcher):** Uses a keyword relevance matrix to verify cherry-picked commits deployed across branches.
* 🖥️ **Responsive Flexbox TUI Grid:** Features a dynamic layout engine (`Constraint::Fill`) that seamlessly scales table columns and side panels from 1080p laptop displays to ultra-wide 4K monitors without empty trailing spaces.
* 🔃 **Smart Auto-Sorting by Last Update:** The dashboard defaults to sorting MRs by `updated_at` (most recently pushed to remote first), automatically re-applied after each refresh. Cycle through sort columns (`S`) and toggle direction (`Shift+S`). The active sort is always visible in the table title bar.
* 🌐 **Browser Integration:** Open any selected MR directly in your default browser with a single keypress (`O`).
* 🔔 **Smart Desktop Notifications:** Receives native OS desktop notifications **only when an MR's branch status has changed** since the last run — no duplicate alerts on restart or redundant refreshes.
* ✨ **Refresh Highlight:** After each background refresh, any MR whose `updated_at` timestamp has changed since the previous cycle is briefly highlighted in the table with a green tint. The highlight fades out automatically after ~10 seconds.
* 📁 **XDG-Compliant Persistence:** Saves tracked dashboard state, UI configurations, and last-known branch statuses automatically to platform-standard configuration paths using `directories`.
* **Customizable Refresh Interval:** Tailor the background polling rate to your needs (defaults to 15 minutes / 900s) via `config.json` or the `GITLAB_REFRESH_INTERVAL_SECS` environment variable.
* 📊 **Activity Badge:** Each MR in the Context Inspector displays a color-coded activity badge based on its `updated_at` timestamp — 🟢 Active, 🟡 Slowing, or 🔴 Stale. Thresholds are fully configurable via `config.json` or environment variables (`ACTIVITY_RECENT_DAYS`, `ACTIVITY_STALE_DAYS`).
* 💬 **Notes Indicator:** The total number of comments and discussion threads (`user_notes_count`) is fetched from the GitLab API at no extra cost and displayed both in the optional **Notes** table column and in the Context Inspector. A yellow `💬 N` badge signals that comments are awaiting attention; a dimmed `✔ No comments` confirms there is nothing to address.
* 🔀 **Animated Status Badge:** For open MRs, the Status column cycles through three phases every second with no extra column:

  | Phase | Badge | Color | Meaning |
  | :--- | :--- | :--- | :--- |
  | 1 | `OPEN` | 🟩 Green | Base state |
  | 2 | Mergeability | varies | Live mergeability from GitLab API |
  | 3 | `CI RUNNING` / `CI PENDING` | 🟧 Orange | Latest pipeline is active — dimmed to `(n/a)` when no pipeline exists |

  The CI badge only appears when the most recent pipeline is in `Running` or `Pending` state; otherwise phase 3 falls back to the mergeability badge. The animation keeps the layout compact while surfacing both merge-readiness and CI status at a glance.
* 🗂️ **Toggleable Table Columns (`C`):** Press `C` at any time to open an interactive column picker popup. Use `↑`/`↓` to navigate and `Space` to toggle each optional column on or off. Your selection is **instantly saved** to `config.json` and persisted across restarts — no manual file editing required. Available optional columns:

  | Column | Description |
  | :--- | :--- |
  | **Activity** | Color-coded activity badge — 🟢 Active, 🟡 Slowing, 🔴 Stale (same thresholds as the Inspector) |
  | **Target** | The branch the MR is intended to merge into |
  | **Labels** | Filtered label chips (respects `table_label_prefixes`) |
  | **Milestone** | The associated milestone title |
  | **Notes** | Total number of comments and discussion threads — `💬 N` in yellow when non-zero, dimmed `✔ 0` otherwise |

  All columns are hidden by default to keep the layout compact. They can also be enabled statically via `visible_columns` in `config.json` (see configuration section below).
* ⭐ **MR Flagging & Advanced Filters:** Manually flag any MR with `Space` to mark it with a coloured star chevron (★) in the title column. Press `F` to open the **filter picker popup**, which lets you narrow the table by:
  * `Flagged ★` — only your manually flagged MRs
  * **GitLab state** — `Opened`, `Merged`, or `Closed`
  * **Mergeability** — `Mergeable`, `Conflict`, `Needs Rebase`, `Not Approved`, `Requested Changes`, `Draft`, `Discussions`
  * **Has comments** — MRs with at least one note or discussion thread
  * **Milestone** — free-text search on the milestone title (case-insensitive)
  * **Assignee** — free-text search on the GitLab assignee and, when a tracker ticket is linked (e.g. Redmine), its assignee as well

  The active filter is shown in the table header. Flagged state is **persisted across restarts** via `tracker_state.json`.

* 🏁 **Milestone Bulk-Add (Release Manager Workflow):** In Insert mode, type `@` followed by any part of a milestone name to trigger a live autocomplete dropdown. Active and upcoming milestones are fetched from GitLab on startup and filtered in real time as you type. Selecting a milestone with `Enter` automatically adds **all open MRs attached to that milestone** in a single action — no need to enter IDs one by one. Ideal for release managers preparing a deployment checklist.

  ```text
  i             → Enter Insert mode
  @5.2          → filters milestones containing "5.2"
  ↓ / Tab       → navigate suggestions
  Enter         → bulk-add all open MRs from the selected milestone
  Esc           → close dropdown without selecting
  ```

* 🔬 **Pipeline Inspector (`P`):** Press `P` on any selected MR to toggle the side panel between MR metadata and its pipeline history. The last 5 pipeline runs are displayed with per-stage job breakdown, status icons, and execution durations:

  ```text
  #9981  ✔ passed
    ▸ test
      ✔ lint        (18s)
      ✔ unit-tests  (74s)
    ▸ build
      ✔ build       (42s)
    ▸ deploy
      ✔ deploy-staging (31s)
  ```

  Pipeline data is fetched **alongside MR metadata** in the same refresh cycle and **persisted to disk** — so it is immediately available on restart without an extra network call. Re-fetching only occurs when GitLab reports a new `updated_at` timestamp, keeping API usage minimal.

---

## 🔑 Authentication & Configuration

The application requires your GitLab Project configuration and an API Personal Access Token.

### Step 1: Set up environment variables (optional)

> **✨ Zero-config first run:** If no `.env` file or `config.json` is present, `gitlab-tracker` will interactively prompt you for the required values on first launch and persist them automatically to `config.json`. No manual file setup is needed.

```text
 ┌──────────────────────────────────────────────────────────┐
 │              FIRST-RUN INTERACTIVE ONBOARDING            │
 ├──────────────────────────────────────────────────────────┤
 │ 🌐 GitLab URL [https://gitlab.com]: _                    │
 │ 🔢 GitLab Project ID: _                                  │
 │ 🔑 GitLab Personal Access Token: _                       │
 └──────────────────────────────────────────────────────────┘
```

For teams and CI pipelines, you can still pre-configure everything via a `.env` file to skip the prompts entirely:

1. Copy the provided template to create your local `.env` file:

   ```bash
   cp .env.example .env
   ```

2. Open `.env` and specify your project details:

   ```env
   # Required: Your target GitLab Project ID
   GITLAB_PROJECT_ID=12345678

   # Optional: Custom self-hosted GitLab instance (Defaults to https://gitlab.com if omitted)
   GITLAB_URL=https://gitlab.my-company.com

   # Optional: Override token via environment variable (Not recommended for disk storage)
   # GITLAB_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx

   # Optional: Override initial tracked branches for new sessions (comma-separated)
   DEFAULT_BRANCHES="main,develop"

   # Optional: Filter table column tags by prefix (comma-separated)
   TABLE_LABEL_PREFIXES="deploy::,review::"

   # Optional: Activity badge thresholds in the Context Inspector (in days)
   ACTIVITY_RECENT_DAYS=2   # 🟢 Green if updated within N days (default: 2)
   ACTIVITY_STALE_DAYS=7    # 🔴 Red if not updated for N days (default: 7)
   ```

---

### 🔄 Settings Resolution Order

Settings are resolved in the following order (highest to lowest priority):

1. **System Environment Variables & Local `.env`** (current directory)
2. **Global `.env`** (`~/.config/gitlab-tracker/.env`)
3. **User Config File** (`~/.config/gitlab-tracker/config.json`)
4. **Built-in Fallback Defaults** (`https://gitlab.com`, `["main"]` for default branch)

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
 │   2. ~/.config/gitlab-tracker/config.json                    │
 │   3. Interactive CLI prompt → saved to config.json           │
 ├──────────────────────────────────────────────────────────────┤
 │ GITLAB_TOKEN                                                 │
 │   1. GITLAB_TOKEN environment variable (if set)              │
 │   2. Native OS Keyring (GNOME Keyring / macOS Keychain)      │
 │   3. Interactive CLI prompt → saved to OS Keyring            │
 └──────────────────────────────────────────────────────────────┘
```

1. **First-Run Onboarding:**
   If no `GITLAB_TOKEN` is found in your `.env` or environment, the application will prompt you interactively in the terminal on its initial launch:

   ```text
   🌐 No GitLab URL found in config or environment.
      Leave empty to use the default (https://gitlab.com)
   GitLab URL [https://gitlab.com]: https://gitlab.my-company.com
   🔢 No GitLab Project ID found in config or environment.
   Please enter your GitLab Project ID: 12345678
   ✅ Config saved to config.json!

   🔑 No GITLAB_TOKEN found in environment or system Keyring.
   Please enter your GitLab Personal Access Token: glpat-xxxxxxxxxxxx
   ✅ Token securely saved to OS Keyring!
   ```

2. **Secure Token Persistence:**
   The token is encrypted and handed off directly to your operating system's native secret manager:
   * **Linux:** GNOME Keyring / KWallet via Secret Service API
   * **macOS:** Apple Keychain Service
   * **Windows:** Windows Credential Manager

3. **Subsequent Launches:**
   You can delete the `GITLAB_TOKEN` entry from your `.env` completely. On subsequent runs, `gitlab-tracker` retrieves the token silently from the OS Keyring without requiring plain-text files or manual re-entry.

---

### Step 3: UI, Default Branches & Label Customization (`config.json`)

On its first launch, the tool automatically generates a `config.json` file inside your OS user configuration directory:

* **Linux:** `~/.config/gitlab-tracker/config.json`
* **macOS:** `~/Library/Application Support/gitlab-tracker/config.json`
* **Windows:** `C:\Users\<User>\AppData\Roaming\gitlab-tracker\config.json`

You can edit this file to adjust default environment branches, label badge colors, wildcard rules, and activity badge thresholds:

```json
{
  "project_id": "12345678",
  "gitlab_url": "https://gitlab.my-company.com",
  "refresh_interval_secs": 900,
  "default_branches": [
    "main"
  ],
  "table_label_prefixes": [
    "deploy::",
    "review::"
  ],
  "activity_recent_days": 2,
  "activity_stale_days": 7,
  "visible_columns": {
    "target_branch": false,
    "labels": false,
    "milestone": false
  },
  "label_colors": {
    "deploy::*": {
      "bg": "#2E7D32",
      "fg": "white"
    },
    "review::approved": {
      "bg": "magenta",
      "fg": "white"
    },
    "review::*": {
      "bg": "cyan",
      "fg": "black"
    },
    "size::*": {
      "bg": "dark_gray",
      "fg": "white"
    },
    "bug": {
      "bg": "#D32F2F",
      "fg": "white"
    }
  }
}
```

> **Optional table columns** — By default the table only shows the fixed columns (ID, Title, Status) plus your tracked branches, keeping the layout compact. Enable any optional column individually in `config.json` under `visible_columns`:
>
> | Key | Default | Column shown |
> | :--- | :--- | :--- |
> | `activity` | `false` | **Activity** — 🟢 Active / 🟡 Slowing / 🔴 Stale badge |
> | `target_branch` | `false` | **Target** — the branch the MR merges into |
> | `labels` | `false` | **Labels** — filtered label chips (respects `table_label_prefixes`) |
> | `milestone` | `false` | **Milestone** — the associated milestone title |
> | `notes` | `false` | **Notes** — total comment count (`💬 N` in yellow when non-zero) |
>
> Example — enable Activity and Target only:
>
> ```json
> "visible_columns": {
>   "activity": true,
>   "target_branch": true,
>   "labels": false,
>   "milestone": false,
>   "notes": false
> }
> ```

> **Activity badge thresholds** control the colored indicator displayed next to the `Updated` field in the Context Inspector:
>
> | Badge | Meaning | Condition |
> | :--- | :--- | :--- |
> | 🟢 Active | Updated recently | `elapsed days < activity_recent_days` |
> | 🟡 Slowing | Activity slowing down | between the two thresholds |
> | 🔴 Stale | No recent activity | `elapsed days ≥ activity_stale_days` |
> | ⬛ Unknown | Timestamp unavailable | — |

#### 🔔 How Desktop Notifications Work

Notifications fire on four events (new branch, MR updated, mergeability changed, milestone changed) and include a clickable **"Open MR"** button that opens the MR in your default browser. Change notifications are suppressed during the initial sync to avoid spurious alerts on restart.

See [`gitlab-tracker-notify/README.md`](gitlab-tracker-notify/README.md) for the full event reference, platform support details, and feature flags.

---

#### 🌿 How Branch Resolution Works:

1. **Active Session Priority:** If `tracker_state.json` exists from a previous run, the app restores your last active layout (columns added/removed via input).
2. **First Run / Fresh Session:** If no state exists, initial branches are loaded from `DEFAULT_BRANCHES` in `.env` if provided, falling back to `default_branches` in `config.json` (defaults to `["main"]`).

---

## 📦 Installation

### Recommended — Install from crates.io

The simplest way to install `gitlab-tracker` if you have Rust (1.80+) available:

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
```

---

## ⌨️ Dashboard Navigation & Shortcuts

The dashboard operates in two keyboard modes, inspired by vim:

### 🟦 Normal Mode (default)

Shortcut keys are active. The input field is passive.

| Shortcut | Action |
| :--- | :--- |
| `i` or `/` | **Enter Insert mode** — focus the input field |
| `▲` / `▼` or `k` / `j` | Navigate rows in the table |
| `Tab` | Cycle focus between panes: **Dashboard → Inspector → Tracker** → Dashboard (Tracker pane only when a ticket is linked) |
| `T` | When focus is on Dashboard or Inspector: **jump to Tracker pane**. When already on Tracker: **open ticket URL** in browser |
| `P` | **Inspector pane focused**: cycle MR Info ↔ Pipelines. **Tracker pane focused**: toggle Ticket Info ↔ Time Log |
| `L` | **Log time** on the linked tracker ticket *(only when a tracker plugin is configured)* |
| `C` | **Open column picker** — toggle optional columns on/off |
| `O` | Open selected MR in your default web browser |
| `R` | Force immediate network refresh for all MRs |
| `s` | Cycle sort column (`Updated → ID → Milestone → Title → …`) |
| `S` | Toggle sort direction (ascending / descending) |
| `Space` | **Toggle flag ★** on the selected MR — persisted across restarts |
| `F` | Open filter picker (state, mergeability, notes, milestone, assignee…) |
| `Del` | Delete selected MR row |
| `Esc` | Quit dashboard |

### 🟩 Column Picker Mode

Opened with `C`. The table border turns **cyan** as a visual indicator.

| Shortcut | Action |
| :--- | :--- |
| `▲` / `▼` or `k` / `j` | Navigate the column list |
| `Space` | Toggle the highlighted column on/off |
| `Enter` or `Esc` | Close the picker — changes are saved immediately to `config.json` |

### 🟨 Insert Mode

The input field has exclusive focus. All printable keys feed the field — shortcuts are suspended. The input bar turns **yellow** as a visual indicator.

| Shortcut | Action |
| :--- | :--- |
| `142` + `Enter` | Add MR ID `!142` to tracking |
| `staging` + `Enter` | Add branch `staging` to target columns |
| `-142` + `Enter` | Remove MR ID `!142` from tracking |
| `-staging` + `Enter` | Remove branch column `staging` |
| `@name` | Filter milestones matching `name` — opens autocomplete dropdown |
| `Enter` | Submit input, or confirm highlighted milestone suggestion |
| `Esc` | Close autocomplete dropdown, or cancel and return to Normal mode |

#### 🏁 Milestone Autocomplete (Insert Mode)

When the input starts with `@`, a dropdown appears above the input bar listing all active/upcoming milestones fetched from GitLab. The list is filtered in real time as you type.

| Shortcut | Action |
| :--- | :--- |
| `↑` / `↓` or `Shift+Tab` / `Tab` | Navigate suggestions |
| `Enter` | Confirm selection — bulk-adds all open MRs from the milestone |
| `Esc` | Close dropdown without selecting |

> **Why two modes?** Branch names starting with `s`, `S`, `p`, `P`, `o`, `O`, `r` or `R` would otherwise collide with shortcut keys. Insert mode guarantees the full branch name is captured without interference.

---

## 🏗️ Project Architecture

This project is structured as a **Cargo workspace** with four crates:

```text
gitlab-tracker/                  # Binary crate — TUI orchestrator
└── src/
    ├── main.rs          # Entry point: wires providers, calls build_tracker_colors(), event loop
    ├── app.rs           # State machine, InputMode, row navigation & sort logic
    ├── config.rs        # Label filtering, wildcard matching, parse_color() & activity badge
    ├── models.rs        # Strongly-typed API DTOs & runtime event types
    ├── gitlab.rs        # Async network handling & rate-limit semaphores
    ├── events.rs        # Keyboard & mouse event dispatch (Normal / Insert mode routing)
    ├── storage.rs       # OS Keyring interface & XDG state/config persistence
    ├── utils.rs         # Fuzzy matching algorithmic utilities
    ├── demo.rs          # Demo mode with pre-populated mock data (screenshots & CI)
    └── ui/
        ├── mod.rs       # Root layout renderer & input bar (mode-aware)
        ├── table.rs     # Main MR table widget
        ├── inspector.rs # Upper-right pane: MR metadata & pipeline history
        └── tracker.rs   # Lower-right pane: linked ticket details & time log (TrackerLabelColors)

gitlab-tracker-core/             # Library crate — shared trait contracts, zero UI dependency
└── src/
    ├── lib.rs           # Re-exports: TrackerProvider, LinkedTicket, LabelColorMaps, …
    └── provider.rs      # TrackerProvider trait + all shared domain types
                         #   LinkedTicket: flat ticket data (type, priority, version, progress, …)
                         #   LabelColorMaps: raw (String, String) badge colour maps — no ratatui

gitlab-tracker-notify/           # Library crate — optional desktop notification plugin
└── src/
    └── lib.rs           # notify-rust integration (no-op stubs when feature `desktop` is disabled)

gitlab-tracker-redmine/          # Library crate — optional Redmine integration plugin
└── src/
    ├── lib.rs           # RedmineProvider: implements TrackerProvider + label_colors()
    ├── client.rs        # Async Redmine REST API client (issue, time entries, activities)
    ├── config.rs        # RedmineConfig: YAML load/save, LabelColorConfig, onboarding prompt
    ├── detector.rs      # Regex-based ticket ID detector (title & description)
    └── keyring.rs       # Secure token retrieval via OS Keyring
```

### Optional Feature Flags

| Feature flag | Default | Effect |
| :--- | :--- | :--- |
| `notifications` | ✅ enabled | Desktop notifications via `notify-rust` |
| `redmine` | ❌ disabled | Redmine ticket & time-tracking integration (see [`gitlab-tracker-redmine`](gitlab-tracker-redmine/README.md)) |

### Tracker Plugins

`gitlab-tracker` supports optional external tracker integrations (Redmine, and future providers such as Jira or Linear) through a plugin architecture based on the `TrackerProvider` trait defined in `gitlab-tracker-core`.

When a tracker plugin is configured, the dashboard is enriched with:

* **Linked ticket display** in the Inspector — subject, type, priority, status, assignee, target version, start date, progress bar, and time tracking (estimate / spent / remaining)
* **Coloured badges** for Type and Priority — colours are fully configurable per-label in the plugin's config file (no hardcoded values — works with any language or custom workflow)
* **Time Log view** (`P` × 2) — chronological list of time entries for the linked ticket, auto-refreshed on MR navigation
* **Log time** (`L`) — submit a new time entry directly from the TUI
* **Tracker column** in the table (toggleable via `C`)

Each plugin lives in its own crate and is activated via a Cargo feature flag. See the plugin's own README for setup instructions:

| Plugin | Feature flag | Documentation |
| :--- | :--- | :--- |
| **Redmine** | `--features redmine` | [`gitlab-tracker-redmine/README.md`](gitlab-tracker-redmine/README.md) |

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
