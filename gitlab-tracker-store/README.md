# gitlab-tracker-store

[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker-store)](https://crates.io/crates/gitlab-tracker-store)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **Internal crate.** Published on crates.io so that `cargo install gitlab-tracker`
> can build; it is part of the application, released with it under the same
> version, and its API may change in any release — see
> [Stability](https://github.com/julien-langlois/gitlab-tracker/blob/main/docs/stability.md#what-is-not).

The local state of [gitlab-tracker](../README.md) in SQLite (`tracker.db`): the tracked MRs of every project, their day-to-day history (`mr_events`), the project caches and the window's settings. Schema migrations are versioned with `PRAGMA user_version` (`migrations/`), forward only; a database written by a newer version is refused, never written to.
