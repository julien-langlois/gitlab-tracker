//! What the window reads from GitLab on demand, answered from the demo's
//! history: an MR page's checks, rules, threads, deployments and the commits
//! it lacks; its patches; a job's log; the project's deployments and branch
//! pipelines.

use super::clock::{clock, iso, sha};
use super::engine::{DeployRec, JobRec, Owner, PipeRec, World};
use crate::gitlab_detail::{
    BehindCommit, BranchPipeline, ConflictFile, Deployment, DeploymentRecord, MergeHistory, Note,
    Overview, PageData, ProjectOverview, ReleaseRecord, Rule, Thread, User,
};
use crate::models::{GitlabMrState, MergeabilityStatus};

fn user(p: &super::model::Person) -> User {
    User {
        name: p.name.into(),
        username: p.login.into(),
    }
}

/// GitLab's `detailed_merge_status` for an MR the app shows as `m`.
fn detailed_status(m: MergeabilityStatus) -> &'static str {
    match m {
        MergeabilityStatus::Mergeable => "mergeable",
        MergeabilityStatus::Conflict => "conflict",
        MergeabilityStatus::NeedsRebase => "need_rebase",
        MergeabilityStatus::NotOpen => "not_open",
        MergeabilityStatus::Draft => "draft_status",
        MergeabilityStatus::DiscussionsNotResolved => "discussions_not_resolved",
        MergeabilityStatus::CiMustPass => "ci_must_pass",
        MergeabilityStatus::CiStillRunning => "ci_still_running",
        MergeabilityStatus::NotApproved => "not_approved",
        MergeabilityStatus::RequestedChanges => "requested_changes",
        MergeabilityStatus::Retrying | MergeabilityStatus::SyncFailed => "checking",
        MergeabilityStatus::Unknown => "external_status_checks",
    }
}

/// Whether two line ranges touch (git stops on adjacent lines too).
fn touch(a: (u32, u32), b: (u32, u32)) -> bool {
    a.0 <= b.1 + 3 && b.0 <= a.1 + 3
}

impl World {
    pub fn commit_url(&self, sha: &str) -> String {
        format!(
            "https://gitlab.orbit-logistics.dev/{}/-/commit/{sha}",
            self.spec.path
        )
    }

