//! The sentinel: between two full refresh cycles, a few light reads of what
//! changed on GitLab since the last check, so that only those MRs are read again.
//!
//! One check reads, from the watermark `since` on:
//! * `merge_requests?updated_after=` — pushes, title, labels, reviewers, state…;
//! * `pipelines?updated_after=` — a pipeline that starts or ends does not touch
//!   its MR's `updated_at`;
//! * `events` — comments and approvals (which may leave `updated_at` as it was)
//!   and pushes on any branch: a tracked branch (branch scan) or the target of
//!   open MRs (mergeability, commits behind).
//!
//! What none of them reports (a resolved thread, a mergeability GitLab recomputes
//! on its own, a milestone's dates…) is left to the full cycle and to
//! `gitlab::REVIEW_MAX_AGE`, as before.
//!
//! The watermark follows the GitLab server's clock (its `Date` header), with an
//! overlap: a check reads a little of the previous one again, and what it already
//! saw (same `updated_at`, same pipeline state, same event) is skipped.

use crate::events::GitlabEvent;
use crate::gitlab::{http_client, CachePolicy, FetchContext};
use crate::models::{GitlabMrState, TrackedMr};
use chrono::{DateTime, TimeDelta, Utc};
use gitlab_tracker_core::pacing::SendPaced;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// How far back each check starts before the previous one ended: absorbs
/// requests in flight at the server and timestamps rounded by GitLab.
pub const OVERLAP: TimeDelta = TimeDelta::seconds(30);

/// A watermark older than this (sleep, network down, long failures) is not
/// caught up on event by event: a full cycle runs instead.
pub const MAX_GAP: TimeDelta = TimeDelta::minutes(15);

/// Most pages read per list in one check: past this, a full cycle is cheaper.
const MAX_PAGES: u32 = 5;
const PAGE_SIZE: usize = 100;

/// The sentinel's state in the app.
#[derive(Debug, Clone)]
pub struct SentinelState {
    /// Changes after this moment (server clock) are not read yet.
    pub since: DateTime<Utc>,
    /// GitLab's clock minus ours, from the last check's `Date` header.
    pub skew: TimeDelta,
    /// A check is running: none is started on top of it.
    pub in_flight: bool,
    /// Events the last check saw, skipped when the overlap shows them again.
    pub seen_events: HashSet<u64>,
}

impl SentinelState {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            since: now - OVERLAP,
            skew: TimeDelta::zero(),
            in_flight: false,
            seen_events: HashSet::new(),
        }
    }

    /// Now on GitLab's clock.
    pub fn server_now(&self, local_now: DateTime<Utc>) -> DateTime<Utc> {
        local_now + self.skew
    }

    /// Moves the watermark to the start of the check `report` (server clock
    /// when GitLab gave it), minus the overlap; it never goes back.
    pub fn advance(&mut self, report: &SentinelReport) {
        if let Some(server) = report.server_now {
            self.skew = server - report.local_now;
        }
        let end = report
            .server_now
            .unwrap_or_else(|| self.server_now(report.local_now));
        self.since = (end - OVERLAP).max(self.since);
        self.seen_events = report.events.iter().map(|e| e.id).collect();
    }
}

