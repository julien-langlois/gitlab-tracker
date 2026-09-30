# Architecture internals

> This document is intended for **contributors**. End-user documentation lives in [`README.md`](README.md).

---

## Event bus

All state mutations flow through a single `tokio::mpsc::unbounded_channel::<AppEvent>`.
No code outside of `app::apply_event` is allowed to mutate `app.mrs` directly.

```text
User input / timer tick
    │
    ▼
events.rs  ──tx.send(AppEvent::*)──►  main.rs event loop
                                            │
                                            ▼
                                    app.apply_event()
                                            │
                                   consults event_policy
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

## Crate boundaries

```text
gitlab-tracker          (binary — TUI, UI, event loop)
    │  uses
    ▼
gitlab-tracker-core     (library — domain traits, zero UI dependency)
    │  re-exports
    ├── TrackerProvider / LinkedTicket
    ├── FilterDef / ColumnDef / ShortcutBlock
    └── …

gitlab-tracker-notify   (library — desktop notifications, optional feature flag)
gitlab-tracker-redmine  (library — Redmine tracker plugin, optional feature flag)
```

**Rule:** `gitlab-tracker-core` must never depend on `ratatui`, `crossterm`, or any
TUI crate. It is the boundary that keeps domain logic testable in isolation.
