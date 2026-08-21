# CHANGELOG

All notable changes to this project will be documented in this file.

## [0.4.3] - 2026-08-21

### 🚀 Features

- **notify**: Alert when MR complexity category changes during refresh
- **inspector**: Add HEAD SHA and pipeline summary to MR overview

### 🐛 Bug Fixes

- Correct truncated diff stats when GitLab diff overflows
- **detection**: Replace fuzzy keyword matching with strict SHA-based branch detection

### 🔨 Refactor

- **ui**: Rename Complexity to Effort and polish side panel labels

## [0.4.2] - 2026-08-19

### 👷 CI

- **release**: Add libdbus-1-dev and pkg-config to publish-crates and build-release jobs to fix libdbus-sys compilation

## [0.4.1] - 2026-08-19

### 👷 CI

- **ci**: Install libdbus-1-dev, libsecret-1-dev and pkg-config on GitHub Actions runner

## [0.4.0] - 2026-08-19

### 🚀 Features

- **inspector**: Add commit count to Diff & Complexity panel
- **ui**: Add responsive help popup with auto-registered shortcut providers
- **keyring**: Make GitLab token storage multi-tenant keyed by instance URL

### 🐛 Bug Fixes

- **table**: Preserve cursor position on MR list re-sort after refresh

### 🖥️ UI

- **help_popup**: Add blank line between section header and shortcuts

### 🔨 Refactor

- Migrate filters and columns to inventory auto-registration

### ⚙️ Miscellaneous

- **deps**: Migrate `serde_yaml` → `serde_yml`, `keyring` v2 → v3, add `thiserror` on `TrackerProvider`

### 📚 Documentation

- Update architecture section to reflect inventory-based filter/column/shortcut modules

## [0.3.6] - 2026-08-18

### 🚀 Features

- **config**: Migrate all settings from config.json to projects.toml
- **redmine**: Migrate config to projects.toml with multi-tenant support
- **storage**: Scope tracker state file per tenant (`tracker_<hash>.json`)
- **diff**: Add review complexity scoring from MR diff stats

### 🐛 Bug Fixes

