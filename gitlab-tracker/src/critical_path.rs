//! What set a pipeline's duration: its critical path.
//!
//! Each job that ran is laid on a timeline: it is ready once the jobs it waits
//! for are done (those it `needs:`, else every job of the stages before it),
//! waits its time for a runner, then runs. The pipeline lasts as long as its
//! last job ends; the chain of jobs that led there, each one started by the
//! last of those it waited for, is the critical path. Making a job off that
//! path faster changes nothing; [`Plan::without`] tells what a change on it
//! would give back.
//!
//! The times come from the jobs' own durations and runner waits, not from
//! their clocks: a retried job counts once, and a manual or skipped one not at
//! all.

use std::collections::HashMap;

use crate::models::{Pipeline, PipelineJob};

/// A job on the timeline, in seconds from the pipeline's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Its index in the pipeline's jobs.
    pub job: usize,
    /// When what it waited for was done.
    pub ready: f64,
    /// When it started running, after its wait for a runner.
    pub start: f64,
    pub end: f64,
    pub critical: bool,
}

impl Slot {
    pub fn queued(&self) -> f64 {
        self.start - self.ready
    }

    pub fn ran(&self) -> f64 {
        self.end - self.start
    }
}

/// A pipeline's timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Stage by stage, in their order; each stage's jobs as listed.
    pub slots: Vec<Slot>,
    /// The stages, first to last.
    pub stages: Vec<String>,
    /// The critical path, first job first (indexes in the pipeline's jobs).
    pub path: Vec<usize>,
    pub total: f64,
    /// Whether the jobs' `needs:` were known: else only the stage order is.
    pub exact: bool,
}

/// What a change gives back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// The job at this index twice as fast.
    Halve(usize),
    /// No wait for a runner anywhere.
    NoQueue,
}

/// Whether a job ran, and so has its place on the timeline.
fn ran(job: &PipelineJob) -> bool {
    job.duration.is_some() && matches!(job.status.as_str(), "success" | "failed")
}

/// The stages, first to last: by their first job's id when GitLab gave ids
/// (the list comes newest first, the ids grow with the stages), else as listed.
pub fn stage_order(jobs: &[PipelineJob]) -> Vec<String> {
    let mut stages: Vec<(String, u64, usize)> = Vec::new();
    for (i, job) in jobs.iter().enumerate() {
        match stages.iter_mut().find(|(s, _, _)| *s == job.stage) {
            Some(entry) => {
                if job.id != 0 {
                    entry.1 = entry.1.min(job.id);
                }
            }
            None => stages.push((
                job.stage.clone(),
                if job.id == 0 { u64::MAX } else { job.id },
                i,
            )),
        }
    }
    if stages.iter().all(|(_, id, _)| *id != u64::MAX) {
        stages.sort_by_key(|(_, id, _)| *id);
    }
    stages.into_iter().map(|(s, _, _)| s).collect()
}

/// The timeline of `pipeline`; `None` while a job runs or waits, or when no job ran.
pub fn plan(pipeline: &Pipeline) -> Option<Plan> {
    plan_with(pipeline, None)
}

