//! What a fetch needs (URL, token, project) and what it may reuse from the last one.

use super::*;

/// The GitLab API token: a single zeroized-on-drop copy shared by every fetch
/// (cloning a `FetchContext` only bumps the reference count).
pub type ApiToken = std::sync::Arc<zeroize::Zeroizing<String>>;

#[derive(Clone)]
pub struct FetchContext {
    pub base_url: String,
    pub token: ApiToken,
    pub project_id: String,
    pub branches: Vec<String>,
}

#[derive(Clone, Default)]
pub struct CachedMrData {
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,

    pub web_url: Option<String>,
    pub labels: Option<Vec<String>>,
    /// Last known `updated_at` timestamp — used to skip pipeline re-fetch when
    /// the MR has not changed since the previous refresh cycle.
    pub updated_at: Option<String>,
    /// Pipelines from the previous fetch — reused when `updated_at` is unchanged.
    pub pipelines: Vec<Pipeline>,
    /// Diff stats from the previous fetch — reused when `updated_at` is unchanged.
    pub diff_stats: Option<crate::models::DiffStats>,
    /// Human note count from the previous fetch — reused when `updated_at` is unchanged.
    pub user_notes_count: u32,
    /// GitLab state at the time the cache was last populated.
    ///
    /// Used to detect state transitions (e.g. `Opened` → `Merged`) so that any
    /// cache entry built while the MR was open is unconditionally invalidated on
    /// the first refresh after the transition.
    pub cached_state: Option<crate::models::GitlabMrState>,
    /// Declares which cached fields must be bypassed on this fetch cycle.
    ///
    /// The default (`CachePolicy::Normal`) respects all existing cache guards.
    /// Callers that need to force a full re-sync (e.g. manual `[R]`, post-merge
    /// housekeeping) set this to `CachePolicy::ForceAll` or a targeted variant.
    pub cache_policy: CachePolicy,
    /// MR commits from the previous fetch — reused with the diff stats.
    pub commits: Vec<MrCommit>,
    /// Merge / squash commit SHA and its fingerprint from the previous fetch.
    pub sha: Option<String>,
    pub landed_fingerprint: Option<CommitFingerprint>,
    /// Branches the MR was found on: kept when branch detection fails transiently.
    pub branches: HashSet<String>,
    /// Discussions-derived data and approvers, reused with `user_notes_count`.
    pub threads: Vec<MrThread>,
    pub review: Option<ReviewActivity>,
    pub approved_by: Vec<String>,
    pub approvers: Vec<String>,
    /// Mergeability at the previous fetch (`None`: unknown): a change triggers a
    /// fresh read of the approvals.
    pub mergeability: Option<MergeabilityStatus>,
    /// When discussions and approvals were last read, and GitLab's note count then.
    pub review_read_at: Option<String>,
    pub notes_seen: Option<u32>,
    /// Milestone at the previous fetch and its history: the history is read
    /// again only when the milestone changed (`milestone_events_due`).
    pub milestone: Option<String>,
    pub milestone_events: Option<Vec<crate::models::MilestoneEvent>>,
}

/// Controls which cached fields are bypassed during a fetch cycle.
///
/// Extend this enum whenever a new independently-cacheable field is added to
/// `CachedMrData`, rather than adding a dedicated `force_*` boolean each time.
#[derive(Clone, Default, PartialEq, Eq)]
pub enum CachePolicy {
    /// Respect all existing cache guards — normal periodic refresh behaviour.
    #[default]
    Normal,
    /// Bypass every cache guard and re-fetch all fields from the GitLab API.
    ///
    /// Used by the manual `[R]` refresh so the user always gets a fully
    /// up-to-date snapshot, even for already-merged MRs.
    ForceAll,
    /// The sentinel saw a sign GitLab does not show in the MR's list entry: a
    /// comment or an approval (`review`), a pipeline that moved (`pipelines`).
    Signals { review: bool, pipelines: bool },
}

impl CachePolicy {
    /// Returns `true` when the notes cache must be bypassed.
    pub fn notes_stale(&self) -> bool {
        matches!(
            self,
            CachePolicy::ForceAll | CachePolicy::Signals { review: true, .. }
        )
    }

    /// Returns `true` when the cached pipelines must be read again.
    pub fn pipelines_stale(&self) -> bool {
        matches!(
            self,
            CachePolicy::Signals {
                pipelines: true,
                ..
            }
        )
    }
}