    /// What an MR's page reads when it opens.
    pub fn page_data(&self, i: usize) -> PageData {
        let mr = &self.mrs[i];
        let spec = &mr.spec;
        let open = mr.state() == GitlabMrState::Opened;
        let unresolved = mr.threads.iter().filter(|t| t.resolved.is_none()).count();
        let stacked = !self.spec.branches.contains(&spec.target);
        let behind: Vec<BehindCommit> = if !open {
            Vec::new()
        } else if stacked {
            let parent = self
                .mrs
                .iter()
                .find(|p| p.spec.source == spec.target && p.merged.is_none());
            self.behind_parent(i)
                .into_iter()
                .rev()
                .map(|c| BehindCommit {
                    sha: c.sha.clone(),
                    title: c.title.into(),
                    author: parent.map_or("", |p| p.spec.author.name).into(),
                    created_at: iso(c.pushed),
                    web_url: self.commit_url(&c.sha),
                    mr_iid: None,
                })
                .collect()
        } else {
            self.behind(i)
                .into_iter()
                .map(|e| BehindCommit {
                    sha: e.sha.clone(),
                    title: e.title.clone(),
                    author: e.author.into(),
                    created_at: iso(e.at),
                    web_url: self.commit_url(&e.sha),
                    mr_iid: e.mr.map(|m| self.mrs[m].iid.clone()),
                })
                .collect()
        };

        // The files both sides changed since they parted.
        let conflicts = (open && spec.mergeability == MergeabilityStatus::Conflict).then(|| {
            let mine = self.changed_files(i);
            let mut list: Vec<ConflictFile> = Vec::new();
            for entry in self.behind(i) {
                let Some(other) = entry.mr else { continue };
                for theirs in self.changed_files(other) {
                    let Some(ours) = mine.iter().find(|f| f.path == theirs.path) else {
                        continue;
                    };
                    let clash = ours
                        .hunks
                        .iter()
                        .any(|a| theirs.hunks.iter().any(|b| touch(*a, *b)));
                    match list.iter_mut().find(|c| c.path == ours.path) {
                        Some(known) => known.clash |= clash,
                        None => list.push(ConflictFile {
                            path: ours.path.clone(),
                            clash,
                        }),
                    }
                }
            }
            list.sort_by_key(|c| !c.clash);
            list
        });

        let rules = self
            .spec
            .rules
            .iter()
            .filter(|rule| {
                rule.paths.is_empty()
                    || spec
                        .files
                        .iter()
                        .any(|f| rule.paths.iter().any(|p| f.path.starts_with(p)))
            })
            .map(|rule| {
                let approved_by: Vec<User> = mr
                    .approved
                    .iter()
                    .filter(|(p, _)| rule.eligible.contains(p))
                    .map(|(p, _)| user(p))
                    .collect();
                Rule {
                    name: rule.name.into(),
                    approvals_required: rule.required,
                    approved: approved_by.len() as u32 >= rule.required,
                    approved_by,
                    eligible_approvers: rule
                        .eligible
                        .iter()
                        .filter(|p| **p != spec.author)
                        .map(|p| user(p))
                        .collect(),
                }
            })
            .collect();

        let threads = mr
            .threads
            .iter()
            .map(|t| Thread {
                resolvable: true,
                resolved: t.resolved.is_some(),
                path: t.place.map(|(path, _)| path.to_string()),
                line: t.place.map(|(_, line)| line),
                notes: t
                    .notes
                    .iter()
                    .map(|n| Note {
                        author: n.by.name.into(),
                        username: n.by.login.into(),
                        body: n.body.into(),
                        created_at: iso(n.at),
                    })
                    .collect(),
            })
            .collect();

        let deployments = self
            .spec
            .environments
            .iter()
            .filter_map(|(branch, environment)| {
                let last = self
                    .deployments
                    .iter()
                    .find(|d| d.branch == *branch && d.ok && d.at <= self.now)?;
                let by_sha = self
                    .present
                    .get(branch)
                    .and_then(|m| m.get(&i))
                    .is_some_and(|since| *since <= last.at);
                let copied = self
                    .arrivals
                    .iter()
                    .any(|a| a.mr == i && a.branch == *branch && a.at <= last.at);
                Some(Deployment {
                    environment: (*environment).into(),
                    reference: (*branch).into(),
                    sha: last.sha.clone(),
                    created_at: iso(last.at),
                    includes: if by_sha {
                        Some(true)
                    } else if copied {
                        None
                    } else {
                        Some(false)
                    },
                })
            })
            .collect();

        PageData {
            overview: Some(Overview {
                detailed_merge_status: if open {
                    detailed_status(spec.mergeability).into()
                } else {
                    "not_open".into()
                },
                has_conflicts: open && spec.mergeability == MergeabilityStatus::Conflict,
                blocking_discussions_resolved: unresolved == 0,
                draft: spec.mergeability == MergeabilityStatus::Draft,
                diverged_commits_count: open.then_some(behind.len() as u32),
                force_remove_source_branch: Some(true),
            }),
            conflicts,
            rules: Some(rules),
            threads: Ok(threads),
            deployments,
            behind: (!behind.is_empty()).then_some(behind),
        }
    }

    /// The MR's files with their patches.
    pub fn file_diffs(&self, i: usize) -> Vec<crate::gitlab::FileDiff> {
        self.mrs[i]
            .spec
            .files
            .iter()
            .map(|f| crate::gitlab::FileDiff {
                old_path: f.old_path.clone().unwrap_or_else(|| f.path.clone()),
                new_path: f.path.clone(),
                new_file: f.new_file,
                renamed_file: f.old_path.is_some(),
                deleted_file: f.deleted_file,
                diff: f.diff.clone(),
            })
            .collect()
    }

    fn deployment_record(d: &DeployRec) -> DeploymentRecord {
        DeploymentRecord {
            environment: d.environment.into(),
            reference: d.branch.into(),
            sha: d.sha.clone(),
            created_at: iso(d.at),
            status: if d.ok { "success" } else { "failed" }.into(),
            user: d.user.into(),
        }
    }

