use crate::models::{
    AppEvent, DiffStats, GitLabCommit, GitLabLabelDetail, GitLabMilestone, GitLabMr, GitLabRef,
    GitlabMrState, MergeabilityStatus, MrLoadedData, Pipeline, PipelineJob,
};

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub const MAX_CONCURRENT_REQUESTS: usize = 3;

/// Estimated number of GitLab API HTTP calls that will be fired for a single MR fetch,
/// broken down by category so the UI can display a meaningful counter.
///
/// The estimate mirrors the exact branching logic of `fetch_gitlab_data` and its
/// sub-functions (`fetch_notes_count`, `fetch_diff_stats`, `fetch_pipelines`).
/// Adding a new network call inside those functions should be reflected here.
#[derive(Debug, Clone, Default)]
pub struct ApiCallEstimate {
    /// Always-present calls: `GET /merge_requests/:id` (1 per MR).
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

impl CountApiCalls for CachedMrData {
    fn estimate(
        &self,
        state: &crate::models::GitlabMrState,
        has_merge_sha: bool,
    ) -> ApiCallEstimate {
        // ── 1. Base MR fetch — always 1 call ─────────────────────────────────
        let base = 1;

        // ── 2. Discussions — always fired for open MRs; skipped for
        //       closed/merged when a non-zero count is already cached ──────────
        // We pessimistically assume 1 page (the common case). Extra pages are
        // rare and impossible to predict without knowing the actual count.
        let discussions =
            if *state != crate::models::GitlabMrState::Opened && self.user_notes_count > 0 {
                0 // Served from cache — no network call.
            } else {
                1 // At least one page fetched.
            };

        // ── 3. Branch detection via /refs — only when a merge SHA is present ──
        let refs = if has_merge_sha { 1 } else { 0 };

        // ── 4. Diff stats: changes + commits pagination ───────────────────────
        // Mirrors the cache-validity check in fetch_gitlab_data:
        //   • Skip when updated_at is unchanged AND the cached stats are valid.
        //   • \"Valid\" means lines_ok && commits_ok (matches `cached_diff_stats_valid`).
        let diff_stats_cached_valid = self.diff_stats.as_ref().is_some_and(|s| {
            let lines_ok = s.files_changed == 0 || s.additions > 0 || s.deletions > 0;
            let commits_ok = s.commits_count > 0 || s.files_changed == 0;
            lines_ok && commits_ok
        });
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
        let cached_has_jobs = self.pipelines.iter().any(|p| !p.jobs.is_empty());
        let cached_has_dates = self.pipelines.iter().all(|p| p.created_at.is_some());
        let cached_has_transient = self.pipelines.iter().any(|p| {
            matches!(
                p.status,
                crate::models::PipelineState::Running
                    | crate::models::PipelineState::Pending
                    | crate::models::PipelineState::Created
            )
        });
        let pipelines_cache_hit = self.updated_at.is_some()
            && !self.pipelines.is_empty()
            && cached_has_jobs
            && cached_has_dates
            && !cached_has_transient;

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
        }
    }
}

#[derive(Clone)]
pub struct FetchContext {
    pub base_url: String,
    pub token: String,
    pub project_id: String,
    pub branches: Vec<String>,
}

#[derive(Clone, Default)]
pub struct CachedMrData {
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub assignee: Option<String>,

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
}

impl CachePolicy {
    /// Returns `true` when the notes cache must be bypassed.
    pub fn notes_stale(&self) -> bool {
        matches!(self, CachePolicy::ForceAll)
    }
}

/// Fetches the last 5 pipelines for the given MR, then enriches each with
/// its job list (one extra request per pipeline, fired concurrently).
///
/// Returns an empty vec on any network or parse error — pipelines are
/// best-effort and must not block the MR data from being displayed.
async fn fetch_pipelines(ctx: &FetchContext, mr_id: &str) -> Vec<Pipeline> {
    let client = reqwest::Client::new();

    // Fetch the last 5 pipeline runs for this MR.
    let pipelines_url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/pipelines?per_page=5",
        ctx.base_url, ctx.project_id, mr_id
    );
    let res = match client
        .get(&pipelines_url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return vec![],
    };

    let mut pipelines: Vec<Pipeline> = match res.json().await {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    // Enrich each pipeline with its jobs (fire requests concurrently).
    let jobs_futures: Vec<_> = pipelines
        .iter()
        .map(|p| {
            let jobs_url = format!(
                "{}/api/v4/projects/{}/pipelines/{}/jobs?per_page=50",
                ctx.base_url, ctx.project_id, p.id
            );
            let client = client.clone();
            let token = ctx.token.clone();
            async move {
                let res = client
                    .get(&jobs_url)
                    .header("PRIVATE-TOKEN", &token)
                    .send()
                    .await
                    .ok()?;
                if res.status().is_success() {
                    match res.json::<Vec<PipelineJob>>().await {
                        Ok(jobs) => Some(jobs),
                        Err(e) => {
                            tracing::warn!("Failed to deserialize jobs: {e}");
                            None
                        }
                    }
                } else {
                    tracing::warn!("Jobs endpoint returned non-2xx: {}", res.status());
                    None
                }
            }
        })
        .collect();

    let jobs_results = futures::future::join_all(jobs_futures).await;

    for (pipeline, jobs) in pipelines.iter_mut().zip(jobs_results) {
        pipeline.jobs = jobs.unwrap_or_default();
    }

    pipelines
}