// ── What GitLab answers ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ListedMr {
    pub iid: u64,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListedPipeline {
    pub id: u64,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectEvent {
    pub id: u64,
    #[serde(default)]
    pub action_name: String,
    pub target_type: Option<String>,
    pub target_iid: Option<u64>,
    pub created_at: Option<String>,
    pub note: Option<EventNote>,
    pub push_data: Option<PushData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventNote {
    pub noteable_type: Option<String>,
    pub noteable_iid: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushData {
    pub ref_type: Option<String>,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
}

/// What one check read.
#[derive(Debug, Clone)]
pub struct SentinelReport {
    /// When it started, on our clock and on GitLab's (`None`: no `Date` header).
    pub local_now: DateTime<Utc>,
    pub server_now: Option<DateTime<Utc>>,
    /// A list had more pages than a check reads: a full refresh runs.
    pub truncated: bool,
    pub mrs: Vec<ListedMr>,
    pub pipelines: Vec<ListedPipeline>,
    /// Events created after `since`, oldest last.
    pub events: Vec<ProjectEvent>,
}

// ── Reading ────────────────────────────────────────────────────────────────

/// Starts a check from `since`; sends `SentinelChecked` with its report, or
/// `None` when a read failed (the watermark then stays where it was).
pub fn spawn_check<E: From<GitlabEvent> + Send + 'static>(
    ctx: FetchContext,
    since: DateTime<Utc>,
    semaphore: Arc<Semaphore>,
    tx: tokio::sync::mpsc::UnboundedSender<E>,
) {
    tokio::spawn(async move {
        let report = {
            let Ok(_permit) = semaphore.acquire().await else {
                return;
            };
            check(&ctx, since).await
        };
        if report.is_none() {
            tracing::warn!("Sentinel check failed — the next one reads from the same point");
        }
        crate::events::emit(&tx, GitlabEvent::SentinelChecked(report.map(Box::new)));
    });
}

async fn check(ctx: &FetchContext, since: DateTime<Utc>) -> Option<SentinelReport> {
    let local_now = Utc::now();
    let after =
        urlencoding::encode(&since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).into_owned();
    let base = format!("{}/api/v4/projects/{}", ctx.base_url, ctx.project_id);

    let mrs = read_pages::<ListedMr>(
        ctx,
        &format!(
            "{base}/merge_requests?state=all&updated_after={after}&order_by=updated_at&sort=desc"
        ),
        |_| false,
    )
    .await
    .ok()?;
    let pipelines = read_pages::<ListedPipeline>(
        ctx,
        &format!("{base}/pipelines?updated_after={after}&order_by=updated_at&sort=desc"),
        |_| false,
    )
    .await
    .ok()?;
    // `after` is a date there (events created after that day): ask from the day
    // before, newest first, and stop at the first page that reaches `since`. A
    // token without the Event permission still gets the two reads above.
    let day_before = (since - TimeDelta::days(1)).format("%Y-%m-%d");
    let events = match read_pages::<ProjectEvent>(
        ctx,
        &format!("{base}/events?after={day_before}&sort=desc"),
        |page| page.iter().any(|e| !event_after(e, since)),
    )
    .await
    {
        Ok(pages) => pages,
        Err(ReadError::Denied) => {
            tracing::warn!("Sentinel: events not readable (token without Event) — skipped");
            Pages::default()
        }
        Err(ReadError::Failed) => return None,
    };

    Some(SentinelReport {
        local_now,
        server_now: mrs.date,
        truncated: mrs.truncated || pipelines.truncated || events.truncated,
        mrs: mrs.items,
        pipelines: pipelines.items,
        events: events
            .items
            .into_iter()
            .filter(|e| event_after(e, since))
            .collect(),
    })
}

fn event_after(event: &ProjectEvent, since: DateTime<Utc>) -> bool {
    event
        .created_at
        .as_deref()
        .and_then(parse_time)
        .is_some_and(|at| at > since)
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// The pages a list read got.
struct Pages<T> {
    items: Vec<T>,
    /// The first answer's `Date` header.
    date: Option<DateTime<Utc>>,
    /// More pages than [`MAX_PAGES`]: the rest was not read.
    truncated: bool,
}

impl<T> Default for Pages<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            date: None,
            truncated: false,
        }
    }
}

enum ReadError {
    /// `401` / `403` / `404`: the token may not read it.
    Denied,
    /// Network, server or parse error.
    Failed,
}

