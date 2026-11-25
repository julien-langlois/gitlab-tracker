//! The project-wide reads: the overview, merge trains, pipeline schedules, deploy freezes.

use super::*;

// The merges as kept between launches (gitlab-tracker-gitlab).
pub use gitlab_tracker_gitlab::history::{MergeHistory, MERGE_HISTORY_DAYS};

/// Pages of 100 merged MRs read at most in one go; a read cut short goes on
/// from where it stopped at the next one.
pub(super) const MERGE_HISTORY_PAGES: u32 = 5;

/// Overlap with the last read, for clocks and MRs updated while it ran.
pub(super) const MERGE_OVERLAP_MINUTES: i64 = 10;

/// Where to read a project-level overview.
#[derive(Clone)]
pub struct ProjectTarget {
    pub base_url: String,
    pub token: crate::gitlab::ApiToken,
    pub project_id: String,
}

impl ProjectTarget {
    pub(super) fn as_target(&self) -> Target {
        Target {
            base_url: self.base_url.clone(),
            token: self.token.clone(),
            project_id: self.project_id.clone(),
            mr_id: String::new(),
            source_branch: String::new(),
            target_branch: String::new(),
            sha: None,
        }
    }
}

/// Reads the project's latest deployments (100 at most), the latest pipeline
/// of each of `branches` with the jobs that failed in it, and its merges since
/// `merges` was last read (all of the period on the first read). A part that
/// cannot be read is left empty; the merges keep what they had.
pub async fn fetch_project(
    target: ProjectTarget,
    branches: Vec<String>,
    merges: MergeHistory,
    cache: Option<crate::storage::ProjectStore>,
) -> ProjectOverview {
    let target = target.as_target();
    let deployments = project_deployments(&target);
    let pipelines = futures::future::join_all(
        branches
            .iter()
            .map(|branch| branch_pipeline(&target, branch)),
    );
    let merges = async {
        // Not known in this session yet: what the last launch left on disk.
        let known = match (&merges.read_at, &cache) {
            (None, Some(store)) => store
                .load_merges()
                .await
                .map(|mut h| {
                    // The database keeps them all: the period's only.
                    retain_period(&mut h.merged, chrono::Utc::now());
                    h
                })
                .unwrap_or(merges),
            _ => merges,
        };
        let updated = project_merges(&target, known.clone()).await;
        if let Some(store) = cache.filter(|_| updated != known) {
            store.save_merges(&updated, &known).await;
        }
        updated
    };
    let releases = project_releases(&target);
    let freezes = project_freezes(&target);
    let schedules = project_schedules(&target);
    let trains = project_trains(&target);
    let (
        deployments,
        pipelines,
        merges,
        (releases, releases_denied),
        (freezes, freezes_denied),
        (schedules, schedules_denied),
        trains,
    ) = tokio::join!(
        deployments,
        pipelines,
        merges,
        releases,
        freezes,
        schedules,
        trains
    );
    ProjectOverview {
        deployments,
        pipelines: pipelines.into_iter().flatten().collect(),
        merges,
        releases,
        releases_denied,
        freezes,
        freezes_denied,
        schedules,
        schedules_denied,
        trains,
    }
}