    /// The project's deployments, the latest pipeline of each tracked branch
    /// and its merges.
    pub fn overview(&self) -> ProjectOverview {
        let pipelines = self
            .spec
            .branches
            .iter()
            .filter_map(|branch| {
                let p = self
                    .pipelines
                    .iter()
                    .filter(|p| p.owner == Owner::Branch(branch))
                    .max_by_key(|p| p.created)?;
                Some(BranchPipeline {
                    branch: (*branch).into(),
                    id: p.id,
                    sha: p.sha.clone(),
                    status: p.status,
                    created_at: iso(p.created),
                    updated_at: iso(p.finished.unwrap_or(self.now)),
                    web_url: format!(
                        "https://gitlab.orbit-logistics.dev/{}/-/pipelines/{}",
                        self.spec.path, p.id
                    ),
                    failed_jobs: p
                        .jobs
                        .iter()
                        .filter(|j| j.status == "failed")
                        .map(|j| j.name.to_string())
                        .collect(),
                })
            })
            .collect();
        ProjectOverview {
            deployments: self
                .deployments
                .iter()
                .map(Self::deployment_record)
                .collect(),
            pipelines,
            releases: self.releases(),
            releases_denied: false,
            freezes: self.freezes(),
            freezes_denied: false,
            schedules: self.schedules(),
            schedules_denied: false,
            merges: MergeHistory {
                merged: self
                    .mrs
                    .iter()
                    .filter_map(|mr| Some((mr.iid.parse().ok()?, iso(mr.merged?.0))))
                    .collect(),
                read_at: Some(self.now.to_rfc3339()),
            },
        }
    }

    /// The version tags made so far, newest first: each on the head its
    /// branch had then.
    /// The deploy freezes, as GitLab gives them: a one-off one as the crons
    /// of its days.
    fn freezes(&self) -> Vec<crate::freeze::Declared> {
        use super::model::FreezeSpec;
        self.spec
            .freezes
            .iter()
            .map(|f| match f {
                FreezeSpec::Cron { start, end, zone } => crate::freeze::Declared {
                    start: (*start).into(),
                    end: (*end).into(),
                    zone: (*zone).into(),
                },
                FreezeSpec::Once { from, to, zone } => {
                    let tz: chrono_tz::Tz = zone.parse().unwrap_or(chrono_tz::UTC);
                    let cron = |at: chrono::DateTime<chrono::Utc>| {
                        let t = at.with_timezone(&tz);
                        t.format("%-M %-H %-d %-m *").to_string()
                    };
                    crate::freeze::Declared {
                        start: cron(clock().at(*from)),
                        end: cron(clock().at(*to)),
                        zone: (*zone).into(),
                    }
                }
            })
            .collect()
    }

    /// The pipeline schedules, their runs at the cron's latest moments.
    fn schedules(&self) -> Vec<crate::gitlab_detail::ScheduleRecord> {
        use crate::gitlab_detail::{ScheduleRecord, ScheduleRun};
        use crate::models::PipelineState;
        let now = clock().now;
        self.spec
            .schedules
            .iter()
            .map(|s| {
                let tz: chrono_tz::Tz = s.zone.parse().unwrap_or(chrono_tz::UTC);
                let cron = crate::freeze::Cron::parse(s.cron);
                // The cron's latest moments, newest first.
                let mut moments = Vec::new();
                let mut at = now;
                while let Some(t) = cron.as_ref().and_then(|c| c.previous(&tz, at)) {
                    moments.push(t);
                    if moments.len() >= s.runs.len() + s.idle {
                        break;
                    }
                    at = t - chrono::Duration::minutes(1);
                }
                let mut runs: Vec<ScheduleRun> = s
                    .runs
                    .chars()
                    .rev()
                    .zip(moments.iter().skip(s.idle))
                    .enumerate()
                    .map(|(i, (c, t))| {
                        let id = 1_259_000 + s.id * 40 - i as u64;
                        ScheduleRun {
                            id,
                            status: if c == 'x' {
                                PipelineState::Failed
                            } else {
                                PipelineState::Success
                            },
                            created_at: iso(*t),
                            web_url: format!(
                                "https://gitlab.orbit-logistics.dev/{}/-/pipelines/{id}",
                                self.spec.path
                            ),
                        }
                    })
                    .collect();
                runs.reverse();
                ScheduleRecord {
                    id: s.id,
                    description: s.description.into(),
                    ref_name: s.ref_name.into(),
                    cron: s.cron.into(),
                    zone: s.zone.into(),
                    next_run_at: cron
                        .as_ref()
                        .filter(|_| s.active)
                        .and_then(|c| c.next(&tz, now))
                        .map(iso),
                    active: s.active,
                    owner: s.owner.into(),
                    owner_state: s.owner_state.into(),
                    runs,
                }
            })
            .collect()
    }

