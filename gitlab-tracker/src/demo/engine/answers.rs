//! What the demo's GitLab answers from that history: ids, URLs, commits behind, files, pipelines, reviews, threads, signatures.

use super::*;

impl World {
    /// GitLab's ids, growing with time: pipelines, jobs, notes.
    pub(super) fn number(&mut self) {
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

    pub(super) fn commits_behind(&self, i: usize) -> Option<u32> {
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
                    needs: None,
                })
                .collect(),
            // The demo's jobs all wait for the stages before them.
            needs_read: true,
        }
    }

    pub(super) fn review(&self, i: usize) -> ReviewActivity {
        let mr = &self.mrs[i];
        let author = mr.spec.author;
        let mut review = ReviewActivity {
            version: crate::models::REVIEW_ACTIVITY_VERSION,
            ..ReviewActivity::default()
        };
        let mut first_review: Option<At> = None;
        let mut activity: BTreeMap<String, At> = BTreeMap::new();
        let mut pushes: Vec<At> = Vec::new();
        let mut pushed_by: BTreeMap<String, At> = BTreeMap::new();
        // Opened as a draft: it has a move to ready, or is a draft today. Notes
        // on a draft nobody was asked to review yet are the review along the
        // way, not the review; an approval always is.
        let mut draft = mr.events.iter().any(|(_, ev)| matches!(ev, Ev::Ready))
            || mr.spec.mergeability == MergeabilityStatus::Draft;
        review.was_draft = draft;
        let mut draft_comments = 0;
        let mut ready: Option<At> = None;
        let mut asked = false;
        for (at, ev) in &mr.events {
            let mut act = |who: &'static Person, at: At, approval: bool| {
                activity.insert(who.login.into(), at);
                if who != author {
                    if draft && !asked && !approval {
                        draft_comments += 1;
                    } else if first_review.is_none() {
                        first_review = Some(at);
                    }
                }
            };
            match *ev {
                Ev::Ready => {
                    draft = false;
                    if first_review.is_none() || ready.is_none() {
                        ready = Some(*at);
                    }
                }
                Ev::Push(..) | Ev::Rebase(_) => {
                    pushes.push(*at);
                    pushed_by.insert(author.login.into(), *at);
                }
                Ev::PushBy(by, ..) => {
                    pushes.push(*at);
                    pushed_by.insert(by.login.into(), *at);
                }
                Ev::Ask(person) => {
                    asked = true;
                    review
                        .review_requested_at
                        .insert(person.login.into(), iso(*at));
                    review.first_review_requested_at.get_or_insert(iso(*at));
                }
                Ev::Note(by, ..) | Ev::Reply(by, ..) | Ev::Resolve(by, _) | Ev::Changes(by) => {
                    act(by, *at, false)
                }
                Ev::Approve(by) => act(by, *at, true),
                _ => {}
            }
        }
        review.first_review_at = first_review.map(iso);
        review.ready_at = ready.map(iso);
        review.draft_comments = draft_comments;
        review.approved_at = mr.approved.iter().map(|(_, at)| *at).max().map(iso);
        review.approvals = mr
            .approved
            .iter()
            .map(|(p, at)| (p.login.to_string(), iso(*at)))
            .collect();
        review.last_push_at = pushes.last().copied().map(iso);
        review.pushes_after_review = first_review.map_or(0, |first| {
            pushes.iter().filter(|p| **p > first).count() as u32
        });
        review.last_activity_by = activity.into_iter().map(|(k, v)| (k, iso(v))).collect();
        review.last_push_by = pushed_by.into_iter().map(|(k, v)| (k, iso(v))).collect();
        review
    }

    pub(super) fn threads(&self, i: usize) -> Vec<MrThread> {
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

    /// What the signature check read for an open MR: every commit signed and
    /// verified, except in a few MRs that show what the check reports.
    pub(super) fn signatures(&self, i: usize) -> Option<crate::signatures::SignatureData> {
        use crate::signatures::{Facts, Signature, SignatureData, Status, Version, VersionCommit};
        let mr = &self.mrs[i];
        if mr.state() != GitlabMrState::Opened {
            return None;
        }
        let mut commits: Vec<&CommitRec> = mr.commits.iter().collect();
        commits.sort_by_key(|c| c.authored);
        let entry = |sha: &str, c: &CommitRec| VersionCommit {
            sha: sha.into(),
            fingerprint: mr.fingerprint(c),
        };
        let current =
            |list: &[&CommitRec]| list.iter().map(|c| entry(&c.sha, c)).collect::<Vec<_>>();
        // The signed commit a rebase from GitLab's UI rewrote.
        let original = |c: &CommitRec| c.sha.chars().rev().collect::<String>();
        let (base_before, base_after) = (
            sha(&[mr.spec.key, "base", "1"]),
            sha(&[mr.spec.key, "base", "2"]),
        );
        let verified = Signature::Signed(Status::Verified);
        let mut facts = Facts::default();
        let mut signed = |sha: &str, signature: Signature| {
            facts.signatures.insert(sha.to_string(), signature);
        };
        // Rebased from GitLab's UI after the review: the first `rebased` commits
        // lost their signature, each copy of a signed original.
        let rebased = match mr.spec.key {
            "push-notifications" => 4,
            "courier-ws" => commits.len(),
            _ => 0,
        };
        let versions = if rebased > 0 {
            let before: Vec<VersionCommit> = commits[..rebased]
                .iter()
                .map(|c| entry(&original(c), c))
                .collect();
            let mut versions = vec![
                Version {
                    base_sha: base_before,
                    commits: before,
                },
                Version {
                    base_sha: base_after.clone(),
                    commits: current(&commits[..rebased]),
                },
            ];
            if rebased < commits.len() {
                versions.push(Version {
                    base_sha: base_after,
                    commits: current(&commits),
                });
            }
            for c in &commits[..rebased] {
                signed(&original(c), verified);
                signed(&c.sha, Signature::Unsigned);
                let patch = Some(format!("patch-{}", c.sha));
                facts.patch_ids.insert(original(c), patch.clone());
                facts.patch_ids.insert(c.sha.clone(), patch);
            }
            versions
        } else {
            vec![Version {
                base_sha: String::new(),
                commits: current(&commits),
            }]
        };
        let newest = commits.last().map(|c| c.sha.as_str()).unwrap_or_default();
        for c in &commits[rebased..] {
            let signature = match mr.spec.key {
                // Pushed after the rebase, from a machine without the key.
                "push-notifications" if c.sha == newest => Signature::Unsigned,
                // Renovate does not sign here (no key configured for it).
                "tokio-ecosystem" => Signature::Unsigned,
                // Signed with a key the author never added to GitLab.
                "rate-limit" if c.sha == newest => Signature::Signed(Status::UnknownKey),
                _ => verified,
            };
            signed(&c.sha, signature);
        }
        Some(SignatureData {
            versions,
            versions_read: rebased > 0,
            facts,
        })
    }
}
