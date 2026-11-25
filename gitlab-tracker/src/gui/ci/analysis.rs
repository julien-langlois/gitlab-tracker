//! Reading the pipelines into runs, jobs, flaky jobs, blocked MRs and the CI budget.

use super::*;

/// One run of a job.
#[derive(Debug, Clone)]
pub struct Run {
    pub at: DateTime<Utc>,
    pub status: String,
    pub duration: Option<f64>,
    pub sha: Option<String>,
    pub mr: String,
    /// The MR's title, for its id's hover.
    pub mr_title: String,
    /// A failure followed by a pass of the same job on the same commit.
    pub flaky: bool,
}

/// A job, across the tracked MRs' pipelines.
#[derive(Debug, Clone)]
pub struct Job {
    pub name: String,
    pub stage: String,
    /// Oldest first.
    pub runs: Vec<Run>,
    pub failure_rate: f64,
    /// The failure rate of the first half of the period, to tell a trend.
    pub rate_before: Option<f64>,
    pub median: Option<f64>,
    /// Failures followed by a pass on the same commit.
    pub flaky_runs: usize,
}

impl Job {
    pub(super) fn failures(&self) -> usize {
        self.runs.iter().filter(|r| r.status == "failed").count()
    }

    pub(super) fn is_flaky(&self) -> bool {
        self.flaky_runs > 0
    }
}

/// An open MR whose latest pipeline failed.
#[derive(Debug, Clone)]
pub struct Blocked {
    pub id: String,
    pub title: String,
    pub author: String,
    pub jobs: Vec<String>,
    pub since: Option<DateTime<Utc>>,
    /// Its failed jobs include a flaky one: a rerun may do.
    pub flaky: bool,
}

/// What the page shows.
#[derive(Debug, Clone, Default)]
pub struct Ci {
    pub pipelines: usize,
    pub success: Option<f64>,
    pub success_before: Option<f64>,
    pub median: Option<f64>,
    pub median_before: Option<f64>,
    pub p90: Option<f64>,
    pub jobs: Vec<Job>,
    pub blocked: Vec<Blocked>,
    /// Per day of the period, oldest first: the median duration and the count.
    pub days: Vec<(chrono::NaiveDate, Option<f64>, usize)>,
    /// Failed jobs per stage.
    pub stages: BTreeMap<String, usize>,
    pub budget: Budget,
}

/// What the pipelines cost in runner time over the period.
#[derive(Debug, Clone, Default)]
pub struct Budget {
    /// Seconds of jobs run, in the period and the one before.
    pub runner_secs: f64,
    pub runner_before: f64,
    /// Seconds of the jobs of failed pipelines: to run again.
    pub failed_secs: f64,
    /// Median and P90 of the time a job waited for a runner, when GitLab gave it.
    pub queued: Option<f64>,
    pub queued_p90: Option<f64>,
    /// Seconds per pipeline, mean.
    pub per_pipeline: Option<f64>,
    /// Seconds per job, the most first.
    pub by_job: Vec<(String, f64)>,
    /// Seconds per day of the period, oldest first.
    pub by_day: Vec<(chrono::NaiveDate, f64)>,
}

/// Seconds of runner time as hours: "12.4 h", "35 min".
pub fn hours_text(seconds: f64) -> String {
    if seconds < 3600.0 {
        tf("{n} min", &[("n", &format!("{:.0}", seconds / 60.0))])
    } else {
        tf("{n} h", &[("n", &format!("{:.1}", seconds / 3600.0))])
    }
}

pub(super) fn seconds_between(from: &str, to: &str) -> Option<f64> {
    let (a, b) = (board::parse_time(from)?, board::parse_time(to)?);
    let s = (b - a).num_seconds() as f64;
    (s > 0.0).then_some(s)
}

pub(super) fn rate(failed: usize, passed: usize) -> Option<f64> {
    let total = failed + passed;
    (total > 0).then(|| failed as f64 / total as f64)
}