/// The cars of the project's merge trains, first car first. Empty on GitLab
/// Free (the endpoint is Premium's), without trains, or without the
/// permission: the window simulates the queue from the MRs set to auto-merge.
pub(super) async fn project_trains(target: &Target) -> Vec<TrainCar> {
    #[derive(serde::Deserialize)]
    struct RawMr {
        iid: u64,
        #[serde(default)]
        title: String,
    }
    #[derive(serde::Deserialize)]
    struct RawPipeline {
        #[serde(default)]
        status: crate::models::PipelineState,
        #[serde(default)]
        created_at: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct RawUser {
        #[serde(default)]
        name: String,
    }
    #[derive(serde::Deserialize)]
    struct Raw {
        id: u64,
        merge_request: RawMr,
        #[serde(default)]
        user: Option<RawUser>,
        #[serde(default)]
        pipeline: Option<RawPipeline>,
        #[serde(default)]
        created_at: String,
        #[serde(default)]
        target_branch: String,
    }
    let Ok(mut cars) = target
        .get::<Vec<Raw>>("merge_trains?scope=active&sort=asc&per_page=100")
        .await
    else {
        return Vec::new();
    };
    cars.sort_by_key(|c| c.id);
    cars.into_iter()
        .map(|c| TrainCar {
            iid: c.merge_request.iid.to_string(),
            title: c.merge_request.title,
            target_branch: c.target_branch,
            added_at: c.created_at,
            pipeline_created_at: c.pipeline.as_ref().and_then(|p| p.created_at.clone()),
            pipeline: c.pipeline.map(|p| p.status),
            user: c.user.map(|u| u.name).unwrap_or_default(),
        })
        .collect()
}

/// The project's pipeline schedules, the active ones first, each with its
/// latest runs; whether the token may not read them (403).
pub(super) async fn project_schedules(target: &Target) -> (Vec<ScheduleRecord>, bool) {
    #[derive(serde::Deserialize)]
    struct Owner {
        #[serde(default)]
        name: String,
        #[serde(default)]
        state: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Raw {
        id: u64,
        #[serde(default)]
        description: Option<String>,
        #[serde(rename = "ref", default)]
        ref_name: Option<String>,
        cron: String,
        #[serde(default)]
        cron_timezone: Option<String>,
        #[serde(default)]
        next_run_at: Option<String>,
        #[serde(default)]
        active: bool,
        #[serde(default)]
        owner: Option<Owner>,
    }
    #[derive(serde::Deserialize)]
    struct RawRun {
        id: u64,
        #[serde(default)]
        status: crate::models::PipelineState,
        #[serde(default)]
        created_at: Option<String>,
        #[serde(default)]
        web_url: Option<String>,
    }
    let list = match target
        .get::<Vec<Raw>>("pipeline_schedules?per_page=100")
        .await
    {
        Ok(list) => list,
        Err(e) => return (Vec::new(), e.contains("403")),
    };
    let mut list = list;
    list.sort_by_key(|s| (!s.active, s.id));
    list.truncate(SCHEDULES_READ);
    let runs = futures::future::join_all(list.iter().map(|s| async move {
        let path = format!(
            "pipeline_schedules/{}/pipelines?sort=desc&per_page={SCHEDULE_RUNS}",
            s.id
        );
        target.get::<Vec<RawRun>>(&path).await
    }))
    .await;
    let schedules = list
        .into_iter()
        .zip(runs)
        .map(|(s, runs)| {
            let mut runs: Vec<ScheduleRun> = runs
                .unwrap_or_default()
                .into_iter()
                .map(|r| ScheduleRun {
                    id: r.id,
                    status: r.status,
                    created_at: r.created_at.unwrap_or_default(),
                    web_url: r.web_url.unwrap_or_default(),
                })
                .collect();
            // Oldest first, the latest ones whatever order the server kept.
            runs.sort_by_key(|r| r.id);
            let skip = runs.len().saturating_sub(SCHEDULE_RUNS);
            runs.drain(..skip);
            let owner = s.owner.unwrap_or(Owner {
                name: String::new(),
                state: None,
            });
            ScheduleRecord {
                id: s.id,
                description: s.description.unwrap_or_default(),
                ref_name: s.ref_name.unwrap_or_default(),
                cron: s.cron,
                zone: s.cron_timezone.unwrap_or_else(|| "UTC".into()),
                next_run_at: s.next_run_at,
                active: s.active,
                owner: owner.name,
                owner_state: owner.state.unwrap_or_else(|| "active".into()),
                runs,
            }
        })
        .collect();
    (schedules, false)
}

/// The project's deploy freezes (Settings › CI/CD › Deploy freezes), and
/// whether the token may not read them (403).
pub(super) async fn project_freezes(target: &Target) -> (Vec<crate::freeze::Declared>, bool) {
    #[derive(serde::Deserialize)]
    struct Raw {
        freeze_start: String,
        freeze_end: String,
        #[serde(default)]
        cron_timezone: Option<String>,
    }
    match target.get::<Vec<Raw>>("freeze_periods?per_page=100").await {
        Ok(list) => (
            list.into_iter()
                .map(|p| crate::freeze::Declared {
                    start: p.freeze_start,
                    end: p.freeze_end,
                    zone: p.cron_timezone.unwrap_or_else(|| "UTC".into()),
                })
                .collect(),
            false,
        ),
        Err(e) => (Vec::new(), e.contains("403")),
    }
}
