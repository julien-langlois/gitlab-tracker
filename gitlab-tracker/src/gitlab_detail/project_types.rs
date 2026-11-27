//! What the project-wide pages read: deployments, branch pipelines, merge trains, schedules, releases.

use super::*;

/// A deployment of an environment, as the environments page lists them.
#[derive(Debug, Clone, PartialEq)]
pub struct DeploymentRecord {
    pub environment: String,
    /// The branch (or tag) deployed.
    pub reference: String,
    pub sha: String,
    pub created_at: String,
    /// `success`, `failed`, `running`, `canceled`…
    pub status: String,
    /// Who started it, by name.
    pub user: String,
}

/// The latest pipeline of a tracked branch.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchPipeline {
    pub branch: String,
    pub id: u64,
    /// The commit it ran on: the branch's head when it started.
    pub sha: String,
    pub status: crate::models::PipelineState,
    pub created_at: String,
    pub updated_at: String,
    pub web_url: String,
    /// The jobs that failed in it, by name.
    pub failed_jobs: Vec<String>,
    /// The branch's recent pipelines, newest first, this one included: when
    /// it turned red, and since what.
    pub history: Vec<PipelineRun>,
}

/// A past pipeline of a tracked branch.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineRun {
    pub id: u64,
    pub sha: String,
    pub status: crate::models::PipelineState,
    pub created_at: String,
    pub web_url: String,
}

/// What the environments and CI pages read about the project itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectOverview {
    /// Newest first.
    pub deployments: Vec<DeploymentRecord>,
    /// One per tracked branch that has a pipeline.
    pub pipelines: Vec<BranchPipeline>,
    /// The project's merges, tracked or not: the team's pace for the
    /// milestones' forecast.
    pub merges: MergeHistory,
    /// The releases: GitLab's when the project has some, else its tags named
    /// like a version. Newest first.
    pub releases: Vec<ReleaseRecord>,
    /// The token may not read the releases (403): only the tags are shown.
    pub releases_denied: bool,
    /// The project's deploy freezes, as declared in GitLab.
    pub freezes: Vec<crate::freeze::Declared>,
    /// The token may not read them (403).
    pub freezes_denied: bool,
    /// The project's pipeline schedules, each with its latest runs.
    pub schedules: Vec<ScheduleRecord>,
    /// The token may not read them (403).
    pub schedules_denied: bool,
    /// The cars of the project's merge trains (GitLab Premium), first car
    /// first; empty on GitLab Free or without trains.
    pub trains: Vec<TrainCar>,
}

/// An MR on a merge train.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainCar {
    /// The MR's iid.
    pub iid: String,
    pub title: String,
    pub target_branch: String,
    /// When it joined the train (RFC 3339).
    pub added_at: String,
    /// Its train pipeline: the MR on top of every car before it.
    pub pipeline: Option<crate::models::PipelineState>,
    pub pipeline_created_at: Option<String>,
    /// Who put it on the train.
    pub user: String,
}

/// A pipeline schedule (CI/CD › Schedules).
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleRecord {
    pub id: u64,
    pub description: String,
    /// The branch or tag it runs on.
    pub ref_name: String,
    pub cron: String,
    /// The zone its cron is read in (an IANA name, or `UTC`).
    pub zone: String,
    /// When it runs next (RFC 3339), as GitLab says.
    pub next_run_at: Option<String>,
    pub active: bool,
    /// Its owner's name: the pipelines run as them.
    pub owner: String,
    /// The owner's account state: a blocked or deactivated owner's schedule
    /// no longer runs.
    pub owner_state: String,
    /// Its latest runs, oldest first ([`SCHEDULE_RUNS`] at most).
    pub runs: Vec<ScheduleRun>,
}

/// A pipeline a schedule started.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleRun {
    pub id: u64,
    pub status: crate::models::PipelineState,
    pub created_at: String,
    pub web_url: String,
}

/// The runs of a schedule read: the strip of the CI page.
pub const SCHEDULE_RUNS: usize = 14;

/// Schedules whose runs are read, at most (one request each).
pub(super) const SCHEDULES_READ: usize = 20;

/// A release, or a tag named like a version.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseRecord {
    pub tag: String,
    /// The release's own name when it has one other than its tag.
    pub name: Option<String>,
    /// The tagged commit.
    pub sha: String,
    /// When the tagged commit was made (RFC 3339): what reached the branch
    /// before is in the release.
    pub committed_at: String,
    /// When it was released (a GitLab release), else none.
    pub released_at: Option<String>,
    /// The release's notes (Markdown), when written in GitLab.
    pub notes: Option<String>,
    /// The release's page; empty for a bare tag.
    pub web_url: String,
    /// A GitLab release, not a bare tag.
    pub is_release: bool,
}

/// Whether `tag` reads as a version: `v1.2`, `1.2.3`, `v2.0.0-rc.1`.
pub fn is_version_tag(tag: &str) -> bool {
    let bare = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    let core = bare.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}
