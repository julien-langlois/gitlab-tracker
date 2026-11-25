//! Plays a [`ProjectSpec`] back: each MR's log gives its commits, pipelines,
//! review timeline and threads; the merges, promotions and copies, in time
//! order, give what each branch holds, its deployments and its pipelines.
//! The app's data is then read from this one history, so every page agrees.

use super::clock::{clock, hash, iso, sha, When};
use super::model::*;
use crate::models::{
    BranchCommits, ChangedFile, CommitFingerprint, DiffStats, GitlabMrState, MergeabilityStatus,
    MrCommit, MrStatus, MrThread, Pipeline, PipelineJob, PipelineState, ReviewActivity, TrackedMr,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

mod answers;
mod play;
mod snapshots;

pub type At = DateTime<Utc>;

pub struct NoteRec {
    pub id: u64,
    pub by: &'static Person,
    pub body: &'static str,
    pub at: At,
}

pub struct ThreadRec {
    pub key: &'static str,
    pub place: Option<(&'static str, u32)>,
    pub notes: Vec<NoteRec>,
    pub resolved: Option<At>,
}

pub struct CommitRec {
    pub sha: String,
    pub title: &'static str,
    pub authored: At,
    pub pushed: At,
    /// Who wrote it: the MR's author, unless someone else pushed it.
    pub by: &'static Person,
}

pub struct JobRec {
    pub id: u64,
    pub name: &'static str,
    pub stage: &'static str,
    pub status: &'static str,
    pub duration: Option<f64>,
    pub queued: Option<f64>,
}

/// Whose pipeline: an MR's (its index) or a branch's.
#[derive(Clone, PartialEq)]
pub enum Owner {
    Mr(usize),
    Branch(&'static str),
}

pub struct PipeRec {
    pub id: u64,
    pub owner: Owner,
    pub sha: String,
    pub created: At,
    pub finished: Option<At>,
    pub status: PipelineState,
    pub jobs: Vec<JobRec>,
}

/// A commit of a branch's first-parent history.
pub struct LogEntry {
    pub at: At,
    pub sha: String,
    pub title: String,
    pub author: &'static str,
    /// The MR it merged (by index), for a merge commit.
    pub mr: Option<usize>,
}

pub struct DeployRec {
    pub environment: &'static str,
    pub branch: &'static str,
    pub sha: String,
    pub at: At,
    pub ok: bool,
    pub user: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum How {
    Merge,
    CherryPick,
    Copy,
}

pub struct Arrival {
    pub mr: usize,
    pub branch: &'static str,
    pub at: At,
    /// Read by the demo's stats database only.
    #[cfg_attr(not(feature = "stats"), allow(dead_code))]
    pub how: How,
}

pub struct BuiltMr {
    pub spec: MrSpec,
    pub iid: String,
    pub opened: At,
    pub events: Vec<(At, Ev)>,
    pub commits: Vec<CommitRec>,
    /// Where its branch starts from on the target (opened, or last rebased).
    pub base: At,
    pub merged: Option<(At, &'static Person)>,
    pub closed: Option<(At, &'static Person)>,
    pub merge_sha: String,
    pub threads: Vec<ThreadRec>,
    pub approved: Vec<(&'static Person, At)>,
    pub reviewers: Vec<&'static Person>,
    pub updated: At,
}

impl BuiltMr {
    pub fn head(&self) -> Option<&str> {
        self.commits.last().map(|c| c.sha.as_str())
    }

    pub fn state(&self) -> GitlabMrState {
        if self.merged.is_some() {
            GitlabMrState::Merged
        } else if self.closed.is_some() {
            GitlabMrState::Closed
        } else {
            GitlabMrState::Opened
        }
    }

    /// The commit GitLab lists for the MR: its merge commit once merged.
    pub fn sha(&self) -> Option<String> {
        if self.merged.is_some() {
            Some(self.merge_sha.clone())
        } else {
            self.head().map(str::to_string)
        }
    }

    pub fn landed(&self) -> Option<CommitFingerprint> {
        let (at, by) = self.merged?;
        Some(CommitFingerprint {
            author_email: by.email(),
            authored_at: at.timestamp(),
            title: format!(
                "Merge branch '{}' into '{}'",
                self.spec.source, self.spec.target
            ),
        })
    }

    pub fn fingerprint(&self, commit: &CommitRec) -> CommitFingerprint {
        CommitFingerprint {
            author_email: commit.by.email(),
            authored_at: commit.authored.timestamp(),
            title: commit.title.to_string(),
        }
    }
}

pub struct World {
    pub spec: ProjectSpec,
    pub now: At,
    pub mrs: Vec<BuiltMr>,
    pub pipelines: Vec<PipeRec>,
    /// First-parent history of each tracked branch, oldest first.
    pub logs: HashMap<&'static str, Vec<LogEntry>>,
    /// The MRs on each tracked branch under their own SHAs, and since when.
    pub present: HashMap<&'static str, HashMap<usize, At>>,
    pub copies: HashMap<&'static str, BranchCommits>,
    pub arrivals: Vec<Arrival>,
    pub deployments: Vec<DeployRec>,
}

/// One change of a branch, in time order.
enum Move {
    Merge(usize),
    Sync(usize),
    Copy(usize),
    Direct(usize),
}

fn stage_order(jobs: &[Job]) -> Vec<&'static str> {
    let mut stages: Vec<&'static str> = Vec::new();
    for (_, stage, _) in jobs {
        if !stages.contains(stage) {
            stages.push(stage);
        }
    }
    stages
}

/// The jobs of one pipeline run: their statuses, durations and when it ended.
fn run_jobs(
    jobs: &[Job],
    ci: Ci,
    created: At,
    now: At,
    seed: u64,
) -> (PipelineState, Vec<JobRec>, Option<At>) {
    let failing: &[&str] = match ci {
        Ci::Fail(names) => names,
        _ => &[],
    };
    let jitter = |j: usize, what: &str| {
        (hash(&[&seed.to_string(), &j.to_string(), what]) % 1000) as f64 / 1000.0
    };
    let mut out = Vec::new();
    let mut elapsed = 0.0_f64;
    let running_for = (now - created).num_seconds() as f64;
    let mut stopped = false;
    let mut state = match ci {
        Ci::Pass | Ci::Flaky(_) => PipelineState::Success,
        Ci::Fail(_) => PipelineState::Failed,
        Ci::Running => PipelineState::Running,
        Ci::Pending => PipelineState::Pending,
        Ci::Canceled => PipelineState::Canceled,
    };
    for (index, stage) in stage_order(jobs).into_iter().enumerate() {
        let mut longest = 0.0_f64;
        let mut failed_here = false;
        for (j, (name, job_stage, secs)) in jobs.iter().enumerate() {
            if job_stage != &stage {
                continue;
            }
            let queued = (4.0 + jitter(j, "queue") * 64.0).round();
            let mut duration = (secs * (0.82 + jitter(j, "run") * 0.36)).round();
            let status: &'static str = if stopped {
                match ci {
                    Ci::Canceled => "canceled",
                    Ci::Running | Ci::Pending => "created",
                    _ => "skipped",
                }
            } else {
                match ci {
                    Ci::Pending => "pending",
                    Ci::Canceled if index > 0 => "canceled",
                    Ci::Running if elapsed + queued + duration > running_for => "running",
                    _ if failing.contains(name) => {
                        failed_here = true;
                        duration = (duration * 0.6).round();
                        "failed"
                    }
                    _ => "success",
                }
            };
            let ran = matches!(status, "success" | "failed");
            if ran || status == "running" {
                longest = longest.max(queued + duration);
            }
            out.push(JobRec {
                id: 0,
                name,
                stage,
                status,
                duration: ran.then_some(duration),
                queued: (ran || status == "running").then_some(queued),
            });
        }
        if !stopped {
            elapsed += longest;
        }
        let running_here = out.iter().any(|j| j.status == "running");
        if failed_here
            || running_here
            || matches!(ci, Ci::Pending)
            || matches!(ci, Ci::Canceled) && index == 0
        {
            stopped = true;
        }
    }
    if matches!(ci, Ci::Running) && !out.iter().any(|j| j.status == "running") {
        // Done already: it passed.
        state = PipelineState::Success;
    }
    let finished = match state {
        PipelineState::Running | PipelineState::Pending => None,
        PipelineState::Canceled => Some(created + Duration::seconds(elapsed as i64 + 40)),
        _ => Some(created + Duration::seconds(elapsed as i64)),
    };
    (state, out, finished)
}

impl World {
    pub fn build(mut spec: ProjectSpec) -> World {
        let c = clock();
        let now = c.now;
        let specs = std::mem::take(&mut spec.mrs);
        let mut world = World {
            spec,
            now,
            mrs: Vec::new(),
            pipelines: Vec::new(),
            logs: HashMap::new(),
            present: HashMap::new(),
            copies: HashMap::new(),
            arrivals: Vec::new(),
            deployments: Vec::new(),
        };

        // Each MR's own story.
        for spec in specs {
            let mrs_len = world.mrs.len();
            let built = world.play(spec, mrs_len);
            world.mrs.push(built);
        }

        // The iids, in the order the MRs were opened; the gaps are MRs the
        // tracker does not follow.
        let mut order: Vec<usize> = (0..world.mrs.len()).collect();
        order.sort_by_key(|&i| world.mrs[i].opened);
        let mut iid = world.spec.first_iid;
        for i in order {
            world.mrs[i].iid = iid.to_string();
            iid += 1 + u32::from(hash(&[world.mrs[i].spec.key]).is_multiple_of(3));
        }

        world.branches_play();
        world.number();
        world
    }

    fn at(&self, when: When) -> At {
        clock().at(when)
    }
}
