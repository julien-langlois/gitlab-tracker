//! Spawning MR fetches: the refresh's list read, settled checks, single reads, refs.

use super::*;

pub fn spawn_mr_fetch<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    mr_id: String,
    cached: CachedMrData,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(fetch_and_report(
        ctx,
        mr_id,
        cached,
        Prefetched::default(),
        semaphore,
        tx,
    ));
}

/// What a batched refresh already read for one MR, so its own fetch skips it.
#[derive(Default)]
pub struct Prefetched {
    /// Its entry in the MR list read (`list_mrs`).
    pub mr: Option<GitLabMr>,
    /// Its pipelines with their jobs, from the GraphQL read (`gitlab_graphql`).
    pub pipelines: Option<Vec<Pipeline>>,
}

pub(super) async fn fetch_and_report<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    mr_id: String,
    cached: CachedMrData,
    prefetched: Prefetched,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    match fetch_gitlab_data(&ctx, &mr_id, cached, prefetched, &semaphore, tx.clone()).await {
        Ok(data) => {
            crate::events::emit(&tx, GitlabEvent::MrLoaded(Box::new(data)));
        }
        Err(error) => {
            crate::events::emit(&tx, GitlabEvent::MrFailed { id: mr_id, error });
        }
    }
}

/// Most MRs read per list request (GitLab's page size cap).
pub const MRS_PER_LIST: usize = 100;

/// `merge_requests?iids[]=…` for `ids` (query string open for paging). Asks GitLab
/// to recheck mergeability like the single read does.
pub(super) fn mr_list_url(ctx: &FetchContext, ids: &[&str]) -> String {
    let iids: String = ids.iter().map(|id| format!("&iids[]={id}")).collect();
    format!(
        "{}/api/v4/projects/{}/merge_requests?with_merge_status_recheck=true{iids}",
        ctx.base_url, ctx.project_id
    )
}

/// Reads the MRs `ids` with one request per [`MRS_PER_LIST`] MRs. An MR missing
/// from the result (failed request, deleted MR) is read on its own afterwards.
pub(super) async fn list_mrs(
    ctx: &FetchContext,
    ids: &[&str],
    semaphore: &Semaphore,
) -> HashMap<String, GitLabMr> {
    let mut listed = HashMap::new();
    for chunk in ids.chunks(MRS_PER_LIST) {
        let Ok(_permit) = semaphore.acquire().await else {
            break;
        };
        match get_all_pages::<GitLabMr>(ctx, &mr_list_url(ctx, chunk)).await {
            Some(mrs) => listed.extend(
                mrs.into_iter()
                    .filter_map(|mr| Some((mr.iid?.to_string(), mr))),
            ),
            None => tracing::warn!(
                mrs = chunk.len(),
                "MR list read failed — reading them one by one"
            ),
        }
    }
    listed
}

/// Whether a listed MR's pipelines must be read: its cache is not reusable (see
/// [`pipelines_cache_reusable`]) or the MR changed since. Same rule as the per-MR
/// fetch, decided ahead so they can be read together.
pub(super) fn pipelines_due(cached: &CachedMrData, listed: &GitLabMr) -> bool {
    cached.cache_policy.pipelines_stale()
        || !(listed.updated_at.is_some()
            && listed.updated_at == cached.updated_at
            && pipelines_cache_reusable(&cached.pipelines))
}

/// Refreshes many MRs: their base data comes from list reads (one request per
/// [`MRS_PER_LIST`] MRs instead of one each), the pipelines due from GraphQL
/// queries (`gitlab_graphql`, one per batch instead of up to 6 requests per MR),
/// then each MR reads only what its cache cannot vouch for (`fetch_gitlab_data`).
pub fn spawn_mr_fetches<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    mrs: Vec<(String, CachedMrData)>,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    if mrs.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let ids: Vec<&str> = mrs.iter().map(|(id, _)| id.as_str()).collect();
        let mut listed = list_mrs(&ctx, &ids, &semaphore).await;
        let due: Vec<String> = mrs
            .iter()
            .filter(|(id, cached)| listed.get(id).is_some_and(|mr| pipelines_due(cached, mr)))
            .map(|(id, _)| id.clone())
            .collect();
        let mut pipelines = if due.is_empty() {
            HashMap::new()
        } else {
            crate::gitlab_graphql::mr_pipelines(&ctx, &due, &semaphore).await
        };
        for (id, cached) in mrs {
            let prefetched = Prefetched {
                mr: listed.remove(&id),
                pipelines: pipelines.remove(&id),
            };
            tokio::spawn(fetch_and_report(
                ctx.clone(),
                id,
                cached,
                prefetched,
                semaphore.clone(),
                tx.clone(),
            ));
        }
    });
}

