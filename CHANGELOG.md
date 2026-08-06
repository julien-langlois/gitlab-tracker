# CHANGELOG

All notable changes to this project will be documented in this file.

## [0.3.3] - 2026-08-05

### 🚀 Features

- **ui**: Add quit confirmation on Esc key press
- **filter**: Replace cycle with interactive picker (state, mergeability, notes, milestone, assignee)
- Use GitLab label colours as fallback for unoverridden chips
- **ui**: Add split tracker pane with mouse-hover focus
- **redmine**: Add data from Redmine in side panel
- **redmine**: Auto-update ETC on time entry submission

### 🐛 Bug Fixes

- **ui**: Replace ANSI DarkGray with absolute RGB colours for consistent readability

### 📚 Documentation

- Use GitLab label colours as fallback for unoverridden chips

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

### ⚙️ Miscellaneous

- Bump package version from 0.2.8 to 0.2.9
- **demo**: Update tape scenario to cover flagging, filter and column picker

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

### 🐛 Bug Fixes

- **ui**: Enforce fixed-width centered state badges in table

## [0.2.5] - 2026-07-27

### 🚀 Features

- **ui**: Animate mergeability badge on open MR status column
- **ui**: Add MR lifecycle state badge to table and inspector

### 🐛 Bug Fixes

- **ui**: Remove redundant [Open] title prefix now superseded by state badge

### ⚙️ Miscellaneous

- Bump package version from 0.2.4 to 0.2.5
- Exclude assets directory from crate and release packages

### 🔨 Refactor

- Extract key and mouse event handling into dedicated events module

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


