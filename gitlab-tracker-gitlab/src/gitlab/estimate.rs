//! How many requests the next refresh sends, per MR (status bar counters).

use super::*;

/// Estimated number of GitLab API HTTP calls that will be fired for a single MR fetch,
/// broken down by category so the UI can display a meaningful counter.
///
/// The estimate mirrors the exact branching logic of `fetch_gitlab_data` and its
/// sub-functions (`fetch_notes_count`, `fetch_diff_stats`, `fetch_pipelines`).
/// Adding a new network call inside those functions should be reflected here.
#[derive(Debug, Clone, Default)]
pub struct ApiCallEstimate {
    /// `GET /merge_requests/:id`: only for an open MR that cannot merge as is (the
    /// others come from the shared list reads).
    pub base: usize,
    /// Discussions endpoint calls (always fired for open MRs, skipped for
    /// closed/merged with a cached non-zero count). Minimum 1 page per MR.
    pub discussions: usize,
    /// Branch-detection call: `GET /commits/:sha/refs` — only when a merge SHA
    /// is available (i.e. the MR is confirmed merged).
    pub refs: usize,
    /// Changes + commits endpoint calls: fired when `updated_at` changed or when
    /// the diff-stats cache is missing / corrupted.
    /// 1 call for the changes endpoint + at least 1 for commits pagination.
    pub diff_stats: usize,
    /// Pipeline list call (1) + job calls (1 per pipeline, up to 5).
    /// Fired when cache is missing, stale, or any pipeline is in a transient state.
    pub pipelines: usize,
    /// Calls this MR no longer makes compared with reading it alone (its own page,
    /// and an open MR's discussions and approvals at every refresh). Not in
    /// `total`; the list reads replacing the pages are counted by the caller.
    pub saved: usize,
}

impl ApiCallEstimate {
    /// Total estimated HTTP calls for this MR.
    pub fn total(&self) -> usize {
        self.base + self.discussions + self.refs + self.diff_stats + self.pipelines
    }
}

/// Implemented by any type that can predict its own API call footprint before
/// the actual network requests are fired.
///
/// This trait is the extension point: if a new fetch function is added to
/// `fetch_gitlab_data`, implement (or update) this trait so the counter stays
/// accurate automatically.
pub trait CountApiCalls {
    /// Returns the estimated number of HTTP calls that will be made when fetching
    /// data for an MR with this cached state.
    ///
    /// `state` is the current GitLab MR state (`Opened`, `Merged`, `Closed`).
    /// `has_merge_sha` indicates whether a merge/squash SHA is already known
    /// (determines whether the `/refs` branch-detection call is needed).
    fn estimate(
        &self,
        state: &crate::models::GitlabMrState,
        has_merge_sha: bool,
    ) -> ApiCallEstimate;
}

/// Implemented on the tracked MR itself: the estimate only reads its cache-relevant
/// fields, so no `CachedMrData` (with cloned description, pipelines, …) is needed.
/// Counts added and deleted lines in a unified patch.
///
/// `+++` / `---` are file headers only before the first `@@` hunk (GitLab's `diff`
/// field usually has none); inside hunks they are real changes such as `++i` or a
/// SQL `-- comment`, which a plain prefix filter would drop.
pub fn count_diff_lines(diff: &str) -> (u32, u32) {
    let (mut added, mut deleted, mut in_hunk) = (0, 0, false);
    for line in diff.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
        } else if !in_hunk {
            continue;
        } else if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('-') {
            deleted += 1;
        }
    }
    (added, deleted)
}

/// Whether cached diff stats can be reused (while `updated_at` is unchanged).
/// Shared by the fetcher and the API-call estimate so they never disagree.
///
/// Rejects entries written by older cache formats:
/// - `files_changed > 0` with no added/removed line: fetched through the old
///   `added_lines` / `removed_lines` fields, which do not exist in the GitLab API;
/// - `files_changed > 0` with 0 commits: written before `commits_count` existed
///   (serde default), since a real MR with changes always has at least one commit.
/// - a commit list that does not match `commits_count`: written before the MR
///   commits were cached (they are needed to recognise copies on other branches).
pub(super) fn diff_stats_cache_valid(
    stats: Option<&crate::models::DiffStats>,
    commits: &[MrCommit],
) -> bool {
    stats.is_some_and(|s| {
        let lines_ok = s.files_changed == 0 || s.additions > 0 || s.deletions > 0;
        let commits_ok = s.commits_count > 0 || s.files_changed == 0;
        let list_ok = s.commits_count as usize == commits.len();
        lines_ok && commits_ok && list_ok && s.files_listed
    })
}

