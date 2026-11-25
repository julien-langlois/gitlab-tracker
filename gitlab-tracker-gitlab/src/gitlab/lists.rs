//! Project lists: milestones, labels, MRs of a milestone, discovery of new MRs.

use super::*;

/// Only the project-scoped id of a MR list entry.
#[derive(serde::Deserialize)]
pub(super) struct MrIid {
    pub(super) iid: u64,
}

/// Fetches all open or upcoming milestones for the project from the GitLab API.
///
/// Uses `state=active` which returns both currently active and upcoming milestones.
/// Results are sorted by title for display in the autocomplete widget.
/// Returns an empty vec on any error — milestones are best-effort.
pub async fn fetch_milestones(ctx: &FetchContext) -> Vec<GitLabMilestone> {
    let url = format!(
        "{}/api/v4/projects/{}/milestones?state=active",
        ctx.base_url, ctx.project_id
    );
    let mut milestones: Vec<GitLabMilestone> = get_all_pages(ctx, &url).await.unwrap_or_default();
    milestones.sort_by(|a, b| a.title.cmp(&b.title));
    milestones
}

/// Fetches all labels for the project from the GitLab API, including their colours.
///
/// Used to provide a fallback colour for labels not overridden in `config.json`.
/// Returns an empty vec on any error — labels are best-effort.
pub async fn fetch_gitlab_labels(ctx: &FetchContext) -> Vec<GitLabLabelDetail> {
    let url = format!(
        "{}/api/v4/projects/{}/labels?with_counts=false",
        ctx.base_url, ctx.project_id
    );
    get_all_pages(ctx, &url).await.unwrap_or_default()
}

/// Spawns an async task that fetches all project labels (with colours) and sends them via `tx`.
pub fn spawn_gitlab_labels_fetch<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(async move {
        let labels = fetch_gitlab_labels(&ctx).await;
        crate::events::emit(&tx, GitlabEvent::GitlabLabelsLoaded(labels));
    });
}

/// Spawns an async task that fetches all open milestones and sends them via `tx`.
pub fn spawn_milestones_fetch<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(async move {
        let milestones = fetch_milestones(&ctx).await;
        crate::events::emit(&tx, GitlabEvent::MilestonesLoaded(milestones));
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
    // No `state` filter — a release manager needs to track all MRs regardless of
    // their state (opened, merged, closed) to verify full branch coverage for a release.
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests?milestone={}",
        ctx.base_url,
        ctx.project_id,
        urlencoding::encode(milestone_title)
    );
    get_all_pages::<MrIid>(ctx, &url)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|mr| mr.iid.to_string())
        .collect()
}

/// Fetches all MR IIDs for the project created on or after `created_after`
/// (RFC 3339 timestamp), regardless of their current state.
///
/// This intentionally includes MRs that were opened and merged between two
/// refresh cycles, so the discovery poller can still add them and let the stats
/// recorder persist an `OnMerge` snapshot. Paginates through all pages
/// ([`get_all_pages`]).
///
/// Returns `None` when any page fails — a partial result must not be mistaken for
/// a complete poll (the discovery anchor only advances after a complete one).
pub async fn fetch_recent_mr_ids(ctx: &FetchContext, created_after: &str) -> Option<Vec<String>> {
    // URL-encode the timestamp so the `+` offset separator is not lost.
    let url = format!(
        "{}/api/v4/projects/{}/merge_requests?state=all&created_after={}",
        ctx.base_url,
        ctx.project_id,
        urlencoding::encode(created_after)
    );
    let mrs = get_all_pages::<MrIid>(ctx, &url).await?;
    Some(mrs.into_iter().map(|mr| mr.iid.to_string()).collect())
}

/// Safety margin applied when advancing the discovery anchor, absorbing clock
/// skew between this machine and the GitLab server.
pub(super) const DISCOVERY_ANCHOR_MARGIN: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// Next `created_after` anchor once a poll that started at `polled_at` completed.
///
/// Everything created before `polled_at` has been seen, so later polls only need
/// to look from there (minus a clock-skew margin) instead of re-paginating every
/// MR since the first launch. The anchor never moves backwards: MRs created before
/// the tool was first started must never be auto-added.
pub(super) fn next_discovery_anchor(
    anchor: &str,
    polled_at: chrono::DateTime<chrono::Utc>,
) -> String {
    let candidate = polled_at - DISCOVERY_ANCHOR_MARGIN;
    match chrono::DateTime::parse_from_rfc3339(anchor) {
        Ok(current) if current >= candidate => anchor.to_string(),
        Ok(_) => candidate.to_rfc3339(),
        Err(_) => anchor.to_string(),
    }
}

/// Spawns an async discovery task that fetches the MR IIDs created after `anchor`
/// and emits `GitlabEvent::NewMrsDiscovered` with the ones not in `known_ids`, plus
/// the advanced anchor. Nothing is emitted when the poll fails.
pub fn spawn_mrs_discovery<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    known_ids: HashSet<String>,
    anchor: String,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(async move {
        let polled_at = chrono::Utc::now();
        let Some(all_ids) = fetch_recent_mr_ids(&ctx, &anchor).await else {
            return;
        };
        let mr_ids = all_ids
            .into_iter()
            .filter(|id| !known_ids.contains(id))
            .collect();
        crate::events::emit(
            &tx,
            GitlabEvent::NewMrsDiscovered {
                mr_ids,
                next_anchor: next_discovery_anchor(&anchor, polled_at),
            },
        );
    });
}

/// Spawns an async task that fetches all open MR IIDs for the given milestone
/// and sends them via `tx` as a `MilestoneMrsLoaded` event.
pub fn spawn_milestone_mrs_fetch<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    milestone_title: String,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(async move {
        let mr_ids = fetch_milestone_mr_ids(&ctx, &milestone_title).await;
        crate::events::emit(
            &tx,
            GitlabEvent::MilestoneMrsLoaded {
                milestone_title,
                mr_ids,
            },
        );
    });
}