pub(super) fn percentile(mut values: Vec<f64>, p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * p).round() as usize;
    values.get(index).copied()
}

/// The CI of `app`'s MRs over the last `days`.
pub fn analyze(app: &App, now: DateTime<Utc>, days: i64) -> Ci {
    let start = now - Duration::days(days);
    let before = start - Duration::days(days);
    let middle = start + Duration::days(days) / 2;
    let mut ci = Ci::default();
    let (mut passed, mut failed, mut passed_before, mut failed_before) = (0, 0, 0, 0);
    let mut durations = Vec::new();
    let mut durations_before = Vec::new();
    let mut by_day: BTreeMap<chrono::NaiveDate, Vec<f64>> = BTreeMap::new();
    let mut count_by_day: BTreeMap<chrono::NaiveDate, usize> = BTreeMap::new();
    let mut jobs: BTreeMap<String, (BTreeMap<String, usize>, Vec<Run>)> = BTreeMap::new();
    let mut by_job_secs: BTreeMap<String, f64> = BTreeMap::new();
    let mut secs_by_day: BTreeMap<chrono::NaiveDate, f64> = BTreeMap::new();
    let mut queued = Vec::new();
    for mr in app.mrs.iter().filter(|mr| !mr.title.is_empty()) {
        for p in &mr.pipelines {
            let Some(at) = board::time_of(p.created_at.as_deref()) else {
                continue;
            };
            let duration = p
                .updated_at
                .as_deref()
                .zip(p.created_at.as_deref())
                .and_then(|(end, begin)| seconds_between(begin, end))
                .filter(|_| matches!(p.status, PipelineState::Success | PipelineState::Failed));
            if at >= start {
                ci.pipelines += 1;
                match p.status {
                    PipelineState::Success => passed += 1,
                    PipelineState::Failed => failed += 1,
                    _ => {}
                }
                let day = at.with_timezone(&Local).date_naive();
                *count_by_day.entry(day).or_default() += 1;
                if let Some(d) = duration {
                    durations.push(d);
                    by_day.entry(day).or_default().push(d);
                }
                for job in &p.jobs {
                    if let Some(secs) = job.duration {
                        ci.budget.runner_secs += secs;
                        *by_job_secs.entry(job.name.clone()).or_default() += secs;
                        *secs_by_day.entry(day).or_default() += secs;
                        if p.status == PipelineState::Failed {
                            ci.budget.failed_secs += secs;
                        }
                    }
                    if let Some(wait) = job.queued_duration {
                        queued.push(wait);
                    }
                    let entry = jobs.entry(job.name.clone()).or_default();
                    *entry.0.entry(job.stage.clone()).or_default() += 1;
                    entry.1.push(Run {
                        at,
                        status: job.status.clone(),
                        duration: job.duration,
                        sha: p.sha.clone(),
                        mr: mr.id.clone(),
                        mr_title: mr.title.clone(),
                        flaky: false,
                    });
                    if job.status == "failed" {
                        *ci.stages.entry(job.stage.clone()).or_default() += 1;
                    }
                }
            } else if at >= before {
                match p.status {
                    PipelineState::Success => passed_before += 1,
                    PipelineState::Failed => failed_before += 1,
                    _ => {}
                }
                if let Some(d) = duration {
                    durations_before.push(d);
                }
                ci.budget.runner_before += p.jobs.iter().filter_map(|j| j.duration).sum::<f64>();
            }
        }
        // Held back by the CI: open, its latest pipeline failed.
        if mr.state == GitlabMrState::Opened {
            let latest = mr
                .pipelines
                .iter()
                .max_by_key(|p| p.created_at.clone().unwrap_or_default());
            if let Some(p) = latest.filter(|p| p.status == PipelineState::Failed) {
                ci.blocked.push(Blocked {
                    id: mr.id.clone(),
                    title: mr.title.clone(),
                    author: mr.author.clone(),
                    jobs: p
                        .jobs
                        .iter()
                        .filter(|j| j.status == "failed")
                        .map(|j| j.name.clone())
                        .collect(),
                    since: board::time_of(p.created_at.as_deref()),
                    flaky: false,
                });
            }
        }
    }
    ci.success = rate(passed, failed);
    ci.success_before = rate(passed_before, failed_before);
    ci.median = crate::next_step::median(durations.clone());
    ci.p90 = percentile(durations, 0.9);
    ci.median_before = crate::next_step::median(durations_before);
    ci.budget.queued = crate::next_step::median(queued.clone());
    ci.budget.queued_p90 = percentile(queued, 0.9);
    ci.budget.per_pipeline =
        (ci.pipelines > 0).then(|| ci.budget.runner_secs / ci.pipelines as f64);
    ci.budget.by_job = by_job_secs.into_iter().collect();
    ci.budget
        .by_job
        .sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let today = now.with_timezone(&Local).date_naive();
    ci.budget.by_day = (0..days)
        .rev()
        .filter_map(|d| today.checked_sub_days(chrono::Days::new(d as u64)))
        .map(|day| (day, secs_by_day.get(&day).copied().unwrap_or(0.0)))
        .collect();
    ci.days = (0..days)
        .rev()
        .filter_map(|d| today.checked_sub_days(chrono::Days::new(d as u64)))
        .map(|day| {
            (
                day,
                by_day
                    .get(&day)
                    .and_then(|v| crate::next_step::median(v.clone())),
                count_by_day.get(&day).copied().unwrap_or(0),
            )
        })
        .collect();
    for (name, (stages, mut runs)) in jobs {
        runs.sort_by_key(|r| r.at);
        // Flaky: a failure, then a pass of the same job on the same commit.
        let mut flaky_runs = 0;
        for i in 0..runs.len() {
            if runs[i].status != "failed" {
                continue;
            }
            let Some(sha) = runs[i].sha.clone() else {
                continue;
            };
            let passed_later = runs[i + 1..]
                .iter()
                .any(|r| r.status == "success" && r.sha.as_deref() == Some(sha.as_str()));
            if passed_later {
                runs[i].flaky = true;
                flaky_runs += 1;
            }
        }
        let count = |runs: &mut dyn Iterator<Item = &Run>| {
            let (mut f, mut p) = (0, 0);
            for r in runs {
                match r.status.as_str() {
                    "failed" => f += 1,
                    "success" => p += 1,
                    _ => {}
                }
            }
            (f, p)
        };
        let (f, p) = count(&mut runs.iter());
        let (fb, pb) = count(&mut runs.iter().filter(|r| r.at < middle));
        let (fa, pa) = count(&mut runs.iter().filter(|r| r.at >= middle));
        let stage = stages
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(s, _)| s)
            .unwrap_or_default();
        let median = crate::next_step::median(runs.iter().filter_map(|r| r.duration).collect());
        ci.jobs.push(Job {
            name,
            stage,
            failure_rate: rate(f, p).unwrap_or(0.0),
            rate_before: rate(fb, pb).filter(|_| rate(fa, pa).is_some()),
            median,
            flaky_runs,
            runs,
        });
    }
    // The most failing first.
    ci.jobs.sort_by(|a, b| {
        b.failure_rate
            .total_cmp(&a.failure_rate)
            .then(b.flaky_runs.cmp(&a.flaky_runs))
            .then(a.name.cmp(&b.name))
    });
    let flaky: HashSet<&str> = ci
        .jobs
        .iter()
        .filter(|j| j.is_flaky())
        .map(|j| j.name.as_str())
        .collect();
    for b in &mut ci.blocked {
        b.flaky = b.jobs.iter().any(|j| flaky.contains(j.as_str()));
    }
    ci.blocked.sort_by_key(|b| b.since);
    ci
}
