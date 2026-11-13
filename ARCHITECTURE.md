# Architecture internals

> This document is intended for **contributors**. End-user documentation lives in [`README.md`](README.md).

---

## Event bus

All state mutations flow through a single `tokio::mpsc::unbounded_channel::<AppEvent>`.
No code outside of `app::apply_event` is allowed to mutate `app.mrs` directly.

`apply_event` is a dispatcher; the per-concern logic lives in `app/`:

| Module | Role |
| :--- | :--- |
| `app/mr_changes.rs` | Change detection on MR loads (branches, `updated_at`, mergeability, milestone, complexity) → log + notifications |
| `app/tracker_sync.rs` | Linked-ticket fetches, automatic status transitions, TimeLog cache |
| `app/stats_recorder.rs` | Stats snapshots on MR loads (`stats` feature) |
| `app/stats_view.rs` | Stats overlay state: tab, window, last report, generation guard against late responses |

Renderer-written state (pane areas, scroll offsets, spinner frame) is grouped in
`App::layout` (`UiLayout` / `PaneScroll`): the renderer writes it, key handlers read it.

The demo mode (`--demo`) runs through the same key handler, with network actions
disabled (read-only).

```text
User input / timer tick
    │
    ▼
events.rs  ──tx.send(AppEvent::*)──►  main.rs event loop
                                            │
                                            ▼
                                    app.apply_event()
                                            │
                              ┌─────────────┴──────────────┐
                              ▼                            ▼
                     mutates App state           spawns async tasks
                     returns needs_persist        (fetch, notify, …)
```

---

## Persistence after events

`apply_event` returns whether the state file must be rewritten. The rule lives in
`AppEvent::persists_state()` (`models.rs`): adding, removing or loading a MR changes
the tracked list or its data; a failed fetch does not (errors are never saved).
The main loop saves once per drained batch of events, not once per event.

The previous `MrLifecycleEvent` / `MrEventPolicy` indirection (a trait with a single
implementation, half of whose methods were never called) was removed in the lot 4
refactor.

---

## Data model

`MrData` (`models.rs`) is the single description of a MR's GitLab data. `SavedMr`
(state file) embeds it with `#[serde(flatten)]`, and `TrackedMr` (runtime) exposes it
through `Deref`, so a new field is declared once. Fetch failures are kept in memory
only, as `MrStatus::Error(String)`, and shown in the Inspector.

**Review data** comes with the discussions fetch (`gitlab::fetch_discussions`, no
extra call): `MrData::threads` (resolvable threads, `review_threads`) and
`MrData::review` (`ReviewActivity`: first review, approval, review requests, last
push, last activity per user — `review_activity`, which matches GitLab's English
system notes). `MrData::approved_by` comes from `/approvals`, refetched while the
MR is open. `review: None` means "never fetched" and drives the discussions cache.

**Branch presence** has two sources. `MrStatus::MergedIn` holds the branches that
contain the MR's merge SHA (GitLab `/refs`, `gitlab::branches_containing`; a
transient failure keeps the previous set). For the other branches,
`App::branch_commits` holds one `BranchCommits` index per branch (SHAs, `-x`
trailer sources, `CommitFingerprint`s), rebuilt by `spawn_branch_scans` after each
startup load / refresh cycle and never persisted. `BranchCommits::presence_of`
matches an MR's merge / squash commit (`MrData::sha` + `landed_fingerprint`) and
then its own `MrData::commits` against it. `App::branch_presence` combines both,
and every consumer (table, export, `absent_branches` filter, notifications) goes
through it.

With `stats`, the first arrival of an MR on each branch other than its target is
recorded (`app/stats_recorder.rs`): same-SHA arrivals from `MrLoaded` (branches new
since `last_known_branches`, see `reached_branches`), copies from
`BranchCommitsLoaded` (dated by `BranchCommits::copied_at`). Branches added in the
session are ignored until every MR has been re-fetched (`just_added_branches`).

---

## Error handling

| Where | Type | Rule |
| :--- | :--- | :--- |
| Libraries (`core`, `redmine`, `stats`) | `thiserror` enums: `TrackerError`, `SetupError`, `StatsError` | Typed variants callers can match (`Auth`, `NotFound`, `Network`…); never `Result<_, String>`. |
| Binary, fallible I/O | `thiserror` enums: `GitlabError` (MR fetch), `DurationError` (Log Time input) | Same rule; the `Display` message is what the UI shows. |
| `AppEvent` | Carries the typed error (`MrFailed { error: GitlabError }`, `Result<_, TrackerError>`, `Result<_, StatsError>`) | Conversion to a display `String` happens once, in `apply_event`, when it lands in UI state (`MrStatus::Error`, `activities_error`, `TimeLogState::Failed`…). |
| `main`, `cli`, `demo` | `anyhow::Result` + `.context(…)` | Only at the top, where errors are reported and the process exits. |
| Best-effort enrichments (diff stats, pipelines, notes count, milestones) | `Option` / empty `Vec` + `tracing::warn!` | A missing enrichment must not fail the MR row. |

---

## Desktop window (`gui` feature)

The window (`gitlab-tracker/src/gui/`) is a second frontend over the same `App`:
no state of its own beyond what the views need (page, panels, text being edited).

* **Same behaviour as the TUI.** Clicks and shortcuts become `Action`s handled by
  `actions::dispatch`, exactly like the TUI's keys; popups follow
  `App::input_mode`. Behaviour belongs in `App` or `dispatch`, never in a view.
* **Frames.** egui redraws on input and on events only: `forward_events` relays
  the session channel and wakes the window per event (the 1 s `Tick` included,
  for the countdown). Each frame applies the pending events, snapshots the
  visible list (`App::begin_render_cache`), draws, then dispatches the actions
  recorded while drawing — views never mutate `App` directly.
