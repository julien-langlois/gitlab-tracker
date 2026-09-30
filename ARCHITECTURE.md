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

---

## Crate boundaries

```text
gitlab-tracker          (binary — TUI, UI, event loop, desktop notifications)
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
