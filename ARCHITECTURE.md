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

## MR lifecycle events

`AppEvent` is the **transport layer** — it carries the raw data needed by the UI.
`MrLifecycleEvent` (defined in `gitlab-tracker-core`) is the **domain layer** — it names
the scenario without any UI concern.

### Mapping

| `AppEvent` variant         | `MrLifecycleEvent`             |
| :------------------------- | :----------------------------- |
| `MrAdded`                  | `Added`                        |
| `MrRemovedByIndex`         | `Deleted`                      |
| `MrRemovedById`            | `Deleted`                      |
| `MrLoaded`                 | `Refreshed`                    |
| `MrFailed`                 | `FetchFailed`                  |
| `MilestoneMrsLoaded` (MRs) | `Added` (per MR)               |
| *(merged/closed detected)* | `Merged` / `Closed` *(future)* |

The bridge is `AppEvent::as_lifecycle_event() -> Option<MrLifecycleEvent>` in `models.rs`.

### Policy

`MrEventPolicy` (trait, `gitlab-tracker-core/src/lifecycle.rs`) answers four questions
for any given lifecycle event:

| Method          | Question                                              |
| :-------------- | :---------------------------------------------------- |
| `needs_refetch` | Should a new GitLab API call be spawned?              |
| `should_remove` | Should the MR be dropped from the tracked list?       |
| `should_notify` | Should a desktop notification be surfaced?            |
| `needs_persist` | Should the state be written to disk after this event? |

#### Default policy (`DefaultMrEventPolicy`)

| Event         | `needs_refetch` | `should_remove` | `should_notify` | `needs_persist` |
| :------------ | :-------------- | :-------------- | :-------------- | :-------------- |
| `Added`       | ✅              | ❌              | ❌              | ✅              |
| `Deleted`     | ❌              | ✅              | ❌              | ✅              |
| `Refreshed`   | ❌              | ❌              | ❌              | ✅              |
| `Merged`      | ❌              | ❌              | ✅              | ✅              |
| `Closed`      | ❌              | ❌              | ✅              | ✅              |
| `FetchFailed` | ❌              | ❌              | ✅              | ❌              |

#### Swapping the policy

`App::event_policy` is an `Arc<dyn MrEventPolicy>`. To inject a custom policy
(e.g. suppress all persists in demo mode, or always refetch on close):

```rust
app.event_policy = Arc::new(MyCustomPolicy);
```

No changes to `apply_event` are needed — the policy is the only place where
these rules live.

---

## Crate boundaries

```text
gitlab-tracker          (binary — TUI, UI, event loop)
    │  uses
    ▼
gitlab-tracker-core     (library — domain traits, zero UI dependency)
    │  re-exports
    ├── MrLifecycleEvent
    ├── MrEventPolicy / DefaultMrEventPolicy
    ├── TrackerProvider / LinkedTicket
    ├── FilterDef / ColumnDef / ShortcutBlock
    └── …

gitlab-tracker-notify   (library — desktop notifications, optional feature flag)
gitlab-tracker-redmine  (library — Redmine tracker plugin, optional feature flag)
```

**Rule:** `gitlab-tracker-core` must never depend on `ratatui`, `crossterm`, or any
TUI crate. It is the boundary that keeps domain logic testable in isolation.