/// Whether cached pipelines can be reused (while `updated_at` is unchanged).
/// Shared by the fetcher and the API-call estimate.
///
/// Re-fetch when the cache is empty, when no pipeline has jobs or one lacks
/// `created_at` (older state files), and always while a pipeline is transient:
/// it can finish without the MR's `updated_at` changing.
pub(super) fn pipelines_cache_reusable(pipelines: &[Pipeline]) -> bool {
    use crate::models::PipelineState::{Created, Pending, Running};
    !pipelines.is_empty()
        && pipelines.iter().any(|p| !p.jobs.is_empty())
        && pipelines.iter().all(|p| p.created_at.is_some())
        && !pipelines
            .iter()
            .any(|p| matches!(p.status, Running | Pending | Created))
}

/// Cache snapshot of a tracked MR handed to the fetcher, so it can skip pipeline,
/// diff and notes re-fetches when the MR has not changed since the last cycle.
impl From<&crate::models::TrackedMr> for CachedMrData {
    fn from(mr: &crate::models::TrackedMr) -> Self {
        Self {
            title: Some(mr.title.clone()),
            description: Some(mr.description.clone()),
            author: Some(mr.author.clone()),
            web_url: Some(mr.web_url.clone()),
            labels: Some(mr.labels.clone()),
            updated_at: mr.updated_at.clone(),
            pipelines: mr.pipelines.clone(),
            diff_stats: mr.diff_stats.clone(),
            user_notes_count: mr.user_notes_count,
            // Lets the fetcher detect Open → Merged transitions and invalidate the
            // notes cache accordingly.
            cached_state: Some(mr.state),
            // Callers needing a forced re-sync override this (e.g. [R] sets ForceAll).
            cache_policy: CachePolicy::Normal,
            commits: mr.commits.clone(),
            sha: mr.sha.clone(),
            landed_fingerprint: mr.landed_fingerprint.clone(),
            branches: match &mr.status {
                crate::models::MrStatus::MergedIn(found) => found.clone(),
                _ => HashSet::new(),
            },
            threads: mr.threads.clone(),
            review: mr.review.clone(),
            approved_by: mr.approved_by.clone(),
            approvers: mr.approvers.clone(),
            // Unknown until the first fetch of the session (reset at startup).
            mergeability: (mr.mergeability != MergeabilityStatus::Unknown)
                .then_some(mr.mergeability),
            review_read_at: mr.review_read_at.clone(),
            notes_seen: mr.notes_seen,
            milestone: mr.milestone.clone(),
            milestone_events: mr.milestone_events.clone(),
        }
    }
}

impl CountApiCalls for crate::models::TrackedMr {
    fn estimate(
        &self,
        state: &crate::models::GitlabMrState,
        has_merge_sha: bool,
    ) -> ApiCallEstimate {
        let open = *state == crate::models::GitlabMrState::Opened;

        // ── 1. Base MR read — shared list reads (counted once per cycle by the
        //       caller, `MRS_PER_LIST` MRs each); its own page only for an open MR
        //       that cannot merge as is (`needs_single_read`) ─────────────────────
        let base = usize::from(open && self.mergeability != MergeabilityStatus::Mergeable);

        // ── 2. Discussions (+ approvals). Open MRs: read again when the list shows
        //       a change or the last read is older than `REVIEW_MAX_AGE` — assumed
        //       unchanged otherwise, like the other caches. Closed / merged: once.
        // We pessimistically assume 1 page (the common case). Extra pages are
        // rare and impossible to predict without knowing the actual count.
        // A timeline read an older way is read again once (`review_current`).
        let review_known = self
            .review
            .as_ref()
            .is_some_and(|r| r.version >= crate::models::REVIEW_ACTIVITY_VERSION);
        let review_recent = review_known
            && self
                .review_read_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| chrono::Utc::now().signed_duration_since(at) < REVIEW_MAX_AGE);
        let discussions = if open {
            if review_recent {
                0
            } else {
                2 // One discussions page + the approvals call.
            }
        } else if review_known {
            0 // Served from cache — no network call.
        } else {
            1 // At least one page fetched.
        };
        // Before list reads and review caching, every MR read its own page and an
        // open one its discussions and approvals at every refresh.
        let saved = (1 - base) + if open { 2 - discussions } else { 0 };
        // The milestone history: read once, then again only when the milestone
        // changes (counted with the discussions, the other timeline read).
        let discussions = discussions + usize::from(self.milestone_events.is_none());

