# gitlab-tracker-gitlab

[![Crates.io Version](https://img.shields.io/crates/v/gitlab-tracker-gitlab)](https://crates.io/crates/gitlab-tracker-gitlab)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **Internal crate.** Published on crates.io so that `cargo install gitlab-tracker`
> can build; it is part of the application, released with it under the same
> version, and its API may change in any release — see
> [Stability](https://github.com/julien-langlois/gitlab-tracker/blob/main/docs/stability.md#what-is-not).

The merge-request model of [gitlab-tracker](../README.md) and the GitLab client that fills it: MR lists and details over REST and GraphQL (paced per instance), branch scans that recognise MRs copied under other SHAs, the sentinel that tells which MRs changed between two refreshes, signed-commit checks, merge history.
