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
            author_email: self.spec.author.email(),
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

    /// Plays an MR's log.
    fn play(&mut self, spec: MrSpec, index: usize) -> BuiltMr {
        let project = self.spec.id;
        let mut events: Vec<(At, Ev)> = spec.log.iter().map(|(w, e)| (self.at(*w), *e)).collect();
        events.sort_by_key(|(at, _)| *at);
        let opened = events.first().map_or(self.now, |(at, _)| *at);
        let mut built = BuiltMr {
            iid: String::new(),
            opened,
            events: Vec::new(),
            commits: Vec::new(),
            base: opened,
            merged: None,
            closed: None,
            merge_sha: sha(&[project, spec.key, "merge"]),
            threads: Vec::new(),
            approved: Vec::new(),
            reviewers: Vec::new(),
            updated: opened,
            spec,
        };
        let mut rebases = 0;
        let mut pushes = 0;
        for (at, ev) in &events {
            built.updated = built.updated.max(*at);
            match *ev {
                Ev::Open => {}
                Ev::Push(titles, ci) => {
                    let n = titles.len();
                    for (i, title) in titles.iter().enumerate() {
                        let k = built.commits.len();
                        built.commits.push(CommitRec {
                            sha: sha(&[project, built.spec.key, &k.to_string(), "0"]),
                            title,
                            authored: *at - Duration::minutes(((n - 1 - i) * 23) as i64),
                            pushed: *at,
                        });
                    }
                    pushes += 1;
                    let head = built.head().unwrap_or_default().to_string();
                    self.pipeline(Owner::Mr(index), head, *at, ci, pushes);
                }
                Ev::Rebase(ci) => {
                    rebases += 1;
                    for (k, commit) in built.commits.iter_mut().enumerate() {
                        commit.sha = sha(&[
                            project,
                            built.spec.key,
                            &k.to_string(),
                            &rebases.to_string(),
                        ]);
                        commit.pushed = *at;
                    }
                    built.base = *at;
                    pushes += 1;
                    let head = built.head().unwrap_or_default().to_string();
                    self.pipeline(Owner::Mr(index), head, *at, ci, pushes);
                }
                Ev::Ask(person) => {
                    if !built.reviewers.contains(&person) {
                        built.reviewers.push(person);
                    }
                }
                Ev::Note(by, key, place, body) => built.threads.push(ThreadRec {
                    key,
                    place,
                    notes: vec![NoteRec {
                        id: 0,
                        by,
                        body,
                        at: *at,
                    }],
                    resolved: None,
                }),
                Ev::Reply(by, key, body) => {
                    if let Some(thread) = built.threads.iter_mut().find(|t| t.key == key) {
                        thread.notes.push(NoteRec {
                            id: 0,
                            by,
                            body,
                            at: *at,
                        });
                    }
                }
                Ev::Resolve(_, key) => {
                    if let Some(thread) = built.threads.iter_mut().find(|t| t.key == key) {
                        thread.resolved = Some(*at);
                    }
                }
                Ev::Approve(by) => {
                    built.approved.retain(|(p, _)| *p != by);
                    built.approved.push((by, *at));
                }
                Ev::Changes(_) => {}
                Ev::Merge(by) => built.merged = Some((*at, by)),
                Ev::Close(by) => built.closed = Some((*at, by)),
            }
        }
        built.events = events;
        built
    }

    /// A pipeline of `owner` on `sha`, started a minute after `at`; a flaky
    /// run is followed by a passing one on the same commit.
    fn pipeline(&mut self, owner: Owner, sha: String, at: At, ci: Ci, n: usize) -> Option<At> {
        let jobs = match &owner {
            Owner::Mr(_) => self.spec.jobs.clone(),
            Owner::Branch(branch) => self.branch_jobs(branch),
        };
        let seed = hash(&[&sha, &n.to_string()]);
        let created = at + Duration::minutes(1);
        let first = match ci {
            Ci::Flaky(job) => Ci::Fail(Box::leak(Box::new([job]))),
            other => other,
        };
        let (status, jobs_run, finished) = run_jobs(&jobs, first, created, self.now, seed);
        self.pipelines.push(PipeRec {
            id: 0,
            owner: owner.clone(),
            sha: sha.clone(),
            created,
            finished,
            status,
            jobs: jobs_run,
        });
        if let (Ci::Flaky(_), Some(done)) = (ci, finished) {
            let again = done + Duration::minutes(9 + (seed % 20) as i64);
            let (status, jobs_run, finished) =
                run_jobs(&jobs, Ci::Pass, again, self.now, seed.rotate_left(7));
            self.pipelines.push(PipeRec {
                id: 0,
                owner,
                sha,
                created: again,
                finished,
                status,
                jobs: jobs_run,
            });
            return finished;
        }
        finished
    }

    fn branch_jobs(&self, branch: &str) -> Vec<Job> {
        let mut jobs = self.spec.jobs.clone();
        if let Some(env) = self.environment(branch) {
            let name: &'static str = Box::leak(format!("deploy-{env}").into_boxed_str());
            jobs.push((name, "deploy", 140.0));
        }
        jobs
    }

    pub fn environment(&self, branch: &str) -> Option<&'static str> {
        self.spec
            .environments
            .iter()
            .find(|(b, _)| *b == branch)
            .map(|(_, env)| *env)
    }

    fn tracked(&self, branch: &str) -> Option<&'static str> {
        self.spec.branches.iter().copied().find(|b| *b == branch)
    }

    pub fn mr(&self, key: &str) -> Option<usize> {
        self.mrs.iter().position(|m| m.spec.key == key)
    }

    /// Plays the merges, promotions, copies and direct pushes in time order.
    fn branches_play(&mut self) {
        let mut moves: Vec<(At, Move)> = Vec::new();
        for (i, mr) in self.mrs.iter().enumerate() {
            if let Some((at, _)) = mr.merged {
                moves.push((at, Move::Merge(i)));
            }
        }
        for (i, s) in self.spec.syncs.iter().enumerate() {
            moves.push((clock().at(s.at), Move::Sync(i)));
        }
        for (i, s) in self.spec.copies.iter().enumerate() {
            moves.push((clock().at(s.at), Move::Copy(i)));
        }
        for (i, s) in self.spec.directs.iter().enumerate() {
            moves.push((clock().at(s.at), Move::Direct(i)));
        }
        moves.sort_by_key(|(at, _)| *at);
        let mut shas: HashMap<&'static str, HashSet<String>> = HashMap::new();
        // Head changes per branch: when, SHA, who, how the deployment went.
        let mut heads: Vec<(&'static str, At, String, &'static str, Deploy)> = Vec::new();
        let project = self.spec.id;
        for (at, mv) in moves {
            if at > self.now {
                continue;
            }
            match mv {
                Move::Merge(i) => {
                    let mr = &self.mrs[i];
                    let Some(branch) = self.tracked(mr.spec.target) else {
                        continue;
                    };
                    let by = mr.merged.map_or("", |(_, p)| p.name);
                    self.present.entry(branch).or_default().insert(i, at);
                    let set = shas.entry(branch).or_default();
                    set.insert(mr.merge_sha.clone());
                    set.extend(mr.commits.iter().map(|c| c.sha.clone()));
                    self.logs.entry(branch).or_default().push(LogEntry {
                        at,
                        sha: mr.merge_sha.clone(),
                        title: mr.spec.title.to_string(),
                        author: by,
                        mr: Some(i),
                    });
                    heads.push((branch, at, mr.merge_sha.clone(), by, Deploy::Ok));
                }
                Move::Sync(s) => {
                    let sync = self.spec.syncs[s].clone();
                    let from: Vec<(usize, At)> = self
                        .present
                        .get(sync.from)
                        .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
                        .unwrap_or_default();
                    let to = self.present.entry(sync.to).or_default();
                    let mut arrived = Vec::new();
                    for (mr, _) in from {
                        if let std::collections::hash_map::Entry::Vacant(slot) = to.entry(mr) {
                            slot.insert(at);
                            arrived.push(mr);
                        }
                    }
                    arrived.sort_unstable();
                    for mr in arrived {
                        self.arrivals.push(Arrival {
                            mr,
                            branch: sync.to,
                            at,
                            how: How::Merge,
                        });
                    }
                    let carried = self.copies.get(sync.from).cloned().unwrap_or_default();
                    let copies = self.copies.entry(sync.to).or_default();
                    for (k, v) in carried.picked_from {
                        copies.picked_from.entry(k).or_insert(v);
                    }
                    for (k, v) in carried.fingerprints {
                        copies.fingerprints.entry(k).or_insert(v);
                    }
                    let carried_shas = shas.get(sync.from).cloned().unwrap_or_default();
                    let commit = sha(&[project, sync.from, sync.to, &at.timestamp().to_string()]);
                    let set = shas.entry(sync.to).or_default();
                    set.extend(carried_shas);
                    set.insert(commit.clone());
                    self.logs.entry(sync.to).or_default().push(LogEntry {
                        at,
                        sha: commit.clone(),
                        title: format!("Merge branch '{}' into '{}'", sync.from, sync.to),
                        author: sync.by.name,
                        mr: None,
                    });
                    heads.push((sync.to, at, commit, sync.by.name, sync.deploy));
                }
                Move::Copy(s) => {
                    let copy = self.spec.copies[s].clone();
                    let Some(i) = self.mr(copy.mr) else {
                        continue;
                    };
                    let mr = &self.mrs[i];
                    let ts = at.timestamp();
                    let entry = self.copies.entry(copy.to).or_default();
                    match copy.how {
                        CopyHow::CherryPick => {
                            entry.picked_from.insert(mr.merge_sha.clone(), ts);
                        }
                        CopyHow::Plain => {
                            if let Some(fp) = mr.landed() {
                                entry.fingerprints.insert(fp, ts);
                            }
                        }
                        CopyHow::Partial(n) => {
                            for commit in mr.commits.iter().take(n) {
                                entry.fingerprints.insert(mr.fingerprint(commit), ts);
                            }
                        }
                    }
                    if copy.how != CopyHow::Partial(0) && !matches!(copy.how, CopyHow::Partial(_)) {
                        self.arrivals.push(Arrival {
                            mr: i,
                            branch: copy.to,
                            at,
                            how: if copy.how == CopyHow::CherryPick {
                                How::CherryPick
                            } else {
                                How::Copy
                            },
                        });
                    }
                    let commit = sha(&[project, copy.mr, copy.to, "copy"]);
                    shas.entry(copy.to).or_default().insert(commit.clone());
                    self.logs.entry(copy.to).or_default().push(LogEntry {
                        at,
                        sha: commit.clone(),
                        title: mr.spec.title.to_string(),
                        author: copy.by.name,
                        mr: None,
                    });
                    heads.push((copy.to, at, commit, copy.by.name, Deploy::Ok));
                }
                Move::Direct(s) => {
                    let direct = self.spec.directs[s].clone();
                    let commit = sha(&[project, direct.branch, direct.title]);
                    shas.entry(direct.branch)
                        .or_default()
                        .insert(commit.clone());
                    self.logs.entry(direct.branch).or_default().push(LogEntry {
                        at,
                        sha: commit.clone(),
                        title: direct.title.to_string(),
                        author: direct.by.name,
                        mr: None,
                    });
                    heads.push((direct.branch, at, commit, direct.by.name, Deploy::Ok));
                }
            }
        }
        for (branch, set) in shas {
            self.copies.entry(branch).or_default().shas = set;
        }

        // Each head runs the branch's pipeline, which deploys it.
        let last: HashMap<&'static str, usize> = heads
            .iter()
            .enumerate()
            .map(|(i, (branch, ..))| (*branch, i))
            .collect();
        for (i, (branch, at, commit, user, deploy)) in heads.into_iter().enumerate() {
            let latest = last.get(branch) == Some(&i);
            let env = self.environment(branch);
            let deploy_job: &'static [&'static str] = match env {
                Some(env) => Box::leak(Box::new([&*Box::leak(
                    format!("deploy-{env}").into_boxed_str(),
                )])),
                None => &[],
            };
            let ci = match deploy {
                Deploy::Failed | Deploy::FailedThenOk(_) => Ci::Fail(deploy_job),
                Deploy::Ok if latest => self
                    .spec
                    .branch_ci
                    .iter()
                    .find(|(b, _)| *b == branch)
                    .map_or(Ci::Pass, |(_, ci)| *ci),
                Deploy::Ok => Ci::Pass,
            };
            let finished = self.pipeline(Owner::Branch(branch), commit.clone(), at, ci, 0);
            let Some(env) = env else { continue };
            let Some(done) = finished else { continue };
            let failed_pipeline = matches!(ci, Ci::Fail(_));
            let failed_deploy = matches!(deploy, Deploy::Failed | Deploy::FailedThenOk(_));
            if failed_pipeline && !failed_deploy {
                continue;
            }
            self.deployments.push(DeployRec {
                environment: env,
                branch,
                sha: commit.clone(),
                at: done,
                ok: !failed_deploy,
                user,
            });
            if let Deploy::FailedThenOk(minutes) = deploy {
                let again = done + Duration::minutes(minutes);
                if again <= self.now {
                    let finished =
                        self.pipeline(Owner::Branch(branch), commit.clone(), again, Ci::Pass, 1);
                    self.deployments.push(DeployRec {
                        environment: env,
                        branch,
                        sha: commit,
                        at: finished.unwrap_or(again),
                        ok: true,
                        user,
                    });
                }
            }
        }
        self.deployments.sort_by_key(|d| std::cmp::Reverse(d.at));
    }

    /// GitLab's ids, growing with time: pipelines, jobs, notes.
    fn number(&mut self) {
        let base = hash(&[self.spec.id]) % 50_000;
        let mut order: Vec<usize> = (0..self.pipelines.len()).collect();
        order.sort_by_key(|&i| self.pipelines[i].created);
        let mut job_id = 6_812_400_000 + base * 1000;
        for (n, i) in order.into_iter().enumerate() {
            let p = &mut self.pipelines[i];
            p.id = 1_240_000 + base + n as u64 * 7 + hash(&[&p.sha]) % 5;
            for job in &mut p.jobs {
                job_id += 1 + hash(&[&p.sha, job.name]) % 9;
                job.id = job_id;
            }
        }
        let mut notes: Vec<(At, usize, usize, usize)> = Vec::new();
        for (m, mr) in self.mrs.iter().enumerate() {
            for (t, thread) in mr.threads.iter().enumerate() {
                for (n, note) in thread.notes.iter().enumerate() {
                    notes.push((note.at, m, t, n));
                }
            }
        }
        notes.sort();
        for (k, (_, m, t, n)) in notes.into_iter().enumerate() {
            self.mrs[m].threads[t].notes[n].id = 1_874_200_000 + base * 10 + k as u64 * 37;
        }
    }

    // ── What the app reads ──────────────────────────────────────────────────

    pub fn web_url(&self, iid: &str) -> String {
        format!(
            "https://gitlab.orbit-logistics.dev/{}/-/merge_requests/{iid}",
            self.spec.path
        )
    }

    /// The commits the target has that the MR's branch lacks, newest first:
    /// the target's own history since the branch left it, or, for an MR on
    /// another MR's branch, that MR's commits pushed since.
    pub fn behind(&self, i: usize) -> Vec<&LogEntry> {
        let mr = &self.mrs[i];
        let mut list: Vec<&LogEntry> = self
            .logs
            .get(mr.spec.target)
            .map(|log| {
                log.iter()
                    .filter(|e| e.at > mr.base && e.at <= self.now)
                    .collect()
            })
            .unwrap_or_default();
        list.reverse();
        list
    }

    /// For an MR on another MR's branch: that MR's commits pushed since this
    /// one's branch left it.
    pub fn behind_parent(&self, i: usize) -> Vec<&CommitRec> {
        let mr = &self.mrs[i];
        self.mrs
            .iter()
            .find(|p| p.spec.source == mr.spec.target && p.merged.is_none())
            .map(|p| p.commits.iter().filter(|c| c.pushed > mr.base).collect())
            .unwrap_or_default()
    }

    fn commits_behind(&self, i: usize) -> Option<u32> {
        let mr = &self.mrs[i];
        if mr.state() != GitlabMrState::Opened {
            return None;
        }
        match mr.spec.mergeability {
            MergeabilityStatus::Mergeable => Some(0),
            MergeabilityStatus::Retrying
            | MergeabilityStatus::SyncFailed
            | MergeabilityStatus::Unknown => None,
            _ if self.tracked(mr.spec.target).is_some() => Some(self.behind(i).len() as u32),
            _ => Some(self.behind_parent(i).len() as u32),
        }
    }

    pub fn changed_files(&self, i: usize) -> Vec<ChangedFile> {
        self.mrs[i]
            .spec
            .files
            .iter()
            .map(|f| {
                let (added, deleted) = crate::gitlab::count_diff_lines(&f.diff);
                ChangedFile {
                    path: f.path.clone(),
                    old_path: f.old_path.clone(),
                    added,
                    deleted,
                    hunks: crate::gitlab::hunk_ranges(&f.diff),
                    new_file: f.new_file,
                    deleted_file: f.deleted_file,
                }
            })
            .collect()
    }

    pub fn pipelines_of(&self, i: usize) -> Vec<&PipeRec> {
        let mut list: Vec<&PipeRec> = self
            .pipelines
            .iter()
            .filter(|p| p.owner == Owner::Mr(i))
            .collect();
        list.sort_by_key(|p| std::cmp::Reverse(p.created));
        list
    }

    pub fn pipeline_model(p: &PipeRec) -> Pipeline {
        Pipeline {
            id: p.id,
            status: p.status,
            created_at: Some(iso(p.created)),
            updated_at: Some(iso(p.finished.unwrap_or_else(|| clock().now))),
            sha: Some(p.sha.clone()),
            jobs: p
                .jobs
                .iter()
                .map(|j| PipelineJob {
                    id: j.id,
                    name: j.name.into(),
                    stage: j.stage.into(),
                    status: j.status.into(),
                    duration: j.duration,
                    queued_duration: j.queued,
                })
                .collect(),
        }
    }

    fn review(&self, i: usize) -> ReviewActivity {
        let mr = &self.mrs[i];
        let author = mr.spec.author;
        let mut review = ReviewActivity::default();
        let mut first_review: Option<At> = None;
        let mut activity: BTreeMap<String, At> = BTreeMap::new();
        let mut pushes: Vec<At> = Vec::new();
        for (at, ev) in &mr.events {
            let mut act = |who: &'static Person, at: At| {
                activity.insert(who.login.into(), at);
                if who != author && first_review.is_none() {
                    first_review = Some(at);
                }
            };
            match *ev {
                Ev::Push(..) | Ev::Rebase(_) => pushes.push(*at),
                Ev::Ask(person) => {
                    review
                        .review_requested_at
                        .insert(person.login.into(), iso(*at));
                    review.first_review_requested_at.get_or_insert(iso(*at));
                }
                Ev::Note(by, ..) | Ev::Reply(by, ..) | Ev::Resolve(by, _) | Ev::Changes(by) => {
                    act(by, *at)
                }
                Ev::Approve(by) => act(by, *at),
                _ => {}
            }
        }
        review.first_review_at = first_review.map(iso);
        review.approved_at = mr.approved.iter().map(|(_, at)| *at).max().map(iso);
        review.last_push_at = pushes.last().copied().map(iso);
        review.pushes_after_review = first_review.map_or(0, |first| {
            pushes.iter().filter(|p| **p > first).count() as u32
        });
        review.last_activity_by = activity.into_iter().map(|(k, v)| (k, iso(v))).collect();
        review
    }

    fn threads(&self, i: usize) -> Vec<MrThread> {
        self.mrs[i]
            .threads
            .iter()
            .map(|t| {
                let first = &t.notes[0];
                let line = first.body.lines().next().unwrap_or_default();
                MrThread {
                    note_id: first.id,
                    author: first.by.login.into(),
                    excerpt: line.chars().take(140).collect(),
                    created_at: iso(first.at),
                    last_note_at: iso(t.notes.last().map_or(first.at, |n| n.at)),
                    notes: t.notes.len() as u32,
                    resolved: t.resolved.is_some(),
                }
            })
            .collect()
    }

    pub fn milestone(&self, title: &str) -> Option<&MilestoneSpec> {
        self.spec.milestones.iter().find(|m| m.title == title)
    }

    pub fn branches_of(&self, i: usize) -> HashSet<String> {
        self.present
            .iter()
            .filter(|(_, mrs)| mrs.contains_key(&i))
            .map(|(b, _)| b.to_string())
            .collect()
    }

    /// The MR as the app holds it after a refresh.
    pub fn tracked_mr(&self, i: usize, tickets: &super::tracker::Tickets) -> TrackedMr {
        let mr = &self.mrs[i];
        let spec = &mr.spec;
        if spec.fetch == Fetch::Loading {
            return TrackedMr::placeholder(mr.iid.clone(), "Loading...".into(), None);
        }
        let state = mr.state();
        let files = self.changed_files(i);
        let threads = self.threads(i);
        let unresolved = threads.iter().filter(|t| !t.resolved).count() as u32;
        let milestone = spec.milestone.and_then(|t| self.milestone(t));
        let landed = mr.landed();
        let status = match &spec.fetch {
            Fetch::Failed(error) => MrStatus::Error((*error).into()),
            _ if state == GitlabMrState::Merged => MrStatus::MergedIn(self.branches_of(i)),
            _ => MrStatus::MergedIn(HashSet::new()),
        };
        TrackedMr {
            id: mr.iid.clone(),
            status,
            mergeability: if state == GitlabMrState::Opened {
                spec.mergeability
            } else {
                MergeabilityStatus::NotOpen
            },
            recently_updated: state == GitlabMrState::Opened
                && self.now - mr.updated < Duration::minutes(30),
            flagged: spec.flagged,
            linked_ticket: spec.ticket.and_then(|id| tickets.linked(id)),
            data: crate::models::MrData {
                title: spec.title.into(),
                sha: mr.sha(),
                description: spec.description.clone(),
                author: spec.author.display(),
                assignee: spec.assignee.map(Person::display),
                reviewers: mr.reviewers.iter().map(|p| p.display()).collect(),
                milestone: spec.milestone.map(str::to_string),
                milestone_due_date: milestone
                    .and_then(|m| m.due)
                    .map(|due| super::clock::date(clock().at(due))),
                milestone_description: milestone.map(|m| m.description.to_string()),
                web_url: self.web_url(&mr.iid),
                labels: spec.labels.iter().map(|l| l.to_string()).collect(),
                updated_at: Some(iso(mr.updated)),
                source_branch: spec.source.into(),
                target_branch: spec.target.into(),
                state,
                merged_by: mr.merged.map(|(_, by)| by.display()),
                merged_at: mr.merged.map(|(at, _)| iso(at)),
                created_at: Some(iso(mr.opened)),
                pipelines: self
                    .pipelines_of(i)
                    .into_iter()
                    .map(Self::pipeline_model)
                    .collect(),
                user_notes_count: unresolved,
                diff_stats: Some(DiffStats {
                    files_changed: files.len() as u32,
                    additions: files.iter().map(|f| f.added).sum(),
                    deletions: files.iter().map(|f| f.deleted).sum(),
                    commits_count: mr.commits.len() as u32,
                    commits_behind: self.commits_behind(i),
                    files,
                    files_listed: true,
                }),
                commits: mr
                    .commits
                    .iter()
                    .map(|c| MrCommit {
                        sha: c.sha.clone(),
                        fingerprint: mr.fingerprint(c),
                    })
                    .collect(),
                landed_fingerprint: landed,
                threads,
                review: Some(self.review(i)),
                approved_by: mr.approved.iter().map(|(p, _)| p.login.into()).collect(),
                approvers: mr.approved.iter().map(|(p, _)| p.display()).collect(),
                review_read_at: None,
                notes_seen: None,
                // Set by its author when the MR was opened.
                milestone_events: Some(
                    spec.milestone
                        .map(|title| crate::models::MilestoneEvent {
                            at: iso(mr.opened),
                            milestone: Some(title.into()),
                            added: true,
                            by: Some(spec.author.display()),
                        })
                        .into_iter()
                        .collect(),
                ),
            },
        }
    }

    /// Where the MRs reached each branch beyond their target, and when.
    pub fn branch_arrivals(&self) -> HashMap<(String, String), At> {
        self.arrivals
            .iter()
            .map(|a| ((self.mrs[a.mr].iid.clone(), a.branch.to_string()), a.at))
            .collect()
    }

    pub fn branch_commits(&self) -> HashMap<String, BranchCommits> {
        self.copies
            .iter()
            .map(|(b, c)| (b.to_string(), c.clone()))
            .collect()
    }

    /// What an MR looked like at `at`: its state, the branches it was on,
    /// its last pipeline. `None` when it did not exist yet.
    pub fn as_of(
        &self,
        i: usize,
        at: At,
    ) -> Option<(GitlabMrState, HashSet<String>, Option<PipelineState>)> {
        let mr = &self.mrs[i];
        if mr.opened > at || mr.spec.fetch == Fetch::Loading {
            return None;
        }
        let state = match (mr.merged, mr.closed) {
            (Some((m, _)), _) if m <= at => GitlabMrState::Merged,
            (_, Some((c, _))) if c <= at => GitlabMrState::Closed,
            _ => GitlabMrState::Opened,
        };
        let branches = self
            .present
            .iter()
            .filter(|(_, mrs)| mrs.get(&i).is_some_and(|since| *since <= at))
            .map(|(b, _)| b.to_string())
            .collect();
        let pipeline = self
            .pipelines_of(i)
            .into_iter()
            .find(|p| p.created <= at)
            .map(|p| match p.finished {
                Some(done) if done <= at => p.status,
                _ => PipelineState::Running,
            });
        Some((state, branches, pipeline))
    }
}