/// GETs the pages of `url` (query string open), up to [`MAX_PAGES`], stopping
/// after a page `last` says ends the list.
async fn read_pages<T: serde::de::DeserializeOwned>(
    ctx: &FetchContext,
    url: &str,
    last: impl Fn(&[T]) -> bool,
) -> Result<Pages<T>, ReadError> {
    let mut pages = Pages::default();
    for page in 1..=MAX_PAGES {
        let res = http_client()
            .get(format!("{url}&per_page={PAGE_SIZE}&page={page}"))
            .header("PRIVATE-TOKEN", ctx.token.as_str())
            .send_paced()
            .await
            .map_err(|_| ReadError::Failed)?;
        match res.status().as_u16() {
            200..=299 => {}
            401 | 403 | 404 => return Err(ReadError::Denied),
            _ => return Err(ReadError::Failed),
        }
        if page == 1 {
            pages.date = res
                .headers()
                .get(reqwest::header::DATE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| DateTime::parse_from_rfc2822(v).ok())
                .map(|at| at.with_timezone(&Utc));
        }
        let batch: Vec<T> = res.json().await.map_err(|_| ReadError::Failed)?;
        let done = batch.len() < PAGE_SIZE || last(&batch);
        pages.items.extend(batch);
        if done {
            return Ok(pages);
        }
    }
    tracing::warn!(url, "Sentinel list truncated — a full refresh catches up");
    pages.truncated = true;
    Ok(pages)
}

// ── Deciding ───────────────────────────────────────────────────────────────

/// What to read again on what a check saw.
#[derive(Debug, Default, PartialEq)]
pub struct SentinelPlan {
    /// Tracked MRs to read again, with what their caches must not vouch for.
    pub refresh: BTreeMap<String, Force>,
    /// A tracked branch got commits: scan the branches.
    pub scan_branches: bool,
    /// An MR the app does not track changed: run the discovery poll.
    pub untracked: bool,
}

/// Caches a sentinel refresh bypasses on top of the usual guards.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Force {
    pub review: bool,
    pub pipelines: bool,
}

impl Force {
    pub fn policy(self) -> CachePolicy {
        if self.review || self.pipelines {
            CachePolicy::Signals {
                review: self.review,
                pipelines: self.pipelines,
            }
        } else {
            CachePolicy::Normal
        }
    }
}

/// The MR iid a pipeline ran for: `refs/merge-requests/<iid>/head` (or `/merge`,
/// `/train`), else the open tracked MR whose source branch it is.
fn pipeline_mr<'a>(pipeline: &ListedPipeline, mrs: &'a [TrackedMr]) -> Option<&'a TrackedMr> {
    let git_ref = pipeline.git_ref.as_deref()?;
    if let Some(rest) = git_ref.strip_prefix("refs/merge-requests/") {
        let iid = rest.split('/').next()?;
        return mrs.iter().find(|mr| mr.id == iid);
    }
    mrs.iter()
        .find(|mr| mr.state == GitlabMrState::Opened && mr.source_branch == git_ref)
}

