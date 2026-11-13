# Command line

Besides the window (`gitlab-tracker gui`) and the terminal UI (`gitlab-tracker`), the binary has a few subcommands for setup and scripts. They can be called from any directory: the configuration comes from `projects.toml`.

```bash
gitlab-tracker gui            # desktop window (feature `gui`)
gitlab-tracker                # terminal UI
gitlab-tracker --demo [gui]   # demo data, no GitLab access, read-only
```

### Select a project

CLI commands and the TUI share the same project resolution (see [which project is loaded](configuration.md#which-project-is-loaded-which-setting-wins)). For multi-project setups, pass `--project` to target a specific entry without changing `active = true` — it takes precedence over `GITLAB_URL` / `GITLAB_PROJECT_ID`:

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

Prints the Markdown release checklist (the same table as **Copy checklist** in the window or `e` in the terminal UI) on stdout, without opening the dashboard — handy in a release script, a CI job or a merge-request description:

```bash
gitlab-tracker checklist                                  # every tracked MR
gitlab-tracker checklist --milestone "Sprint 42"          # the milestone's MRs, tracked or not
gitlab-tracker checklist --absent-from production         # what is left to deploy
gitlab-tracker --project backend checklist --milestone "1.2.0" --absent-from production > release.md
```

The project is loaded exactly like the dashboard loads it (same settings, branches and token lookup). The command waits for every MR fetch and branch scan, so cells are never `…`; MRs that failed to load are shown with `…`. It is read-only: no state file, stats snapshot or tracker status transition is written, and merged MRs are listed even when `hide_merged` is on. `--absent-from` must name a tracked branch.
