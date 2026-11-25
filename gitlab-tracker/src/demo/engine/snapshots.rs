//! The MRs as the app holds them after a refresh, the branches' arrivals and commits, and what an MR looked like at a given time.

use super::*;

impl World {
    /// The MR as the app holds it after a refresh.
    pub fn tracked_mr(&self, i: usize, tickets: &super::super::tracker::Tickets) -> TrackedMr {
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
        let mut tracked = TrackedMr {
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
            snooze: None,
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
                    .map(|due| super::super::clock::date(clock().at(due))),
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
                        author_name: c.by.name.to_string(),
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
                signatures: self.signatures(i),
                auto_merge: state == GitlabMrState::Opened
                    && self.spec.auto_merge.contains(&mr.iid.as_str()),
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
        };
        if state == GitlabMrState::Opened {
            if let Some(until) = spec.snooze {
                let until = until.map(|when| super::super::clock::clock().at(when));
                tracked.snooze = Some(crate::snooze::Snooze::new(&tracked, until));
            }
        }
        tracked
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