    fn releases(&self) -> Vec<ReleaseRecord> {
        self.spec
            .releases
            .iter()
            .rev()
            .filter_map(|r| {
                let at = clock().at(r.at);
                if at > self.now {
                    return None;
                }
                let head = self
                    .logs
                    .get(r.branch)
                    .and_then(|log| log.iter().rev().find(|e| e.at <= at))
                    .map_or_else(|| sha(&[self.spec.id, r.tag]), |e| e.sha.clone());
                Some(ReleaseRecord {
                    tag: r.tag.into(),
                    name: None,
                    sha: head,
                    committed_at: iso(at),
                    released_at: r.release.then(|| iso(at)),
                    notes: r.notes.map(str::to_string),
                    web_url: if r.release {
                        format!(
                            "https://gitlab.orbit-logistics.dev/{}/-/releases/{}",
                            self.spec.path, r.tag
                        )
                    } else {
                        String::new()
                    },
                    is_release: r.release,
                })
            })
            .collect()
    }

    /// When each MR reached `branch` (by index), up to `until`: merged
    /// there, promoted or copied.
    fn on_branch(
        &self,
        branch: &str,
        until: chrono::DateTime<chrono::Utc>,
    ) -> Vec<(usize, chrono::DateTime<chrono::Utc>)> {
        let mut list: Vec<(usize, chrono::DateTime<chrono::Utc>)> = self
            .present
            .get(branch)
            .map(|m| m.iter().map(|(i, at)| (*i, *at)).collect())
            .unwrap_or_default();
        for a in self.arrivals.iter().filter(|a| a.branch == branch) {
            if !list.iter().any(|(i, _)| *i == a.mr) {
                list.push((a.mr, a.at));
            }
        }
        list.retain(|(_, at)| *at <= until);
        list
    }

    /// Where a ref of a range stands: a version tag (its branch then) or a
    /// tracked branch (now).
    fn ref_point(&self, reference: &str) -> Option<(&'static str, chrono::DateTime<chrono::Utc>)> {
        if let Some(r) = self.spec.releases.iter().find(|r| r.tag == reference) {
            return Some((r.branch, clock().at(r.at).min(self.now)));
        }
        let branch = self.spec.branches.iter().find(|b| **b == reference)?;
        Some((*branch, self.now))
    }

    /// The commits of `range` (`from..to`), newest first, as GitLab lists
    /// them: a merge commit per MR on `to` and not on `from`, and the
    /// commits pushed straight to `to`'s branch after `from`.
    pub fn range_commits(&self, range: &str) -> Vec<crate::changelog::ApiCommit> {
        let Some((from, to)) = range.split_once("..") else {
            return Vec::new();
        };
        let (Some((from_branch, from_at)), Some((to_branch, to_at))) =
            (self.ref_point(from), self.ref_point(to))
        else {
            return Vec::new();
        };
        let before: std::collections::HashSet<usize> = self
            .on_branch(from_branch, from_at)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        let mut list: Vec<(chrono::DateTime<chrono::Utc>, crate::changelog::ApiCommit)> = self
            .on_branch(to_branch, to_at)
            .into_iter()
            .filter(|(i, _)| !before.contains(i))
            .map(|(i, at)| {
                let mr = &self.mrs[i];
                let message = format!(
                    "Merge branch '{}' into '{}'\n\n{}\n\nSee merge request {}!{}",
                    mr.spec.source, mr.spec.target, mr.spec.title, self.spec.path, mr.iid
                );
                (
                    at,
                    crate::changelog::ApiCommit {
                        id: mr.merge_sha.clone(),
                        message,
                        committed_date: Some(iso(at)),
                        parent_ids: vec![String::new(), String::new()],
                        ..Default::default()
                    },
                )
            })
            .collect();
        for direct in self.spec.directs.iter().filter(|c| c.branch == to_branch) {
            let at = clock().at(direct.at);
            if at > from_at && at <= to_at {
                list.push((
                    at,
                    crate::changelog::ApiCommit {
                        id: sha(&[self.spec.id, direct.title]),
                        message: direct.title.to_string(),
                        committed_date: Some(iso(at)),
                        ..Default::default()
                    },
                ));
            }
        }
        list.sort_by_key(|c| std::cmp::Reverse(c.0));
        list.into_iter().map(|(_, c)| c).collect()
    }

    pub fn job(&self, id: u64) -> Option<(&PipeRec, &JobRec)> {
        self.pipelines
            .iter()
            .find_map(|p| p.jobs.iter().find(|j| j.id == id).map(|j| (p, j)))
    }

    /// The end of a job's log.
    pub fn job_log(&self, id: u64) -> Option<String> {
        let (pipeline, job) = self.job(id)?;
        Some(crate::gitlab::log_tail(
            &super::logs::log(self, pipeline, job),
            60,
        ))
    }
}
