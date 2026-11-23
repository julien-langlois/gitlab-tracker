//! *Internal crate of [gitlab-tracker](https://crates.io/crates/gitlab-tracker),
//! released with it under the same version: its API may change in any release.*
//!
//! The merge-request model of gitlab-tracker and the GitLab client that fills it:
//!
//! - [`models`] — an MR as tracked (its GitLab data, its presence on each branch,
//!   pipelines, threads, review activity) and as saved;
//! - [`gitlab`] — the REST client: MR lists and details, branch scans, pipelines,
//!   paced per instance;
//! - [`gitlab_graphql`] — the GraphQL batches that replace many REST reads;
//! - [`sentinel`] — the cheap poll that tells which MRs changed between two refreshes;
//! - [`signatures`], [`gitlab_signatures`] — signed-commit checks and their reads;
//! - [`history`] — what a project merged over a period, kept between launches;
//! - [`snooze`] — snoozing an MR until a moment or a change.

pub mod events;
pub mod gitlab;
pub mod gitlab_graphql;
pub mod gitlab_signatures;
pub mod history;
pub mod models;
pub mod sentinel;
pub mod signatures;
pub mod snooze;

// The translation hook, under the path the modules use (`crate::i18n::tl`).
use gitlab_tracker_core::i18n;

pub use events::GitlabEvent;
pub use gitlab::is_bot;