        // ── 3. Branch detection via /refs — only when a merge SHA is present,
        //       plus the one-off fetch of the merge / squash commit fingerprint ──
        let landed_fetch = usize::from(self.sha.is_some() && self.landed_fingerprint.is_none());
        let refs = usize::from(has_merge_sha) + landed_fetch;

        // ── 4. Diff stats: changes + commits pagination ───────────────────────
        // Mirrors the cache-validity check in fetch_gitlab_data:
        //   • Skip when updated_at is unchanged AND the cached stats are valid.
        //   • \"Valid\" means lines_ok && commits_ok (matches `cached_diff_stats_valid`).
        let diff_stats_cached_valid =
            diff_stats_cache_valid(self.diff_stats.as_ref(), &self.commits);
        let diff_stats = if self.updated_at.is_some() && diff_stats_cached_valid {
            // Cache hit — no calls needed.
            // (The real guard also checks updated_at == fresh updated_at, which we
            // cannot know before the base fetch. We optimistically assume it matches.)
            0
        } else {
            // 1 call for /changes + 1 call for /commits (first page).
            // Additional commit pages are rare and not predictable ahead of time.
            2
        };

        // ── 4b. commits_behind: /compare fallback ────────────────────────────
        // `diverged_commits_count` is returned for free in the base MR response
        // (via `include_diverged_commits_count=true` — no extra call).
        // The `/compare` fallback fires only when GitLab omits that field for a
        // non-Mergeable open MR. We cannot know ahead of time whether GitLab will
        // provide the field, but we can use the cached `commits_behind` value as a
        // signal: if it was already resolved last cycle (Some(_)), the field was
        // likely provided — no fallback call expected. If it is None for an open
        // non-merged MR, assume the fallback may fire (conservative +1).
        let compare_fallback = if *state == crate::models::GitlabMrState::Opened {
            let already_resolved = self
                .diff_stats
                .as_ref()
                .is_some_and(|s| s.commits_behind.is_some());
            if already_resolved {
                0
            } else {
                1
            }
        } else {
            0 // Merged / Closed — commits_behind is not fetched at all.
        };

        // ── 5. Pipelines: list + per-pipeline job fetches ─────────────────────
        // Mirrors the cache-validity logic in fetch_gitlab_data:
        //   • Skip when updated_at unchanged, pipelines cached, all have jobs and
        //     dates, and none is in a transient state.
        let pipelines_cache_hit =
            self.updated_at.is_some() && pipelines_cache_reusable(&self.pipelines);

        let pipelines = if pipelines_cache_hit {
            0
        } else {
            // 1 call for the pipeline list (up to 5 results) +
            // 1 call per pipeline for its job list.
            // We use the cached pipeline count as the best available estimate;
            // when the cache is empty we assume a conservative 1 pipeline.
            let pipeline_count = if self.pipelines.is_empty() {
                1
            } else {
                self.pipelines.len()
            };
            1 + pipeline_count
        };

        ApiCallEstimate {
            base,
            discussions,
            refs,
            // compare_fallback is bundled into diff_stats: same category (diff enrichment),
            // and avoids adding a dedicated field to ApiCallEstimate for a rare edge case.
            diff_stats: diff_stats + compare_fallback,
            pipelines,
            saved,
        }
    }
}
