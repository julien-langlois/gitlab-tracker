//! What the window reads from GitLab on demand, answered from the demo's
//! history: an MR page's checks, rules, threads, deployments and the commits
//! it lacks; its patches; a job's log; the project's deployments and branch
//! pipelines.

use super::clock::iso;
use super::engine::{DeployRec, JobRec, Owner, PipeRec, World};
use crate::gitlab_detail::{
    BehindCommit, BranchPipeline, ConflictFile, Deployment, DeploymentRecord, MergeHistory, Note,
    Overview, PageData, ProjectOverview, Rule, Thread, User,
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