/// Decides what to read again. `branches`: the tracked branches; `seen`: the
/// events of the previous check (shown again by the overlap).
pub fn plan(
    report: &SentinelReport,
    mrs: &[TrackedMr],
    known: &HashSet<String>,
    branches: &[String],
    seen: &HashSet<u64>,
) -> SentinelPlan {
    let mut plan = SentinelPlan::default();
    let tracked = |iid: u64| mrs.iter().find(|mr| mr.id == iid.to_string());

    for listed in &report.mrs {
        match tracked(listed.iid) {
            Some(mr) if mr.updated_at != listed.updated_at => {
                plan.refresh.entry(mr.id.clone()).or_default();
            }
            Some(_) => {}
            None => plan.untracked |= !known.contains(&listed.iid.to_string()),
        }
    }

    for pipeline in &report.pipelines {
        let Some(mr) = pipeline_mr(pipeline, mrs) else {
            continue;
        };
        let known_state = mr
            .pipelines
            .iter()
            .any(|p| p.id == pipeline.id && p.updated_at == pipeline.updated_at);
        if !known_state {
            plan.refresh.entry(mr.id.clone()).or_default().pipelines = true;
        }
    }

    for event in report.events.iter().filter(|e| !seen.contains(&e.id)) {
        if let Some(push) = &event.push_data {
            if push.ref_type.as_deref() != Some("branch") {
                continue;
            }
            let Some(branch) = push.git_ref.as_deref() else {
                continue;
            };
            plan.scan_branches |= branches.iter().any(|b| b == branch);
            // The target moved: mergeability and commits behind of the MRs aiming at it.
            for mr in mrs
                .iter()
                .filter(|mr| mr.state == GitlabMrState::Opened && mr.target_branch == branch)
            {
                plan.refresh.entry(mr.id.clone()).or_default();
            }
            continue;
        }
        let review_of = match event.target_type.as_deref() {
            Some("Note" | "DiffNote" | "DiscussionNote") => event
                .note
                .as_ref()
                .filter(|n| n.noteable_type.as_deref() == Some("MergeRequest"))
                .and_then(|n| n.noteable_iid),
            Some("MergeRequest")
                if matches!(event.action_name.as_str(), "approved" | "unapproved") =>
            {
                event.target_iid
            }
            _ => None,
        };
        if let Some(mr) = review_of.and_then(tracked) {
            plan.refresh.entry(mr.id.clone()).or_default().review = true;
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MrData, Pipeline};

    fn tracked(id: &str, source: &str, target: &str, updated: &str) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
        mr.data = MrData {
            source_branch: source.into(),
            target_branch: target.into(),
            updated_at: Some(updated.into()),
            state: GitlabMrState::Opened,
            ..MrData::default()
        };
        mr
    }

    fn at(text: &str) -> DateTime<Utc> {
        parse_time(text).unwrap()
    }

    fn report() -> SentinelReport {
        SentinelReport {
            local_now: at("2026-11-23T10:02:00Z"),
            server_now: Some(at("2026-11-23T10:02:05Z")),
            truncated: false,
            mrs: Vec::new(),
            pipelines: Vec::new(),
            events: Vec::new(),
        }
    }

    fn event(id: u64, target_type: Option<&str>, action: &str) -> ProjectEvent {
        ProjectEvent {
            id,
            action_name: action.into(),
            target_type: target_type.map(String::from),
            target_iid: None,
            created_at: Some("2026-11-23T10:01:00Z".into()),
            note: None,
            push_data: None,
        }
    }

    fn mrs() -> Vec<TrackedMr> {
        vec![
            tracked("1", "feat/a", "main", "2026-11-23T09:00:00Z"),
            tracked("2", "feat/b", "develop", "2026-11-23T09:00:00Z"),
        ]
    }

    #[test]
    fn a_listed_mr_is_read_again_only_when_its_updated_at_moved() {
        let mut r = report();
        r.mrs = vec![
            ListedMr {
                iid: 1,
                updated_at: Some("2026-11-23T10:01:00Z".into()),
            },
            // Same as known (the overlap shows it again): skipped.
            ListedMr {
                iid: 2,
                updated_at: Some("2026-11-23T09:00:00Z".into()),
            },
        ];
        let plan = plan(&r, &mrs(), &HashSet::new(), &[], &HashSet::new());
        assert_eq!(plan.refresh.keys().collect::<Vec<_>>(), ["1"]);
        assert_eq!(plan.refresh["1"], Force::default());
        assert!(!plan.untracked);
    }

    #[test]
    fn an_untracked_mr_asks_for_discovery_unless_known() {
        let mut r = report();
        r.mrs = vec![ListedMr {
            iid: 9,
            updated_at: None,
        }];
        assert!(plan(&r, &mrs(), &HashSet::new(), &[], &HashSet::new()).untracked);
        let dismissed = HashSet::from(["9".to_string()]);
        assert!(!plan(&r, &mrs(), &dismissed, &[], &HashSet::new()).untracked);
    }

    #[test]
    fn a_pipeline_forces_its_mr_pipelines_by_ref_or_source_branch() {
        let mut list = mrs();
        list[1].data.pipelines = vec![Pipeline {
            id: 20,
            updated_at: Some("2026-11-23T10:01:00Z".into()),
            ..Pipeline::default()
        }];
        let mut r = report();
        r.pipelines = vec![
            ListedPipeline {
                id: 10,
                git_ref: Some("refs/merge-requests/1/head".into()),
                updated_at: Some("2026-11-23T10:01:30Z".into()),
            },
            // Already known in this state: skipped.
            ListedPipeline {
                id: 20,
                git_ref: Some("feat/b".into()),
                updated_at: Some("2026-11-23T10:01:00Z".into()),
            },
            // Nobody's branch.
            ListedPipeline {
                id: 30,
                git_ref: Some("main".into()),
                updated_at: None,
            },
        ];
        let plan = plan(&r, &list, &HashSet::new(), &[], &HashSet::new());
        assert_eq!(plan.refresh.len(), 1);
        assert_eq!(
            plan.refresh["1"],
            Force {
                review: false,
                pipelines: true
            }
        );
    }

    #[test]
    fn comments_and_approvals_force_the_review() {
        let mut r = report();
        let mut note = event(1, Some("DiffNote"), "commented on");
        note.note = Some(EventNote {
            noteable_type: Some("MergeRequest".into()),
            noteable_iid: Some(2),
        });
        let mut approved = event(2, Some("MergeRequest"), "approved");
        approved.target_iid = Some(1);
        let mut issue_note = event(3, Some("Note"), "commented on");
        issue_note.note = Some(EventNote {
            noteable_type: Some("Issue".into()),
            noteable_iid: Some(1),
        });
        r.events = vec![note, approved, issue_note];
        let plan = plan(&r, &mrs(), &HashSet::new(), &[], &HashSet::new());
        assert_eq!(
            plan.refresh["1"],
            Force {
                review: true,
                pipelines: false
            }
        );
        assert_eq!(
            plan.refresh["2"],
            Force {
                review: true,
                pipelines: false
            }
        );
    }

    #[test]
    fn a_push_scans_tracked_branches_and_rereads_the_mrs_aiming_at_it() {
        let mut r = report();
        let mut push = event(1, None, "pushed to");
        push.push_data = Some(PushData {
            ref_type: Some("branch".into()),
            git_ref: Some("main".into()),
        });
        r.events = vec![push];
        let branches = vec!["main".to_string(), "release".to_string()];
        let plan = plan(&r, &mrs(), &HashSet::new(), &branches, &HashSet::new());
        assert!(plan.scan_branches);
        assert_eq!(plan.refresh.keys().collect::<Vec<_>>(), ["1"]);
    }

    #[test]
    fn events_seen_by_the_previous_check_are_skipped() {
        let mut r = report();
        let mut approved = event(7, Some("MergeRequest"), "approved");
        approved.target_iid = Some(1);
        r.events = vec![approved];
        let seen = HashSet::from([7]);
        assert_eq!(
            plan(&r, &mrs(), &HashSet::new(), &[], &seen),
            SentinelPlan::default()
        );
    }

    #[test]
    fn the_watermark_follows_the_server_clock_minus_the_overlap() {
        let mut state = SentinelState::new(at("2026-11-23T10:00:00Z"));
        let r = report();
        state.advance(&r);
        assert_eq!(state.since, at("2026-11-23T10:01:35Z"));
        assert_eq!(state.skew, TimeDelta::seconds(5));
        // No Date header: our clock plus the known skew.
        let mut r2 = report();
        r2.server_now = None;
        r2.local_now = at("2026-11-23T10:04:00Z");
        state.advance(&r2);
        assert_eq!(state.since, at("2026-11-23T10:03:35Z"));
    }

    #[test]
    fn only_events_after_the_watermark_count() {
        let since = at("2026-11-23T10:00:00Z");
        let mut e = event(1, None, "pushed to");
        assert!(event_after(&e, since));
        e.created_at = Some("2026-11-23T09:59:59Z".into());
        assert!(!event_after(&e, since));
        e.created_at = None;
        assert!(!event_after(&e, since));
    }
}
