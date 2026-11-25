//! Playing the scenario back: each MR's log, then the merges, promotions, copies and direct pushes in time order.

use super::*;

impl World {
    /// Plays an MR's log.
    pub(super) fn play(&mut self, spec: MrSpec, index: usize) -> BuiltMr {
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
                Ev::Push(titles, ci) | Ev::PushBy(_, titles, ci) => {
                    let by = match *ev {
                        Ev::PushBy(by, ..) => by,
                        _ => built.spec.author,
                    };
                    let n = titles.len();
                    for (i, title) in titles.iter().enumerate() {
                        let k = built.commits.len();
                        built.commits.push(CommitRec {
                            sha: sha(&[project, built.spec.key, &k.to_string(), "0"]),
                            title,
                            authored: *at - Duration::minutes(((n - 1 - i) * 23) as i64),
                            pushed: *at,
                            by,
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
                Ev::Changes(_) | Ev::Ready => {}
                Ev::Merge(by) => built.merged = Some((*at, by)),
                Ev::Close(by) => built.closed = Some((*at, by)),
            }
        }
        built.events = events;
        built
    }

    /// A pipeline of `owner` on `sha`, started a minute after `at`; a flaky
    /// run is followed by a passing one on the same commit.
    pub(super) fn pipeline(
        &mut self,
        owner: Owner,
        sha: String,
        at: At,
        ci: Ci,
        n: usize,
    ) -> Option<At> {
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

    pub(super) fn branch_jobs(&self, branch: &str) -> Vec<Job> {
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

    pub(super) fn tracked(&self, branch: &str) -> Option<&'static str> {
        self.spec.branches.iter().copied().find(|b| *b == branch)
    }

    pub fn mr(&self, key: &str) -> Option<usize> {
        self.mrs.iter().position(|m| m.spec.key == key)
    }

    /// Plays the merges, promotions, copies and direct pushes in time order.
    pub(super) fn branches_play(&mut self) {
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
}