/// An MR that refresh cycles leave out (merged on every tracked branch), as it
/// was last read.
pub struct SettledMr {
    pub id: String,
    pub updated_at: Option<String>,
    pub milestone: Option<String>,
    /// Its milestone history was read (`MrData::milestone_events`).
    pub events_read: bool,
}

impl From<&crate::models::TrackedMr> for SettledMr {
    fn from(mr: &crate::models::TrackedMr) -> Self {
        Self {
            id: mr.id.clone(),
            updated_at: mr.updated_at.clone(),
            milestone: mr.milestone.clone(),
            events_read: mr.milestone_events.is_some(),
        }
    }
}

/// Whether a settled MR must be read again: GitLab still lets its milestone,
/// title or labels change after the merge (`updated_at` moves), and its
/// milestone history may never have been read. Missing from the list (failed
/// read, deleted MR): nothing to go on, left as it is.
pub(super) fn settled_changed(settled: &SettledMr, listed: Option<&GitLabMr>) -> bool {
    !settled.events_read
        || listed.is_some_and(|mr| {
            mr.updated_at != settled.updated_at
                || mr.milestone.as_ref().map(|m| m.title.as_str()) != settled.milestone.as_deref()
        })
}

/// Checks the settled MRs with list reads (one request per [`MRS_PER_LIST`]),
/// and emits `SettledMrsChanged` with those to refresh.
pub fn spawn_settled_check<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    settled: Vec<SettledMr>,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    if settled.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let ids: Vec<&str> = settled
            .iter()
            .filter(|s| s.events_read)
            .map(|s| s.id.as_str())
            .collect();
        let listed = list_mrs(&ctx, &ids, &semaphore).await;
        let changed: Vec<String> = settled
            .iter()
            .filter(|s| settled_changed(s, listed.get(&s.id)))
            .map(|s| s.id.clone())
            .collect();
        if !changed.is_empty() {
            crate::events::emit(&tx, GitlabEvent::SettledMrsChanged(changed));
        }
    });
}

/// Whether an MR's list entry is not enough and its own page must be read: an
/// open MR that cannot merge as is needs `diverged_commits_count` (single read
/// only), and a mergeability still being computed goes through the retries.
pub(super) fn needs_single_read(mr: &GitLabMr) -> bool {
    let open = mr.state.unwrap_or_default() == GitlabMrState::Opened;
    is_transient_mergeability_status(mr.detailed_merge_status.as_deref())
        || (open && mr.detailed_merge_status.as_deref() != Some("mergeable"))
}

/// Discussions and approvals of an open MR with no sign of change are still read
/// again past this age: a safety net for changes GitLab reports nowhere in the
/// list entry (an optional approval may leave `updated_at` as it was).
pub const REVIEW_MAX_AGE: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// The cached review timeline was read the current way: one read with an
/// older [`crate::models::REVIEW_ACTIVITY_VERSION`] is read again once, even
/// on a merged MR (its discussions are otherwise frozen).
pub(super) fn review_current(cached: &CachedMrData) -> bool {
    cached
        .review
        .as_ref()
        .is_some_and(|r| r.version >= crate::models::REVIEW_ACTIVITY_VERSION)
}

/// Whether an open MR's discussions and approvals must be read again.
///
/// `unchanged`: same `updated_at` and same GitLab note count as at the last read
/// (a new comment moves the count). Resolving a thread moves neither, so the
/// blocking-threads flag is compared with the cached unresolved count; an
/// approval may only show in the mergeability (an approval rule now met).
pub(super) fn review_stale(
    cached: &CachedMrData,
    unchanged: bool,
    blocking_resolved: Option<bool>,
    mergeability: MergeabilityStatus,
    read_age: Option<chrono::TimeDelta>,
) -> bool {
    if cached.cache_policy.notes_stale() || !review_current(cached) || !unchanged {
        return true;
    }
    let threads_moved = match blocking_resolved {
        Some(true) => cached.user_notes_count > 0,
        Some(false) => cached.user_notes_count == 0,
        None => false,
    };
    threads_moved
        || cached.mergeability != Some(mergeability)
        || read_age.is_none_or(|age| age >= REVIEW_MAX_AGE)
}