- **filter**: Resolve filter-aware MR selection for inspector and side panels (#3)

### 🖥️ UI

- **ui**: Add relative date labels to inspector and tracker panels
- **ui**: Update complexity chips width

## [0.3.5] - 2026-08-07

### 🚀 Features

- **core**: Add TicketChange enum and LinkedTicket::diff for tracker field change tracking
- **core**: Add DoneRatio variant to TicketChange and track progress changes in diff
- **filter**: Add HasLinkedTicket and CiFailing filter modes with n/a dimming

### 🐛 Bug Fixes

- **auto-refresh**: Always re-fetch Redmine tickets on timer tick
- **pipelines**: Always re-fetch pipelines when a cached pipeline is in a transient state

### ⚙️ Miscellaneous

- **changelog**: Adapt cliff.toml commit parsers

### 📚 Documentation

- Manually backfill 0.3.4 changelog entries

### 🖥️ UI

- **ui**: Add animated spinner and live MR count to table title
- **ui**: Inject live MR count, input mode and active filter into terminal window title
- **ui**: Add live CI pipeline badge to status column animation

## [0.3.4] - 2026-08-06

### 🚀 Features

- **notify**: Open MR in browser on notification click

### 🐛 Bug Fixes

- **release**: Scope git-cliff hook to run once via idempotent guard
- **release**: Run git-cliff hook from workspace root via --workdir

### ⚙️ Miscellaneous

- Add git-cliff changelog automation and harden release pipeline

## [0.3.3] - 2026-08-05

### 🚀 Features

- **filter**: Replace cycle with interactive picker (state, mergeability, notes, milestone, assignee)
- Use GitLab label colours as fallback for unoverridden chips
- **redmine**: Add data from Redmine in side panel
- **redmine**: Auto-update ETC on time entry submission

### 📚 Documentation

- Use GitLab label colours as fallback for unoverridden chips

### 🖥️ UI

- **ui**: Add quit confirmation on Esc key press
- **ui**: Replace ANSI DarkGray with absolute RGB colours for consistent readability
- **ui**: Add split tracker pane with mouse-hover focus

## [0.3.2] - 2026-08-04

### 🔨 Refactor

- **tracker**: Decouple orchestrator from redmine-specific cfg flags

## [0.3.0] - 2026-08-02

### 🚀 Features

- **redmine**: Add time log inspector view with live time entries fetching and log time popup
- **notify**: Add gitlab-tracker-notify plugin crate with optional desktop feature

### 🐛 Bug Fixes

- Publish all crates in correct order

### 📚 Documentation

- Update architecture and build instructions for Cargo workspace

### 🔨 Refactor

- **app**: Centralise event handling and fetch context in App
- Remove legacy src/ root — sources live in gitlab-tracker/src/
- Move binary crate into gitlab-tracker/ workspace member

### 🏗️ Build

- **release**: Add cargo-release configuration for workspace patch/minor/major bumps
- Migrate to Cargo workspace with shared version management

### 👷 CI

- Target gitlab-tracker package explicitly for crates.io publish in workspace

## [0.2.10] - 2026-07-31

### 🚀 Features

- Add change-detection notifications for updated_at, mergeability and milestone
- Expand MergeabilityStatus with new GitLab merge statuses

### ⚙️ Miscellaneous

- Bump package version from 0.2.9 to 0.2.10

## [0.2.9] - 2026-07-30

### 🚀 Features

- Add MR flagging with persistent state and filter mode cycling

### 🐛 Bug Fixes

- **keyring**: Unify service name and add legacy entry migration

### 🎬 Demo

- **demo**: Update tape scenario to cover flagging, filter and column picker

### ⚙️ Miscellaneous

- Bump package version from 0.2.8 to 0.2.9

### 🖥️ UI

- Normalize state/merge badges in inspector panel (uppercase, centered)

## [0.2.8] - 2026-07-30

### 🚀 Features

- **inspector**: Enrich MR side panel with sections, people names, merge info and git clone yank
- **pipelines**: Display and persist created_at date in pipeline view
- **onboarding**: Interactive first-run prompt for gitlab_url and project_id
- **milestone**: Add milestone autocomplete and bulk-add MRs for release managers

### 🐛 Bug Fixes

- **pipeline**: Force jobs re-fetch when cached pipelines have no jobs

### ⚙️ Miscellaneous

- Bump package version from 0.2.7 to 0.2.8

### 🔨 Refactor

- Remove X key binding for MR deletion, keep Del only

## [0.2.7] - 2026-07-29

### 🚀 Features

- **notes**: Add user notes count indicator for MR review tracking
- **table**: Highlight recently-updated MRs on refresh + add activity column
- **table**: Add optional activity column with context menu toggle
- Add toggleable optional columns with persistent config and TUI picker

### ⚙️ Miscellaneous

- Bump package version from 0.2.6 to 0.2.7

## [0.2.6] - 2026-07-28

### 🚀 Features

- **input**: Add Normal/Insert mode to prevent shortcut conflicts with branch names
- **pipelines**: Add pipeline inspector panel with rate-limit-aware caching

### 🖥️ UI

- **ui**: Enforce fixed-width centered state badges in table

## [0.2.5] - 2026-07-27

### ⚙️ Miscellaneous

- Bump package version from 0.2.4 to 0.2.5
- Exclude assets directory from crate and release packages

### 🔨 Refactor

- Extract key and mouse event handling into dedicated events module

### 🖥️ UI

- **ui**: Animate mergeability badge on open MR status column
- **ui**: Remove redundant [Open] title prefix now superseded by state badge
- **ui**: Add MR lifecycle state badge to table and inspector

## [0.2.4] - 2026-07-27

### 🚀 Features

- Add ActivePane system to handle focus between dashboard and inspector panes

### 🐛 Bug Fixes

- Clamp inspector scroll to prevent blank space when content fits in pane

### 🎬 Demo

- Add Inspector scroll and pane switching to VHS scenario

### ⚙️ Miscellaneous

- Bump package version from 0.2.3 to 0.2.4

### 🔨 Refactor

- Extract demo mode into dedicated demo module

## [0.2.3] - 2026-07-24

### 🚀 Features

- Add activity badge to Context Inspector with configurable thresholds
- **sort**: Add updated_at field with auto-sort by most recently updated MR

### 🐛 Bug Fixes

- **notifications**: Only notify on branch status change, not on every refresh

### ⚙️ Miscellaneous

- Update README to track Crates.io data

### 📚 Documentation

- Regenerate demo.gif with updated sort and activity badges

## [0.2.0] - 2026-07-22

### 🚀 Features

- **gitlab-tracker**: Update release.yml to publish package to crates.io
- **gitlab-tracker**: Make auto-refresh interval configurable via config and env
- **gitlab-tracker**: Update release.yml to publish package to crates.io
- **gitlab-tracker**: Update README
- **gitlab-tracker**: Init commit


