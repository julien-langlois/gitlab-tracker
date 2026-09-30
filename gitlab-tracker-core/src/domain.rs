//! Domain enums shared by the orchestrator and plugins (filters, trackers, stats).
//!
//! They carry no UI dependency, so filter predicates can match on variants instead of
//! string literals: a typo is now a compile error, not a silently empty filter.

use serde::{Deserialize, Serialize};

/// Represents the GitLab-side lifecycle state of a merge request.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GitlabMrState {
    #[default]
    Opened,
    Merged,
    Closed,
}

/// Represents the mergeability status of an open merge request as reported by GitLab.
///
/// Only meaningful for MRs in the `Opened` state — ignored for Merged/Closed.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MergeabilityStatus {
    /// GitLab reports the MR can be merged cleanly.
    Mergeable,
    /// The MR has conflicts that must be resolved before merging.
    Conflict,
    /// The MR branch is behind the target branch and needs a rebase.
    NeedsRebase,
    /// The MR is not open (already merged or closed in GitLab).
    NotOpen,
    /// The MR is a draft — intentionally not ready to merge.
    Draft,
    /// There are unresolved discussion threads on the MR.
    DiscussionsNotResolved,
    /// A CI pipeline is required before this MR can be merged.
    CiMustPass,
    /// A CI pipeline is currently running.
    CiStillRunning,
    /// Required approvals are missing.
    NotApproved,
    /// A reviewer has explicitly requested changes before the MR can be merged.
    RequestedChanges,
    /// GitLab is still computing mergeability; the fetcher will retry within the current run.
    Retrying,
    /// GitLab did not return a resolved mergeability status after the bounded retry window.
    SyncFailed,
    /// Status not yet fetched, not applicable, or an unrecognised value.
    #[default]
    Unknown,
}

/// Lifecycle state of a GitLab pipeline run.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PipelineState {
    Created,
    Pending,
    Running,
    Success,
    Failed,
    Canceled,
    Skipped,
    #[serde(other)]
    #[default]
    Unknown,
}

impl GitlabMrState {
    /// Canonical lowercase name, as used by the GitLab API and the stats DB.
    pub fn as_str(self) -> &'static str {
        match self {
            GitlabMrState::Opened => "opened",
            GitlabMrState::Merged => "merged",
            GitlabMrState::Closed => "closed",
        }
    }
}

/// Runtime condition an optional column or filter depends on.
///
/// Checked by the orchestrator (`App::requirement_met`); entries whose requirement is
/// not met are hidden from the pickers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// A tracker provider (Redmine…) is configured.
    Tracker,
    /// `gitlab_username` is set for the project (the "me" filters).
    GitlabUsername,
}