* **Per-frame cost.** A frame must not grow with the number of MRs where it can
  avoid it: the table builds only the rows in view (`show_rows`), and
  animations repaint at a bounded rate (`widgets::spinner`, 10 fps) rather than
  at the display rate.
* **Never wait on the UI thread.** The state file is written by a
  `storage::StateWriter` (background, one write at a time, newest last, flushed
  on exit); a project switch runs `build_session` in a task and swaps the session
  in when it is ready. `block_on` is kept for short local steps (small TOML
  saves in `dispatch`).
* **Tabs.** Each open project is a `Session` of its own. The shown tab's state
  lives in `GuiApp`'s fields (so the views read `g.app` as before); the others
  are `Tab`s parked in `GuiApp::tabs`, swapped in by `GuiApp::activate`. Hidden
  tabs still apply their events every frame (`Tab::run_hidden`). Tabs load in
  the background (`load_project`); a project without its token comes back as
  `Loaded::NeedsTokens` and opens the project form (`gui/setup.rs`) instead of
  prompting: the window turns `secrets::set_interactive(false)` on at start.
* **On-demand reads.** What the refresh cycle does not keep is read when a view
  asks for it and cached per tab in `gui/details.rs`: an MR's diff, a job's log,
  and the MR page's data (`gitlab_detail.rs`: merge checks, approval rules, full
  threads, deployments), keyed by the MR's `updated_at` so a change reads it
  again. The demo serves canned answers (`demo::demo_*`).
* **Theme.** Colours come from `bevel-theme`; `gui/tones.rs` and `gui/colors.rs`
  map the app's states and the TUI palette onto it.

---

## GitLab and Redmine requests

What keeps the request count low (details, figures and the GraphQL roadmap in
[`GRAPHQL.md`](GRAPHQL.md)):

* **Batched refresh.** `gitlab::spawn_mr_fetches` reads the MRs of a cycle through
  `merge_requests?iids[]=` lists, then the pipelines due through one GraphQL query
  per batch (`gitlab_graphql`), then lets each MR read only what its cache cannot
  vouch for (`fetch_gitlab_data`, fed a `Prefetched`). A single added MR still goes
  through `spawn_mr_fetch`.
* **Review cache.** Discussions and approvals of an open MR are read again only on
  a sign of change in its list entry, and at least every `REVIEW_MAX_AGE`
  (`gitlab::review_stale`).
* **Incremental branch scans.** `gitlab::plan_branch_scan` reads `<old head>..<branch>`
  and `BranchCommits::absorb` merges it; a full scan rebuilds the index daily.
* **Tracker bulk reads.** A cycle hands every linked ticket to
  `TrackerProvider::fetch_tickets` at once (`app::spawn_tickets_fetch`).
* **Pacing per instance.** Every request goes through
  `gitlab_tracker_core::pacing::SendPaced`; projects on the same server share
  `pacing::permits` and the pauses a `429` sets.
* **Counter.** `App::recompute_api_call_estimate` mirrors these rules: requests of
  the next cycle and those saved (`saved_*`), shown in the status bar.

**Token permissions.** Every GitLab endpoint and GraphQL type the app reads is
listed with its fine-grained permission in [`docs/gitlab-token.md`](docs/gitlab-token.md#permissions).
A change that adds a GitLab call updates that table (and the short list in the
README) in the same commit.

---

## Code layout

```text
gitlab-tracker/                  # Binary crate — desktop window, terminal UI, event loop
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

---

## Crate boundaries

```text
gitlab-tracker          (binary — TUI, desktop window (feature `gui`), event loop, notifications)
    │  uses
    ▼
gitlab-tracker-core     (library — domain contracts, zero UI dependency)
    ├── provider: TrackerProvider / TicketTransitionProvider / TrackerError / LinkedTicket
    ├── domain:   GitlabMrState / MergeabilityStatus / PipelineState / Requirement
    ├── registries: FilterDef / ColumnDef / ShortcutBlock / ProjectSettingDef
    └── secrets (feature): env → keyring → prompt token resolution

gitlab-tracker-redmine  (library — Redmine tracker plugin, feature `redmine`)
gitlab-tracker-stats    (library — SQLite snapshots & analytics, feature `stats`)
```

The stats schema is versioned with `PRAGMA user_version`; migrations live in
`gitlab-tracker-stats/migrations/` and are embedded with `include_str!`. Add a new
numbered file and append it to `MIGRATIONS` in `db.rs` (the version is its index) — never edit a shipped migration.

**Rule:** `gitlab-tracker-core` must never depend on `ratatui`, `crossterm`, or any
TUI crate. It is the boundary that keeps domain logic testable in isolation.

---

## Releasing

Release notes come from the commits (Conventional Commits), through one
`cliff.toml` used in two places, so `CHANGELOG.md` and the GitHub release always
show the same text:

```text
cargo release minor                 # dry run: prints the section that will be added, writes nothing
cargo release minor --execute
  ├─ pre-release hook (gitlab-tracker/Cargo.toml):
  │    git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md
  ├─ commit "chore(release): vX.Y.Z" (version bumps + CHANGELOG.md), signed tag vX.Y.Z
  └─ push
release.yml (on the tag)
  ├─ github-release: git cliff --current --strip header → release body
  │                  (+ "Full Changelog" compare link to the previous tag)
  ├─ publish-crates
  └─ build-release: binaries attached to that release
```

* The commit subject **is** the release note line: write it for readers. Use
  `feat!:` / a `BREAKING CHANGE:` footer for incompatible changes (shown as
  `[**breaking**]`).
* `chore(release)` and `chore(changelog)` commits are skipped.
* Sections older than 0.5.0 were edited by hand; `--prepend` never rewrites them.