/// Fetches the real human-comment count for a MR from the GitLab Discussions API.
///
/// `GET /projects/:id/merge_requests/:iid/discussions` returns all discussion threads.
/// Each thread contains one or more notes. We count every note where `system == false`,
/// which corresponds to actual human feedback (thread starters + replies), regardless
/// of whether the thread is resolved or still open.
///
/// System notes (label changes, merge commits, assignee changes, etc.) are excluded
/// because they are GitLab activity entries, not reviewer feedback.
///
/// Falls back to `0` on any network or parse error — this is a best-effort enrichment.
async fn fetch_notes_count(ctx: &FetchContext, mr_id: &str, client: &reqwest::Client) -> u32 {
    let mut total: u32 = 0;
    let mut page: u32 = 1;

    loop {
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests/{}/discussions?per_page=100&page={}",
            ctx.base_url, ctx.project_id, mr_id, page
        );

        let res = match client
            .get(&url)
            .header("PRIVATE-TOKEN", &ctx.token)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(
                    "Discussions API returned non-2xx for MR {}: {}",
                    mr_id,
                    r.status()
                );
                break;
            }
            Err(e) => {
                tracing::warn!("Discussions API network error for MR {}: {}", mr_id, e);
                break;
            }
        };

        let discussions: Vec<serde_json::Value> = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to deserialize discussions for MR {}: {}", mr_id, e);
                break;
            }
        };

        if discussions.is_empty() {
            break;
        }

        let page_count = discussions.len();

        for discussion in discussions {
            if let Some(notes) = discussion.get("notes").and_then(|n| n.as_array()) {
                // A discussion thread is "unresolved" when at least one of its notes
                // is resolvable (i.e. it is a proper review thread, not a plain comment)
                // and has not yet been resolved.
                // We count threads (discussions), not individual notes/replies.
                let is_unresolved_thread = notes.iter().any(|note| {
                    let resolvable = note
                        .get("resolvable")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let resolved = note
                        .get("resolved")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    resolvable && !resolved
                });
                if is_unresolved_thread {
                    total += 1;
                }
            }
        }
        // If fewer than 100 results were returned, this was the last page.
        if page_count < 100 {
            break;
        }
        page += 1;
    }

    total
}

/// Fetches the number of commits the source branch is behind the target branch.
///
/// Uses `GET /projects/:id/repository/compare?from=<source>&to=<target>`.
/// The `commits` array in the response contains the commits present on `target`
/// but not yet on `source` — its length is the \"behind\" count.
///
/// Returns `None` on any network or parse error — this is a best-effort enrichment.
async fn fetch_commits_behind(
    ctx: &FetchContext,
    source_branch: &str,
    target_branch: &str,
) -> Option<u32> {
    let client = reqwest::Client::new();
    // "from=source&to=target" returns commits present on `target` but not on `source`,
    // which is the number of commits the source branch is *behind* the target branch.
    let url = format!(
        "{}/api/v4/projects/{}/repository/compare?from={}&to={}&straight=true",
        ctx.base_url,
        ctx.project_id,
        urlencoding::encode(source_branch),
        urlencoding::encode(target_branch),
    );
    let res = client
        .get(&url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        tracing::warn!(
            "Compare API returned non-2xx for {source_branch}..{target_branch}: {}",
            res.status()
        );
        return None;
    }

    let body: serde_json::Value = res.json().await.ok()?;

    // `commits` is the list of commits on `target` but not on `source`.
    let count = body
        .get("commits")
        .and_then(|v| v.as_array())
        .map(|arr| arr.len() as u32)?;

    Some(count)
}