fn plan_with(pipeline: &Pipeline, change: Option<Change>) -> Option<Plan> {
    let jobs = &pipeline.jobs;
    if jobs
        .iter()
        .any(|j| matches!(j.status.as_str(), "running" | "pending" | "preparing"))
    {
        return None;
    }
    let stages = stage_order(jobs);
    let stage_of = |job: &PipelineJob| stages.iter().position(|s| *s == job.stage).unwrap_or(0);
    // Stage by stage, each stage's jobs in id order (as listed when unknown).
    let mut order: Vec<usize> = (0..jobs.len()).filter(|&i| ran(&jobs[i])).collect();
    if order.is_empty() {
        return None;
    }
    order.sort_by_key(|&i| {
        let id = if jobs[i].id == 0 {
            i as u64
        } else {
            jobs[i].id
        };
        (stage_of(&jobs[i]), id)
    });
    let exact = pipeline.needs_read || jobs.iter().any(|j| j.needs.is_some());
    let by_name: HashMap<&str, usize> = order.iter().map(|&i| (jobs[i].name.as_str(), i)).collect();
    let mut end: HashMap<usize, f64> = HashMap::new();
    let mut waited_on: HashMap<usize, Option<usize>> = HashMap::new();
    let mut slots = Vec::new();
    for &i in &order {
        let job = &jobs[i];
        let before: Vec<usize> = match &job.needs {
            Some(needs) => needs
                .iter()
                .filter_map(|n| by_name.get(n.as_str()).copied())
                .collect(),
            None => order
                .iter()
                .copied()
                .filter(|&k| stage_of(&jobs[k]) < stage_of(job))
                .collect(),
        };
        // The last one done is what it waited for.
        let last = before
            .iter()
            .copied()
            .filter_map(|k| end.get(&k).map(|e| (k, *e)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let ready = last.map_or(0.0, |(_, e)| e);
        let mut queue = job.queued_duration.unwrap_or(0.0);
        let mut run = job.duration.unwrap_or(0.0);
        match change {
            Some(Change::NoQueue) => queue = 0.0,
            Some(Change::Halve(k)) if k == i => run /= 2.0,
            _ => {}
        }
        let start = ready + queue;
        end.insert(i, start + run);
        waited_on.insert(i, last.map(|(k, _)| k));
        slots.push(Slot {
            job: i,
            ready,
            start,
            end: start + run,
            critical: false,
        });
    }
    // Back from the job that ended last.
    let last = slots
        .iter()
        .max_by(|a, b| a.end.total_cmp(&b.end))
        .map(|s| s.job)?;
    let total = end[&last];
    let mut path = vec![last];
    while let Some(Some(k)) = waited_on.get(path.last().expect("not empty")) {
        path.push(*k);
    }
    path.reverse();
    for slot in &mut slots {
        slot.critical = path.contains(&slot.job);
    }
    Some(Plan {
        slots,
        stages,
        path,
        total,
        exact,
    })
}

impl Plan {
    /// The pipeline's duration with `change`, and the seconds it gives back.
    pub fn without(&self, pipeline: &Pipeline, change: Change) -> f64 {
        plan_with(pipeline, Some(change)).map_or(0.0, |p| (self.total - p.total).max(0.0))
    }

    /// Seconds waiting for a runner, and running, along the path.
    pub fn path_times(&self) -> (f64, f64) {
        self.slots
            .iter()
            .filter(|s| s.critical)
            .fold((0.0, 0.0), |(q, r), s| (q + s.queued(), r + s.ran()))
    }

    /// The longest job off the path: making it faster gives nothing back.
    pub fn longest_off_path(&self) -> Option<&Slot> {
        self.slots
            .iter()
            .filter(|s| !s.critical)
            .max_by(|a, b| a.ran().total_cmp(&b.ran()))
    }
}

/// How long each job usually runs: the median of its successful runs in
/// `pipelines`, by name.
pub fn usual<'a>(pipelines: impl Iterator<Item = &'a Pipeline>) -> HashMap<String, f64> {
    let mut runs: HashMap<String, Vec<f64>> = HashMap::new();
    for pipeline in pipelines {
        for job in &pipeline.jobs {
            if let (Some(d), "success") = (job.duration, job.status.as_str()) {
                runs.entry(job.name.clone()).or_default().push(d);
            }
        }
    }
    runs.into_iter()
        .filter(|(_, v)| v.len() >= 3)
        .map(|(name, mut v)| {
            v.sort_by(f64::total_cmp);
            let mid = v.len() / 2;
            let median = if v.len() % 2 == 0 {
                (v[mid - 1] + v[mid]) / 2.0
            } else {
                v[mid]
            };
            (name, median)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: u64, name: &str, stage: &str, queued: f64, run: f64) -> PipelineJob {
        PipelineJob {
            id,
            name: name.into(),
            stage: stage.into(),
            status: "success".into(),
            duration: Some(run),
            queued_duration: Some(queued),
            needs: None,
        }
    }

    fn pipeline(jobs: Vec<PipelineJob>) -> Pipeline {
        Pipeline {
            jobs,
            ..Pipeline::default()
        }
    }

    /// The mockup's pipeline, listed newest first as GitLab does.
    fn mockup() -> Pipeline {
        let mut jobs = vec![
            job(1, "build:rust", "build", 40.0, 550.0),
            job(2, "build:docker", "build", 35.0, 325.0),
            job(3, "lint:clippy", "build", 30.0, 220.0),
            job(4, "test:integration", "test", 150.0, 650.0),
            job(5, "test:e2e", "test", 20.0, 530.0),
            job(6, "test:unit", "test", 10.0, 260.0),
            job(7, "audit:deps", "test", 10.0, 60.0),
            job(8, "deploy:review", "deploy", 10.0, 140.0),
        ];
        jobs.reverse();
        pipeline(jobs)
    }

    fn names(p: &Pipeline, path: &[usize]) -> Vec<String> {
        path.iter().map(|&i| p.jobs[i].name.clone()).collect()
    }

    #[test]
    fn stages_come_in_the_order_they_run() {
        let p = mockup();
        assert_eq!(stage_order(&p.jobs), ["build", "test", "deploy"]);
        // Without ids (the demo, old state files): as listed.
        let jobs = vec![job(0, "a", "lint", 0.0, 1.0), job(0, "b", "test", 0.0, 1.0)];
        assert_eq!(stage_order(&jobs), ["lint", "test"]);
    }

    #[test]
    fn the_path_follows_the_last_job_each_one_waited_for() {
        let p = mockup();
        let plan = plan(&p).unwrap();
        assert_eq!(
            names(&p, &plan.path),
            ["build:rust", "test:integration", "deploy:review"]
        );
        // 40 + 550, then 150 + 650, then 10 + 140.
        assert_eq!(plan.total, 1540.0);
        assert_eq!(plan.path_times(), (200.0, 1340.0));
        assert!(!plan.exact);
    }

    #[test]
    fn a_change_gives_back_only_what_is_on_the_path() {
        let p = mockup();
        let plan = plan(&p).unwrap();
        let at = |name: &str| p.jobs.iter().position(|j| j.name == name).unwrap();
        // 325 s faster, but test:e2e then ends last in its stage: 250 s back.
        assert_eq!(
            plan.without(&p, Change::Halve(at("test:integration"))),
            250.0
        );
        assert_eq!(plan.without(&p, Change::Halve(at("test:e2e"))), 0.0);
        assert_eq!(plan.without(&p, Change::NoQueue), 200.0);
        assert_eq!(
            p.jobs[plan.longest_off_path().unwrap().job].name,
            "test:e2e"
        );
    }

    #[test]
    fn needs_let_a_job_start_before_its_stage() {
        let mut p = mockup();
        for j in &mut p.jobs {
            j.needs = Some(match j.name.as_str() {
                "test:integration" => vec!["build:docker".into()],
                "deploy:review" => vec!["test:integration".into()],
                _ => vec![],
            });
        }
        let plan = plan(&p).unwrap();
        assert!(plan.exact);
        // test:integration waits for build:docker (360 s) only.
        assert_eq!(
            names(&p, &plan.path),
            ["build:docker", "test:integration", "deploy:review"]
        );
        assert_eq!(plan.total, 360.0 + 800.0 + 150.0);
    }

    #[test]
    fn a_running_pipeline_has_no_plan_yet() {
        let mut p = mockup();
        p.jobs[0].status = "running".into();
        assert!(plan(&p).is_none());
        // Skipped and manual jobs take no place.
        let mut p = mockup();
        p.jobs[0].status = "manual".into();
        p.jobs[0].duration = None;
        assert_eq!(plan(&p).unwrap().slots.len(), 7);
    }

    #[test]
    fn a_job_usually_runs_its_median() {
        let runs = |d: f64| pipeline(vec![job(1, "t", "test", 0.0, d)]);
        let list = [runs(100.0), runs(300.0), runs(200.0), runs(900.0)];
        assert_eq!(usual(list.iter())["t"], 250.0);
        // Too few runs to say.
        assert!(usual(list[..2].iter()).is_empty());
    }
}
