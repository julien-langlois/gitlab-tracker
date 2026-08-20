# gitlab-tracker-notify

Optional desktop notification plugin for [gitlab-tracker](../README.md).

Powered by [`notify-rust`](https://crates.io/crates/notify-rust), it surfaces MR state changes as native OS notifications. Build without the `desktop` feature for a zero-dependency stub suitable for headless or CI environments.

---

## Events

Nine events trigger a desktop notification:

**GitLab MR events**

| Event | Trigger condition |
| :--- | :--- |
| 🌿 **New branch detected** | An MR's commit SHA was found on a branch not present in the last persisted state |
| 🕐 **MR updated** | `updated_at` from GitLab differs from the previously stored value |
| 🔀 **Mergeability changed** | The mergeability status transitioned (e.g. `Mergeable → Conflict`) |
| 🏁 **Milestone changed** | The milestone attached to the MR changed (e.g. `v2.4.0 → v2.5.0`) |
| ⚠️ **Complexity changed** | The review complexity category crossed a boundary (e.g. `🟢 EASY → 🔴 COMPLEX`) — uses `dialog-warning` icon |

**Tracker ticket events** *(requires an active tracker plugin, e.g. Redmine)*

Each notification shows the changed field with a clear **before → after** format.

| Event | Trigger condition |
| :--- | :--- |
| 🔴 **Priority changed** | Ticket priority transitioned (e.g. `Normal → High`) — uses `dialog-warning` icon |
| 🔄 **Status changed** | Ticket status transitioned (e.g. `In Progress → Resolved`) |
| 👤 **Assignee changed** | Ticket assignee changed (e.g. `Alice → Bob`, or `→ Unassigned`) |
| 📦 **Version changed** | Target version/release changed (e.g. `v1.2 → v1.3`, or `→ None`) |
| 📊 **Progress changed** | Completion percentage changed in either direction (e.g. `50% → 75%` or `75% → 50%`) |

Tracker ticket notifications open the **ticket URL** (not the MR) when clicked.

---

## Clickable notifications

Each notification includes an **"Open MR"** action button. Clicking it opens the MR directly in your default web browser — no need to switch to the terminal first. The URL is resolved from the `web_url` field returned by the GitLab API.

> **Platform support:** the click-to-open behaviour relies on D-Bus actions on Linux (GNOME, KDE, etc.) and the system `open` command on macOS. On environments without a notification daemon the click action is silently ignored.

---

## Anti-spam on startup

Change notifications (`updated_at`, mergeability, milestone) are **suppressed during the initial sync** — the first fetch cycle after launch. This prevents a flood of toasts when the app starts and reconciles its in-memory state with the GitLab API. Only genuine changes detected during subsequent background refreshes (or a manual `R` refresh) will produce notifications.

* ✅ **No duplicate alerts** when restarting the app with an unchanged state.
* ✅ **No spam** during the initial sync or redundant refresh cycles.
* ✅ **Reliable detection** of real changes across refreshes and restarts.
* ✅ **One click to open** the MR in your browser directly from the notification.

The last-known branch state per MR is persisted in `tracker_state.json` under the `last_known_branches` key.

---

## Feature flags

| Feature flag | Default | Effect |
| :--- | :--- | :--- |
| `desktop` | ✅ enabled | Enables `notify-rust` and `open` dependencies — produces real OS notifications |

Build without desktop notifications (headless / CI environments):

```bash
cargo build --release --no-default-features
```