/// Fetches diff statistics for a merge request from the GitLab Changes API.
///
/// Calls `GET /projects/:id/merge_requests/:iid/changes` and aggregates
/// `additions` + `deletions` from each changed file entry.
///
/// Returns `None` on any network or parse error — diff stats are best-effort
/// and must not block the MR data from being displayed.
async fn fetch_diff_stats(ctx: &FetchContext, mr_id: &str) -> Option<DiffStats> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}/changes",
        ctx.base_url, ctx.project_id, mr_id
    );
    let res = client
        .get(&url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
        .ok()?;

    if !res.status().is_success() {
        tracing::warn!(
            "Changes API returned non-2xx for MR {}: {}",
            mr_id,
            res.status()
        );
        return None;
    }

    let body: serde_json::Value = res.json().await.ok()?;

    let mut files_changed: u32 = 0;
    let mut additions: u32 = 0;
    let mut deletions: u32 = 0;

    // GitLab sets `overflow: true` when the diff is truncated (> 1 000 files).
    // In that case `changes` is still present but only contains a partial list,
    // so counting its entries would give an incorrect total.
    let overflow = body
        .get("overflow")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Helper: parse `changes_count` from the top-level response.
    // GitLab returns it as a string, sometimes with a trailing "+" (e.g. "1000+")
    // when the count itself is capped — strip that suffix before parsing.
    let parse_changes_count = |b: &serde_json::Value| -> u32 {
        b.get("changes_count")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('+'))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    };

    // `changes` can be null when the diff is too large for GitLab to compute inline.
    // In that case we still return the stats we have rather than returning None
    // and staying stuck on Loading.
    if let Some(changes) = body.get("changes").and_then(|v| v.as_array()) {
        for change in changes {
            files_changed += 1;
            // Each `change` entry carries a `diff` field with the raw unified patch.
            // Count lines starting with `+`/`-`, excluding `+++`/`---` file headers.
            if let Some(diff) = change.get("diff").and_then(|v| v.as_str()) {
                for line in diff.lines() {
                    if line.starts_with('+') && !line.starts_with("+++") {
                        additions += 1;
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        deletions += 1;
                    }
                }
            }
        }

        // When the diff is overflowing, the `changes` array is truncated.
        // Prefer the server-side `changes_count` which reflects the real total.
        if overflow {
            let server_count = parse_changes_count(&body);
            if server_count > 0 {
                files_changed = server_count;
            }
            // additions/deletions are already partial and cannot be recovered from
            // this endpoint when overflowing — they remain best-effort.
        }
    } else {
        // Fallback: GitLab exposes `changes_count` as a string (e.g. "42") at the
        // top level when the inline diff is omitted due to size limits.
        files_changed = parse_changes_count(&body);
    }

    // Fetch the commit count via the dedicated commits endpoint —
    // GET /projects/:id/merge_requests/:iid/commits returns a paginated list;
    // we iterate through all pages (100 per page) and sum the counts.
    let commits_count = {
        let mut total: u32 = 0;
        let mut page: u32 = 1;
        loop {
            let commits_url = format!(
                "{}/api/v4/projects/{}/merge_requests/{}/commits?per_page=100&page={}",
                ctx.base_url, ctx.project_id, mr_id, page
            );
            let res = client
                .get(&commits_url)
                .header("PRIVATE-TOKEN", &ctx.token)
                .send()
                .await;
            match res {
                Ok(r) if r.status().is_success() => {
                    match r.json::<Vec<GitLabCommit>>().await {
                        Ok(page_commits) if !page_commits.is_empty() => {
                            total += page_commits.len() as u32;
                            // If fewer than 100 results, this is the last page.
                            if page_commits.len() < 100 {
                                break;
                            }
                            page += 1;
                        }
                        // Empty page or parse error — stop paginating.
                        _ => break,
                    }
                }
                // Network or HTTP error — stop paginating, keep what we have.
                _ => break,
            }
        }
        total
    };

    Some(DiffStats {
        files_changed,
        additions,
        deletions,
        commits_count,
        commits_behind: None,
    })
}

/// Fetches all open or upcoming milestones for the project from the GitLab API.
///
/// Uses `state=active` which returns both currently active and upcoming milestones.
/// Results are sorted by title for display in the autocomplete widget.
/// Returns an empty vec on any error — milestones are best-effort.
pub async fn fetch_milestones(ctx: &FetchContext) -> Vec<GitLabMilestone> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v4/projects/{}/milestones?state=active&per_page=100",
        ctx.base_url, ctx.project_id
    );
    let res = match client
        .get(&url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return vec![],
    };
    let mut milestones: Vec<GitLabMilestone> = match res.json().await {
        Ok(m) => m,
        Err(_) => return vec![],
    };
    milestones.sort_by(|a, b| a.title.cmp(&b.title));
    milestones
}

/// Fetches all labels for the project from the GitLab API, including their colours.
///
/// Used to provide a fallback colour for labels not overridden in `config.json`.
/// Returns an empty vec on any error — labels are best-effort.
pub async fn fetch_gitlab_labels(ctx: &FetchContext) -> Vec<GitLabLabelDetail> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/v4/projects/{}/labels?per_page=100",
        ctx.base_url, ctx.project_id
    );
    let res = match client
        .get(&url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return vec![],
    };
    res.json::<Vec<GitLabLabelDetail>>()
        .await
        .unwrap_or_default()
}

/// Spawns an async task that fetches all project labels (with colours) and sends them via `tx`.
pub fn spawn_gitlab_labels_fetch(
    ctx: FetchContext,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let labels = fetch_gitlab_labels(&ctx).await;
        let _ = tx.send(AppEvent::GitlabLabelsLoaded(labels));
    });
}

/// Spawns an async task that fetches all open milestones and sends them via `tx`.
pub fn spawn_milestones_fetch(ctx: FetchContext, tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let milestones = fetch_milestones(&ctx).await;
        let _ = tx.send(AppEvent::MilestonesLoaded(milestones));
    });
}

