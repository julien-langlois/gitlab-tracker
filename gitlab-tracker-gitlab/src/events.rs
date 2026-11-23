//! What the GitLab tasks of this crate report when they finish. They send to any
//! channel whose event type converts from [`GitlabEvent`]: the app maps each one
//! to its own event, so this crate knows nothing of the app's.

use crate::models::{BranchCommits, GitLabLabelDetail, GitLabMilestone, MrLoadedData};

/// An outcome of a GitLab task (`spawn_*`).
pub enum GitlabEvent {
    /// Recent commits of `branch`, scanned to recognise MRs copied there under
    /// other SHAs (`None`: the scan failed, the previous result is kept).
    BranchCommitsLoaded {
        branch: String,
        commits: Option<BranchCommits>,
        /// What was read: the index to keep (`Full`) or commits to add (`Range`).
        scan: crate::gitlab::BranchScan,
        /// The branch head after this scan (the next scan reads from it).
        head: Option<String>,
    },
    /// The project label list (with colours).
    GitlabLabelsLoaded(Vec<GitLabLabelDetail>),
    /// The MR IDs linked to a milestone.
    MilestoneMrsLoaded {
        milestone_title: String,
        mr_ids: Vec<String>,
    },
    /// The milestone list.
    MilestonesLoaded(Vec<GitLabMilestone>),
    /// An MR could not be read.
    MrFailed {
        id: String,
        error: crate::gitlab::GitlabError,
    },
    /// An MR was read.
    MrLoaded(Box<MrLoadedData>),
    /// GitLab is still computing an MR's mergeability: the fetcher retries.
    MrMergeabilityRetrying { id: String },
    /// A discovery poll: `mr_ids` are the IIDs not yet tracked (possibly none),
    /// `next_anchor` the advanced `created_after` anchor.
    NewMrsDiscovered {
        mr_ids: Vec<String>,
        next_anchor: String,
    },
    /// A sentinel check came back (`None`: a read failed), see [`crate::sentinel`].
    SentinelChecked(Option<Box<crate::sentinel::SentinelReport>>),
    /// Settled MRs (merged on every tracked branch, left out of refresh cycles)
    /// that changed on GitLab since their last read: to refresh.
    SettledMrsChanged(Vec<String>),
    /// What was read to check an MR's commit signatures
    /// ([`crate::gitlab_signatures::spawn_signature_check`]).
    SignaturesChecked {
        mr_id: String,
        data: Box<crate::signatures::SignatureData>,
    },
}

/// Sends `event` as the channel's own event type; a closed channel (the app is
/// quitting) is not an error.
pub fn emit<E: From<GitlabEvent>>(tx: &tokio::sync::mpsc::UnboundedSender<E>, event: GitlabEvent) {
    let _ = tx.send(event.into());
}