pub(super) async fn fetch_single_mr(
    client: &reqwest::Client,
    ctx: &FetchContext,
    mr_url: &str,
) -> Result<GitLabMr, GitlabError> {
    let mr_res = client
        .get(mr_url)
        .header("PRIVATE-TOKEN", ctx.token.as_str())
        .send_paced()
        .await
        .map_err(GitlabError::Network)?;

    if !mr_res.status().is_success() {
        return Err(GitlabError::Http(mr_res.status()));
    }

    mr_res.json().await.map_err(GitlabError::Decode)
}

pub(super) const REFS_PER_PAGE: usize = 100;
/// Upper bound on `/refs` pages fetched per MR (500 branches). Each page makes the
/// server list every branch containing the commit; past the cap the branch scan
/// still finds the merge SHA on a tracked branch (`BranchPresence`).
pub(super) const REFS_MAX_PAGES: usize = 5;

/// Outcome of a `/refs` lookup for one commit.
pub(super) enum RefsLookup {
    /// Every page read: the tracked branches containing the commit.
    Found(HashSet<String>),
    /// 404: the commit no longer exists (garbage-collected).
    CommitGone,
    /// Rate limit, server or network error: the branches found before it failed.
    Failed(HashSet<String>),
}

/// Tracked branches containing `commit_sha` (GitLab `/commits/:sha/refs`). The
/// endpoint is paginated: an old commit can be in many branches, so pages are read
/// while they are full and some tracked branch is still missing.
pub(super) async fn branches_containing(
    client: &reqwest::Client,
    ctx: &FetchContext,
    commit_sha: &str,
) -> RefsLookup {
    let refs_url = format!(
        "{}/api/v4/projects/{}/repository/commits/{}/refs?type=branch&per_page={REFS_PER_PAGE}",
        ctx.base_url, ctx.project_id, commit_sha
    );
    let mut found = HashSet::new();
    for page in 1..=REFS_MAX_PAGES {
        let res = match client
            .get(format!("{refs_url}&page={page}"))
            .header("PRIVATE-TOKEN", ctx.token.as_str())
            .send_paced()
            .await
        {
            Ok(res) => res,
            Err(e) => {
                tracing::warn!(error = %e, commit_sha, "Network error on /refs");
                return RefsLookup::Failed(found);
            }
        };
        if res.status() == reqwest::StatusCode::NOT_FOUND && page == 1 {
            return RefsLookup::CommitGone;
        }
        if !res.status().is_success() {
            tracing::warn!(status = %res.status(), commit_sha, "Non-2xx on /refs");
            return RefsLookup::Failed(found);
        }
        let refs = match res.json::<Vec<GitLabRef>>().await {
            Ok(refs) => refs,
            Err(e) => {
                tracing::warn!(error = %e, commit_sha, "Undecodable /refs response");
                return RefsLookup::Failed(found);
            }
        };
        let full_page = refs.len() == REFS_PER_PAGE;
        collect_tracked_refs(refs, &ctx.branches, &mut found);
        if !full_page || found.len() == ctx.branches.len() {
            tracing::debug!(commit_sha, found = ?found, "Branch detection via /refs");
            return RefsLookup::Found(found);
        }
    }
    // Cap reached with a tracked branch still unseen: the list is incomplete, so it
    // says nothing about that branch — keep what was known (like a failed call).
    tracing::warn!(commit_sha, "Branch detection stopped at the /refs page cap");
    RefsLookup::Failed(found)
}

/// Adds every ref from `refs` that is one of the tracked `branches` to `found`.
pub(super) fn collect_tracked_refs(
    refs: Vec<GitLabRef>,
    branches: &[String],
    found: &mut HashSet<String>,
) {
    for r in refs {
        let cleaned = r.name.replace("refs/heads/", "");
        if branches.contains(&cleaned) {
            found.insert(cleaned);
        }
    }
}

pub(super) fn is_transient_mergeability_status(status: Option<&str>) -> bool {
    matches!(status, Some("checking" | "unchecked" | "preparing"))
}