/// Fetches all MR IIDs (internal project IDs) attached to a given milestone,
/// regardless of their state (opened, merged, closed).
///
/// A release manager needs full visibility over all MRs in a release to verify
/// that every change has been correctly ported to the target branches.
///
/// The `iid` field (not `id`) is used because it is the project-scoped identifier
/// that matches what users type in the input field.
///
/// The GitLab MRs API filters by milestone **title** (not numeric ID) via the
/// `milestone` query parameter — hence we URL-encode the title.
pub async fn fetch_milestone_mr_ids(ctx: &FetchContext, milestone_title: &str) -> Vec<String> {
    let client = reqwest::Client::new();
    // No `state` filter — a release manager needs to track all MRs regardless of
    // their state (opened, merged, closed) to verify full branch coverage for a release.
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests?milestone={}&per_page=100",
        ctx.base_url,
        ctx.project_id,
        urlencoding::encode(milestone_title)
    );
    let res = match client
        .get(&url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return vec![],
    };

    let mrs: Vec<serde_json::Value> = match res.json().await {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    mrs.iter()
        .filter_map(|v| v.get("iid").and_then(|id| id.as_u64()))
        .map(|id| id.to_string())
        .collect()
}

/// Fetches all MR IIDs for the project created on or after `created_after`
/// (RFC 3339 timestamp), regardless of their current state.
///
/// This intentionally includes MRs that were opened and merged between two
/// refresh cycles, so the discovery poller can still add them and let the stats
/// recorder persist an `OnMerge` snapshot. Paginates through all pages
/// (100/page). Returns an empty vec on any network or parse error — discovery is
/// best-effort.
pub async fn fetch_recent_mr_ids(ctx: &FetchContext, created_after: &str) -> Vec<String> {
    let client = reqwest::Client::new();
    let mut all_ids: Vec<String> = Vec::new();
    let mut page: u32 = 1;

    // URL-encode the timestamp so the `+` offset separator is not lost.
    let encoded_after = urlencoding::encode(created_after);

    loop {
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests?state=all&created_after={}&per_page=100&page={}",
            ctx.base_url, ctx.project_id, encoded_after, page
        );
        let res = match client
            .get(&url)
            .header("PRIVATE-TOKEN", &ctx.token)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => break,
        };
        let mrs: Vec<serde_json::Value> = match res.json().await {
            Ok(v) => v,
            Err(_) => break,
        };
        let page_len = mrs.len();
        for mr in mrs {
            if let Some(id) = mr.get("iid").and_then(|v| v.as_u64()) {
                all_ids.push(id.to_string());
            }
        }
        // GitLab returns fewer than 100 items on the last page.
        if page_len < 100 {
            break;
        }
        page += 1;
    }

    all_ids
}

/// Spawns an async discovery task that fetches all recent MR IIDs and emits
/// `AppEvent::NewMrsDiscovered` for any IID not yet in `known_ids`.
///
/// `created_after` is an RFC 3339 timestamp used as a lower bound on the
/// `created_at` field of the MRs returned by GitLab. When `None` (should
/// never happen in practice after the first poll), falls back to the current
/// time so the result set is always empty — safe default that avoids flooding.
pub fn spawn_mrs_discovery(
    ctx: FetchContext,
    known_ids: Vec<String>,
    created_after: Option<String>,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        // Safety net: if the anchor is somehow missing, use `now` so that no
        // pre-existing MR can slip through.
        let anchor = created_after.unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let all_ids = fetch_recent_mr_ids(&ctx, &anchor).await;
        let new_ids: Vec<String> = all_ids
            .into_iter()
            .filter(|id| !known_ids.contains(id))
            .collect();
        if !new_ids.is_empty() {
            let _ = tx.send(AppEvent::NewMrsDiscovered(new_ids));
        }
    });
}

/// Spawns an async task that fetches all open MR IIDs for the given milestone
/// and sends them via `tx` as a `MilestoneMrsLoaded` event.
pub fn spawn_milestone_mrs_fetch(
    ctx: FetchContext,
    milestone_title: String,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let mr_ids = fetch_milestone_mr_ids(&ctx, &milestone_title).await;
        let _ = tx.send(AppEvent::MilestoneMrsLoaded {
            milestone_title,
            mr_ids,
        });
    });
}

pub fn spawn_mr_fetch(
    ctx: FetchContext,
    mr_id: String,
    cached: CachedMrData,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let Ok(_permit) = semaphore.acquire().await else {
            let _ = tx.send(AppEvent::MrFailed {
                id: mr_id,
                error: "MR fetch cancelled: concurrency limiter was closed".to_string(),
            });
            return;
        };

        match fetch_gitlab_data(&ctx, &mr_id, cached).await {
            Ok(data) => {
                let _ = tx.send(AppEvent::MrLoaded(Box::new(data)));
            }
            Err(err_msg) => {
                let _ = tx.send(AppEvent::MrFailed {
                    id: mr_id,
                    error: err_msg,
                });
            }
        }
    });
}

