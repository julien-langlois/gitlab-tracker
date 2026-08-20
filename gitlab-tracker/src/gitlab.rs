use crate::models::{
    AppEvent, DiffStats, GitLabCommit, GitLabLabelDetail, GitLabMilestone, GitLabMr, GitLabRef,
    MergeabilityStatus, MrLoadedData, Pipeline, PipelineJob,
};

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub const MAX_CONCURRENT_REQUESTS: usize = 3;

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
    } else {
        // Fallback: GitLab exposes `changes_count` as a string (e.g. "42") at the
        // top level when the inline diff is omitted due to size limits.
        files_changed = body
            .get("changes_count")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
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
        let _permit = semaphore.acquire().await.unwrap();
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
    // Always read the state fresh from the API response — never served from cache.
    let state = mr.state.clone().unwrap_or_default();
    // Always read the notes count fresh — it reflects live discussion activity.
    let user_notes_count = mr.user_notes_count.unwrap_or(0);

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
    // A MR that was open on the previous fetch (sha = None in cache) may have been
    // merged since, and the cache would silently keep sha = None, preventing branch
    // detection from ever running for that MR.
    let sha = mr.merge_commit_sha.or(mr.squash_commit_sha);

    let (title, description, author, assignee, web_url, labels) = match (
        cached.title,
        cached.description,
        cached.author,
        cached.assignee,
        cached.web_url,
        cached.labels,
    ) {
        (Some(t), Some(d), Some(a), Some(asg), Some(w), Some(lbls))
            if !t.contains("⚠️ ERROR") && !w.is_empty() =>
        {
            (t, d, a, asg, w, lbls)
        }
        _ => {
            let title = mr.title;
            let desc = mr.description.unwrap_or_default();
            let auth = mr
                .author
                .map(|u| format!("{} (@{})", u.name, u.username))
                .unwrap_or_else(|| "unknown".to_string());
            let asg = mr
                .assignee
                .map(|u| format!("{} (@{})", u.name, u.username))
                .unwrap_or_else(|| "none".to_string());
            let web_url = mr.web_url.unwrap_or_default();
            let labels = mr.labels.unwrap_or_default();

            (title, desc, auth, asg, web_url, labels)
        }
    };

    let mut found_branches = HashSet::new();

    if let Some(ref commit_sha) = sha {
        let refs_url_standard = format!(
            "{}/api/v4/projects/{}/repository/commits/{}/refs?type=branch",
            ctx.base_url, ctx.project_id, commit_sha
        );
        if let Ok(refs_res) = client
            .get(&refs_url_standard)
            .header("PRIVATE-TOKEN", &ctx.token)
            .send()
            .await
        {
            if refs_res.status().is_success() {
                if let Ok(refs) = refs_res.json::<Vec<GitLabRef>>().await {
                    for r in refs {
                        let cleaned = r.name.replace("refs/heads/", "");
                        if ctx.branches.contains(&cleaned) {
                            found_branches.insert(cleaned);
                        }
                    }
                }
            }
        }
    }

    // No fallback search needed: if merge_commit_sha (or squash_commit_sha) is absent,
    // the MR has not been merged yet and cannot be present on any target branch.
    // The /refs?type=branch call above is the only reliable detection mechanism —
    // any commit-message search would be guesswork and a source of false positives.

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
        web_url,
        labels,
        updated_at,
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