pub async fn fetch_gitlab_data(
    ctx: &FetchContext,
    mr_id: &str,
    cached: CachedMrData,
) -> Result<MrLoadedData, String> {
    let client = reqwest::Client::new();

    // updated_at is always fetched fresh — never served from cache — so we always
    // know the real last-update timestamp regardless of the cache hit path.
    let mr_url = format!(
        "{}/api/v4/projects/{}/merge_requests/{}",
        ctx.base_url, ctx.project_id, mr_id
    );
    let mr_res = client
        .get(&mr_url)
        .header("PRIVATE-TOKEN", &ctx.token)
        .send()
        .await
        .map_err(|e| format!("MR network error: {}", e))?;

    if !mr_res.status().is_success() {
        return Err(format!("HTTP {} on MR", mr_res.status()));
    }

    let mr: GitLabMr = mr_res
        .json()
        .await
        .map_err(|e| format!("Error reading MR JSON: {}", e))?;

    let updated_at = mr.updated_at.clone();
    let created_at = mr.created_at.clone();
    // Always read the state fresh from the API response — never served from cache.
    let state = mr.state.clone().unwrap_or_default();
    // Fetch the real human-note count from the discussions endpoint.
    // The native `user_notes_count` field from the MR API is unreliable: it counts
    // ALL notes including system events (label changes, merge activity, etc.) and notes
    // inside resolved threads. On a merged MR this can reach 100+ while the actual
    // number of human comments is far lower.
    //
    // Instead we call GET /discussions which gives us the full thread graph:
    //   - `notes[].system == true`  → skip (GitLab activity entries, not human comments)
    //   - everything else           → count (thread starters + replies, resolved or not)
    //
    // This number reflects the real volume of review feedback left on the MR.
    //
    // NOTE: intentionally NOT cached on `updated_at`. GitLab does NOT update the MR's
    // `updated_at` timestamp when a discussion thread is resolved or a note is added.
    // Caching on `updated_at` would therefore silently serve a stale count after a
    // reviewer resolves a thread. We always refetch — one extra paginated request per
    // MR per refresh cycle, which is acceptable for correctness.
    //
    // Exception: merged/closed MRs with a known count are frozen — their discussion
    // history is immutable and will never change.
    // Reuse the cached notes count only when ALL of these conditions hold:
    //   1. The MR is currently non-Open (Merged or Closed) — discussion history is frozen.
    //   2. A non-zero count was already recorded — zero could mean "never fetched".
    //   3. The cached state matches the fresh state — if the MR just transitioned from
    //      Open → Merged we must refetch, because threads may have been resolved or added
    //      between the last Open refresh and the merge event.
    let state_unchanged = cached.cached_state.as_ref().is_some_and(|s| s == &state);
    // Reuse the cached notes count only when ALL conditions hold:
    //   1. The cache policy does not demand a forced re-fetch (e.g. manual [R]).
    //   2. The MR is non-Open — discussion history is frozen once merged/closed.
    //   3. A non-zero count is already recorded — zero could mean "never fetched".
    //   4. The cached state matches the fresh state — guards against Open → Merged
    //      transitions where threads may have been resolved just before the merge.
    let user_notes_count = if !cached.cache_policy.notes_stale()
        && state != GitlabMrState::Opened
        && cached.user_notes_count > 0
        && state_unchanged
    {
        cached.user_notes_count
    } else {
        fetch_notes_count(ctx, mr_id, &client).await
    };

    // Resolve mergeability with the following priority:
    //   1. `has_conflicts: true` always wins — manual intervention is required regardless
    //      of what `detailed_merge_status` says. GitLab can return "need_rebase" AND
    //      has_conflicts: true simultaneously when the rebase would produce conflicts.
    //   2. `detailed_merge_status` (GitLab ≥ 15.6) — exhaustive mapping of all known values.
    //   3. `merge_status` (legacy, GitLab < 15.6) — coarse-grained last resort.
    let mergeability = if mr.has_conflicts == Some(true) {
        MergeabilityStatus::Conflict
    } else {
        match mr.detailed_merge_status.as_deref() {
            // ── Mergeable ────────────────────────────────────────────────────────────────
            // The MR can be merged cleanly with no further action required.
            Some("mergeable") => MergeabilityStatus::Mergeable,

            // ── NeedsRebase ──────────────────────────────────────────────────────────────
            // The source branch is behind the target branch but there are no conflicts;
            // a simple rebase (or merge commit) is sufficient.
            // "need_rebase" is the canonical value; "behind_target_branch" is its alias
            // returned by some GitLab versions.
            Some("need_rebase") | Some("behind_target_branch") => MergeabilityStatus::NeedsRebase,

            // ── Conflict ─────────────────────────────────────────────────────────────────
            // Merge conflicts that require manual resolution before the MR can progress.
            Some("merge_conflict") | Some("conflict") => MergeabilityStatus::Conflict,
            // GitLab was unable to compute the merge status — treat as a blocking conflict
            // to avoid falsely showing the MR as ready.
            Some("broken_status") => MergeabilityStatus::Conflict,
            // A security policy is violated — blocks merge, treat as conflict-level blocker.
            Some("security_policy_violations") => MergeabilityStatus::Conflict,

            // ── NotOpen ──────────────────────────────────────────────────────────────────
            // The MR is not open (already merged or closed in GitLab).
            Some("not_open") => MergeabilityStatus::NotOpen,

            // ── Draft ────────────────────────────────────────────────────────────────────
            // The MR is a draft — intentionally not ready to merge.
            Some("draft_status") => MergeabilityStatus::Draft,

            // ── DiscussionsNotResolved ────────────────────────────────────────────────────
            // There are unresolved discussion threads that must be resolved before merging.
            Some("discussions_not_resolved") => MergeabilityStatus::DiscussionsNotResolved,

            // ── CiMustPass ───────────────────────────────────────────────────────────────
            // A required CI pipeline must pass before this MR can be merged.
            Some("ci_must_pass") => MergeabilityStatus::CiMustPass,

            // ── CiStillRunning ───────────────────────────────────────────────────────────
            // A CI pipeline is currently running — outcome not yet known.
            Some("ci_still_running") => MergeabilityStatus::CiStillRunning,

            // ── NotApproved ──────────────────────────────────────────────────────────────
            // Required approval rules are not yet satisfied.
            // "approvals_syncing" is a transient state while GitLab recomputes approvals.
            Some("not_approved") | Some("approvals_syncing") => MergeabilityStatus::NotApproved,

            // ── RequestedChanges ─────────────────────────────────────────────────────────
            // A reviewer has explicitly requested changes on the MR.
            Some("requested_changes") => MergeabilityStatus::RequestedChanges,

            // ── Unknown (transient / unactionable states) ────────────────────────────────
            // GitLab is currently computing the merge status — not yet actionable.
            Some("checking") | Some("unchecked") | Some("preparing") => MergeabilityStatus::Unknown,
            // External status checks (e.g. deployment gates) have not yet passed.
            Some("external_status_checks") => MergeabilityStatus::Unknown,
            // A required Jira issue association is missing.
            Some("jira_association_missing") => MergeabilityStatus::Unknown,
            // Commit message format or signature requirements are not met.
            Some("commits_status") => MergeabilityStatus::Unknown,

            // ── Fallback ─────────────────────────────────────────────────────────────────
            // Unknown or future detailed_merge_status values: fall back to the legacy field.
            _ => match mr.merge_status.as_deref() {
                Some("can_be_merged") => MergeabilityStatus::Mergeable,
                Some("cannot_be_merged") => MergeabilityStatus::Conflict,
                _ => MergeabilityStatus::Unknown,
            },
        }
    };

    // Milestone is always read fresh from the GitLab API response — never served from cache.
    // This is the authoritative source: a MR may be attached or detached from a milestone
    // at any time, and the cache would silently hold a stale value.
    let milestone_due_date = mr.milestone.as_ref().and_then(|m| m.due_date.clone());
    let milestone_description = mr.milestone.as_ref().and_then(|m| m.description.clone());
    let milestone = mr
        .milestone
        .map(|m| m.title)
        .unwrap_or_else(|| "None".to_string());

    // Reviewers are always read fresh — they can be added or removed at any time.
    // Format: "Full Name (username)" — mirrors the author/assignee display convention.
    let reviewers = mr
        .reviewers
        .unwrap_or_default()
        .into_iter()
        .map(|u| format!("{} (@{})", u.name, u.username))
        .collect::<Vec<_>>();

    // merged_by and merged_at are only populated for merged MRs.
    let merged_by = mr
        .merged_by
        .map(|u| format!("{} (@{})", u.name, u.username));
    let merged_at = mr.merged_at.clone();

    let source_branch = mr
        .source_branch
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Always read the merge SHA from the fresh API response — never from cache.
    //
    // `merge_sha`: the commit used to detect branch presence via /refs?type=branch.
    //   Priority: merge_commit_sha → squash_commit_sha → sha (HEAD, fast-forward fallback).
    //   The HEAD SHA (mr.sha) is only a valid branch-detection signal when the MR has been
    //   merged via fast-forward: in that case GitLab sets no merge_commit_sha/squash_commit_sha
    //   but the HEAD commit lands directly on the target branch.
    //   For still-open MRs the HEAD lives on the source branch only — using it would produce
    //   false positives (the commit is NOT on develop, it is on the feature branch).
    //
    // `stored_sha`: the value persisted into TrackedMr.sha and used by the auto-refresh
    //   skip guard (`mr.sha.is_some()` in app.rs). We only store a SHA once the MR is
    //   truly merged (merge_commit_sha or squash_commit_sha present) so open MRs keep
    //   sha = None and are never incorrectly frozen by the skip guard.
    let merge_sha = mr
        .merge_commit_sha
        .clone()
        .or_else(|| mr.squash_commit_sha.clone())
        .or_else(|| {
            // Use the HEAD SHA only when the MR is confirmed merged by GitLab — this
            // covers fast-forward merges where no dedicated merge commit is created.
            if state == GitlabMrState::Merged {
                mr.sha.clone()
            } else {
                None
            }
        });
    // stored_sha: only the "real" merge commit; keeps open-MR sha = None.
    let sha = mr.merge_commit_sha.or(mr.squash_commit_sha);

    // Title, description and labels: serve from cache when `updated_at` is unchanged
    // (GitLab bumps `updated_at` on any edit to these fields), refresh otherwise.
    // This avoids holding a stale "Draft: TITLE" after the draft prefix is removed,
    // while still saving the extra JSON parsing work on unchanged MRs.
    let updated_at_unchanged = updated_at.is_some() && updated_at == cached.updated_at;

    let title = if updated_at_unchanged {
        cached
            .title
            .filter(|t| !t.contains("⚠️ ERROR"))
            .unwrap_or(mr.title)
    } else {
        mr.title
    };

    let description = if updated_at_unchanged {
        cached
            .description
            .unwrap_or_else(|| mr.description.unwrap_or_default())
    } else {
        mr.description.unwrap_or_default()
    };

    let labels = if updated_at_unchanged {
        cached
            .labels
            .unwrap_or_else(|| mr.labels.unwrap_or_default())
    } else {
        mr.labels.unwrap_or_default()
    };

    // Author and web_url are immutable after MR creation — always served from cache
    // when available to avoid redundant formatting work.
    let (author, assignee, web_url) = match (cached.author, cached.assignee, cached.web_url) {
        (Some(a), Some(asg), Some(w)) if !w.is_empty() => (a, asg, w),
        _ => {
            let auth = mr
                .author
                .map(|u| format!("{} (@{})", u.name, u.username))
                .unwrap_or_else(|| "unknown".to_string());
            let asg = mr
                .assignee
                .map(|u| format!("{} (@{})", u.name, u.username))
                .unwrap_or_else(|| "none".to_string());
            let web_url = mr.web_url.unwrap_or_default();

            (auth, asg, web_url)
        }
    };

    let mut found_branches = HashSet::new();

    if let Some(ref commit_sha) = merge_sha {
        let refs_url = format!(
            "{}/api/v4/projects/{}/repository/commits/{}/refs?type=branch",
            ctx.base_url, ctx.project_id, commit_sha
        );
        match client
            .get(&refs_url)
            .header("PRIVATE-TOKEN", &ctx.token)
            .send()
            .await
        {
            Ok(refs_res) if refs_res.status().is_success() => {
                match refs_res.json::<Vec<GitLabRef>>().await {
                    Ok(refs) => {
                        for r in refs {
                            let cleaned = r.name.replace("refs/heads/", "");
                            if ctx.branches.contains(&cleaned) {
                                found_branches.insert(cleaned);
                            }
                        }
                        tracing::debug!(
                            mr_id = %mr_id,
                            commit_sha = %commit_sha,
                            found = ?found_branches,
                            "Branch detection via /refs",
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            mr_id = %mr_id,
                            commit_sha = %commit_sha,
                            error = %e,
                            "Failed to deserialize /refs response — skipping branch detection",
                        );
                    }
                }
            }
            Ok(refs_res) => {
                // GitLab returns 404 when the commit has been garbage-collected
                // (e.g. force-push after merge, or squash with no merge commit kept).
                // Fall back to the MR target_branch as a best-effort signal: if the MR
                // is confirmed merged by GitLab, it was at least present on its target.
                let status = refs_res.status();
                tracing::warn!(
                    mr_id = %mr_id,
                    commit_sha = %commit_sha,
                    http_status = %status,
                    "Non-2xx on /refs — commit may be garbage-collected; falling back to target_branch",
                );
                // `target_branch` is resolved later; use the raw field from the API response
                // to avoid a forward-reference. Only inject if it is one of the tracked branches.
                if let Some(ref tb) = mr.target_branch {
                    if ctx.branches.contains(tb) {
                        found_branches.insert(tb.clone());
                        tracing::debug!(
                            mr_id = %mr_id,
                            target_branch = %tb,
                            "Inserted target_branch into found_branches via fallback",
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    mr_id = %mr_id,
                    commit_sha = %commit_sha,
                    error = %e,
                    "Network error on /refs — skipping branch detection",
                );
            }
        }
    } else {
        tracing::debug!(
            mr_id = %mr_id,
            state = ?state,
            "No merge SHA available — skipping branch detection (MR not yet merged)",
        );
    }

    // Fetch diff statistics (files changed, additions, deletions) from the Changes API.
    // We cache this behind the same `updated_at` guard as pipelines to avoid hammering
    // the API: if the MR hasn't changed, the diff hasn't changed either.
    let cached_diff_stats = cached.diff_stats.clone();
    // Invalidate the cache if the stored stats look corrupted: files_changed > 0
    // but both additions and deletions are 0 means they were fetched with the old
    // `added_lines`/`removed_lines` fields that do not exist in the GitLab API.
    let cached_diff_stats_valid = cached_diff_stats.as_ref().is_some_and(|s| {
        // Reject entries where additions/deletions are inconsistent (old cache format).
        let lines_ok = s.files_changed == 0 || s.additions > 0 || s.deletions > 0;
        // Reject entries where commits_count was never fetched (field added later —
        // stale state files have 0 from serde default, but a real MR always has ≥ 1 commit).
        // Once the fresh fetch writes a real value (even 0 from the API), we accept it.
        // We distinguish stale-zero from api-zero by checking files_changed: if files > 0
        // and commits == 0, the entry was written before this field existed.
        let commits_ok = s.commits_count > 0 || s.files_changed == 0;
        lines_ok && commits_ok
    });
    let diff_stats =
        if updated_at.is_some() && updated_at == cached.updated_at && cached_diff_stats_valid {
            cached_diff_stats
        } else {
            fetch_diff_stats(ctx, mr_id).await
        };

    // Resolve the number of commits the source branch is behind the target branch.
    //
    // Strategy (in priority order):
    //   1. `diverged_commits_count` from the MR response — free, no extra API call.
    //      Populated when the request includes `include_diverged_commits_count=true`.
    //      GitLab may return `null` for this field even with the param when the MR is
    //      Mergeable or when the computation has not yet been triggered server-side.
    //   2. Implicit `Some(0)` when mergeability is `Mergeable` — up to date by definition.
    //   3. `/repository/compare` fallback — one extra call, used only when GitLab did
    //      not provide `diverged_commits_count` for a non-Mergeable open MR.
    //   4. `None` for merged/closed MRs — not applicable.
    let commits_behind: Option<u32> = if state == GitlabMrState::Opened {
        match mergeability {
            MergeabilityStatus::Mergeable => Some(0),
            MergeabilityStatus::Unknown => {
                // GitLab has not computed the status yet — also skip the compare call
                // to avoid a spurious request while the MR is still being checked.
                None
            }
            _ => {
                // Non-Mergeable open MR: prefer the free field from the MR response.
                // Fall back to /compare only when GitLab did not provide the count.
                match mr.diverged_commits_count {
                    Some(n) => {
                        tracing::debug!(
                            mr_id = %mr_id,
                            commits_behind = n,
                            "Using diverged_commits_count from MR response",
                        );
                        Some(n)
                    }
                    None => {
                        tracing::debug!(
                            mr_id = %mr_id,
                            "diverged_commits_count absent — falling back to /compare",
                        );
                        let tb = mr.target_branch.as_deref().unwrap_or("main");
                        fetch_commits_behind(ctx, &source_branch, tb).await
                    }
                }
            }
        }
    } else {
        // Merged / Closed — not applicable.
        None
    };

    // Inject commits_behind into the diff_stats so it is persisted alongside the rest.
    let diff_stats = diff_stats.map(|mut s| {
        s.commits_behind = commits_behind;
        s
    });

    // Only re-fetch pipelines if the MR has been updated since the last cycle.
    // If `updated_at` is unchanged, reuse the cached pipeline data to avoid
    // hammering the GitLab API with redundant requests (rate-limit friendly).
    // Also re-fetch if cached pipelines exist but none have jobs — this handles
    // stale cache entries written before jobs were persisted.
    // Also re-fetch if any cached pipeline is missing `created_at` — this
    // transparently enriches state files written before that field was added.
    //
    // IMPORTANT: always re-fetch when any cached pipeline is in a transient state
    // (running, pending, created, waiting). A pipeline can transition to success/failed
    // without the MR's `updated_at` changing, so the equality check alone is not
    // sufficient to detect stale pipeline data.
    let cached_has_jobs = cached.pipelines.iter().any(|p| !p.jobs.is_empty());
    let cached_has_dates = cached.pipelines.iter().all(|p| p.created_at.is_some());
    let cached_has_transient_pipeline = cached.pipelines.iter().any(|p| {
        matches!(
            p.status,
            crate::models::PipelineState::Running
                | crate::models::PipelineState::Pending
                | crate::models::PipelineState::Created
        )
    });
    let pipelines = if updated_at.is_some()
        && updated_at == cached.updated_at
        && !cached.pipelines.is_empty()
        && cached_has_jobs
        && cached_has_dates
        && !cached_has_transient_pipeline
    {
        cached.pipelines
    } else {
        fetch_pipelines(ctx, mr_id).await
    };

    let target_branch = mr.target_branch.unwrap_or_else(|| "unknown".to_string());

    Ok(MrLoadedData {
        id: mr_id.to_string(),
        title,
        sha,
        branches: found_branches,
        description,
        author,
        assignee,
        reviewers,
        milestone,
        milestone_due_date,
        milestone_description,
        web_url,
        labels,
        updated_at,
        created_at,
        source_branch,
        target_branch,
        state,
        merged_by,
        merged_at,
        mergeability,
        pipelines,
        user_notes_count,
        diff_stats,
    })
}
