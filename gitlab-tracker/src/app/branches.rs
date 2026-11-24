//! Branch presence: branch scans, flow rows, signed-commit checks.

use super::*;

/// Branches an MR reached since it was last loaded (`known`), other than its
/// `target` and the branches just added to tracking. Empty on first sighting:
/// when it got there is unknown.
#[cfg_attr(not(feature = "stats"), allow(dead_code))]
pub(super) fn reached_branches(
    known: Option<&HashSet<String>>,
    found: &HashSet<String>,
    target: &str,
    just_added: &HashSet<String>,
) -> Vec<String> {
    let Some(known) = known else {
        return Vec::new();
    };
    found
        .iter()
        .filter(|b| !known.contains(*b) && *b != target && !just_added.contains(*b))
        .cloned()
        .collect()
}

/// How far back a branch scan looks for copies of the MRs missing from it.
pub(super) const BRANCH_SCAN_LOOKBACK_DAYS: i64 = 90;

/// How `mr`, absent from a branch by SHA, reached it according to its `index`.
pub(super) fn copy_presence(mr: &TrackedMr, index: &BranchCommits) -> BranchPresence {
    index.presence_of(
        mr.sha.as_deref(),
        mr.landed_fingerprint.as_ref(),
        &mr.commits,
    )
}

/// MRs absent from `branch` by SHA that were missing from it in the `before` scan
/// and are fully there in the `after` one, with how they got there (for toasts).
pub(super) fn newly_copied<'a>(
    mrs: &'a [TrackedMr],
    branch: &'a str,
    before: &'a BranchCommits,
    after: &'a BranchCommits,
) -> impl Iterator<Item = (&'a TrackedMr, &'static str)> {
    mrs.iter().filter_map(move |mr| {
        if !matches!(&mr.status, MrStatus::MergedIn(found) if !found.contains(branch)) {
            return None;
        }
        if !copy_presence(mr, before).is_missing() {
            return None;
        }
        match copy_presence(mr, after) {
            BranchPresence::Present => Some((mr, "merged")),
            BranchPresence::CherryPicked => Some((mr, "cherry-picked")),
            BranchPresence::Copied => Some((mr, "copied")),
            BranchPresence::Partial | BranchPresence::Absent => None,
        }
    })
}

impl App {
    /// Builds a `FetchContext` from the current application state.
    ///
    /// Centralises the repeated construction of `FetchContext` that was
    /// previously scattered across `main.rs` and `events.rs`.
    pub fn fetch_context(&self) -> FetchContext {
        FetchContext {
            base_url: self.base_url.clone(),
            token: self.token.clone(),
            project_id: self.project_id.clone(),
            branches: self.branches.clone(),
        }
    }

    /// The tracked branches with their roles (see [`crate::branch_flow`]).
    #[cfg_attr(
        not(feature = "gui"),
        allow(dead_code, reason = "only the window draws the branch flow")
    )]
    pub fn branch_flow(&self) -> crate::branch_flow::BranchFlow {
        crate::branch_flow::BranchFlow::new(
            &self.branches,
            self.project_settings.branch_flow.as_ref(),
        )
    }

    /// How `mr` shows up on `branch`; `None` while it is loading or failed.
    pub fn branch_presence(&self, mr: &TrackedMr, branch: &str) -> Option<BranchPresence> {
        let MrStatus::MergedIn(found) = &mr.status else {
            return None;
        };
        if found.contains(branch) {
            return Some(BranchPresence::Present);
        }
        Some(
            self.branch_commits
                .get(branch)
                .map_or(BranchPresence::Absent, |index| copy_presence(mr, index)),
        )
    }

    /// [`App::branch_rows`] in the MR's flow: its target, up to production, the
    /// branches its changes come down to, then the others; each with its place.
    #[cfg_attr(
        not(feature = "gui"),
        allow(dead_code, reason = "only the window draws the branch flow")
    )]
    pub fn flow_branch_rows(&self, mr: &TrackedMr) -> Vec<(BranchRow, crate::branch_flow::Place)> {
        self.branch_flow().path(&mr.target_branch).arrange(
            &mr.target_branch,
            self.branch_rows(mr),
            |row| row.branch.as_str(),
        )
    }

    /// The merged MRs whose changes have to come down to `branch` (it is fed by
    /// their target, see [`crate::branch_flow`]) and are not (all) there yet,
    /// latest merge first: a rebase or a merge is due.
    #[cfg_attr(
        not(feature = "gui"),
        allow(dead_code, reason = "only the window draws the branch flow")
    )]
    pub fn behind_on(&self, branch: &str) -> Vec<&TrackedMr> {
        let flow = self.branch_flow();
        let mut behind: Vec<&TrackedMr> = self
            .mrs
            .iter()
            // `MergedIn` only says the MR was loaded: an open MR has it too.
            .filter(|mr| mr.state == GitlabMrState::Merged)
            .filter(|mr| {
                flow.path(&mr.target_branch)
                    .sync
                    .iter()
                    .any(|b| b == branch)
            })
            .filter(|mr| {
                self.branch_presence(mr, branch)
                    .is_some_and(|p| p.is_missing())
            })
            .collect();
        behind.sort_by(|a, b| b.merged_at.cmp(&a.merged_at));
        behind
    }

    /// Once no MR is loading any more, every MR has been fetched since the last
    /// branch was added: its arrivals are real again.
    pub(super) fn release_added_branches(&mut self) {
        if !self.just_added_branches.is_empty()
            && !self.mrs.iter().any(|mr| mr.status == MrStatus::Loading)
        {
            self.just_added_branches.clear();
        }
    }

    /// One row per tracked branch for the Inspector, with the commits still
    /// missing from the branches the MR only partially reached.
    pub fn branch_rows(&self, mr: &TrackedMr) -> Vec<BranchRow> {
        self.branches
            .iter()
            .map(|branch| {
                let presence = self.branch_presence(mr, branch);
                let missing = match (presence, self.branch_commits.get(branch)) {
                    (Some(BranchPresence::Partial), Some(index)) => mr
                        .commits
                        .iter()
                        .filter(|c| !index.contains_copy_of(c))
                        .cloned()
                        .collect(),
                    _ => Vec::new(),
                };
                BranchRow {
                    branch: branch.clone(),
                    presence,
                    missing,
                }
            })
            .collect()
    }

    /// The index `branch` has after a scan: rebuilt (`Full`) or grown by the
    /// commits read (`Range`), with its scope updated. `None` keeps the previous
    /// index; a failed range read also forgets the scope, so the next scan is full
    /// (the old head may be gone after a force-push).
    pub(super) fn scanned_index(
        &mut self,
        branch: &str,
        commits: Option<BranchCommits>,
        scan: crate::gitlab::BranchScan,
        head: Option<String>,
    ) -> Option<BranchCommits> {
        use crate::gitlab::{BranchScan, BranchScanScope};
        let Some(commits) = commits else {
            if matches!(scan, BranchScan::Range { .. }) {
                self.branch_scan_scopes.remove(branch);
            }
            return None;
        };
        match scan {
            BranchScan::Full { since } => {
                let now = chrono::Utc::now();
                let floor = chrono::DateTime::parse_from_rfc3339(&since)
                    .map_or(now, |at| at.with_timezone(&chrono::Utc));
                self.branch_scan_scopes.insert(
                    branch.to_string(),
                    BranchScanScope {
                        floor,
                        head,
                        full_at: now,
                    },
                );
                Some(commits)
            }
            BranchScan::Range { .. } => {
                let (Some(before), Some(scope)) = (
                    self.branch_commits.get(branch),
                    self.branch_scan_scopes.get_mut(branch),
                ) else {
                    // The index was dropped meanwhile: rebuild it next time.
                    self.branch_scan_scopes.remove(branch);
                    return None;
                };
                scope.head = head;
                let mut index = before.clone();
                index.absorb(commits);
                Some(index)
            }
        }
    }

    /// Scans the recent commits of each tracked branch that some MR is missing from
    /// by SHA (any state: an open MR's commits can be cherry-picked early), from the
    /// oldest such MR's creation on, at most [`BRANCH_SCAN_LOOKBACK_DAYS`] back.
    /// Branches with no such MR are skipped (no API call) and their index dropped.
    pub fn spawn_branch_scans(
        &mut self,
        semaphore: &Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
    ) {
        if self.read_only {
            return;
        }
        let floor = chrono::Utc::now() - chrono::TimeDelta::days(BRANCH_SCAN_LOOKBACK_DAYS);
        for branch in self.branches.clone() {
            let oldest = self
                .mrs
                .iter()
                .filter(|mr| matches!(&mr.status, MrStatus::MergedIn(found) if !found.contains(&branch)))
                .filter_map(|mr| mr.created_at.as_deref())
                .filter_map(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&chrono::Utc))
                .min();
            match oldest {
                Some(oldest) => {
                    let scope = self.branch_scan_scopes.get(&branch);
                    let scan = crate::gitlab::plan_branch_scan(
                        scope,
                        oldest.max(floor),
                        chrono::Utc::now(),
                    );
                    let previous_head = scope.and_then(|s| s.head.clone());
                    crate::gitlab::spawn_branch_commits_fetch(
                        self.fetch_context(),
                        branch,
                        scan,
                        previous_head,
                        semaphore.clone(),
                        tx.clone(),
                    );
                    self.pending_branch_scans += 1;
                }
                None => {
                    self.branch_commits.remove(&branch);
                    self.branch_scan_scopes.remove(&branch);
                }
            }
        }
    }

    /// Checks the commit signatures of the open MRs that lack something (new
    /// commits, a signature or a patch not read yet), when the project enforces
    /// signed commits. One check per MR at a time.
    pub fn spawn_signature_checks(
        &mut self,
        semaphore: &Arc<Semaphore>,
        tx: &UnboundedSender<AppEvent>,
    ) {
        let Some(policy) = self.config.signed_commits.clone() else {
            return;
        };
        if self.read_only {
            return;
        }
        let ctx = self.fetch_context();
        for mr in self.mrs.iter() {
            if mr.state != GitlabMrState::Opened || self.signature_checks.contains(&mr.id) {
                continue;
            }
            let author = crate::signatures::author_username(&mr.author).to_string();
            if !crate::gitlab_signatures::needs_check(
                mr.signatures.as_ref(),
                &mr.commits,
                &author,
                &policy,
            ) {
                continue;
            }
            self.signature_checks.insert(mr.id.clone());
            crate::gitlab_signatures::spawn_signature_check(
                ctx.clone(),
                crate::gitlab_signatures::CheckRequest {
                    mr_id: mr.id.clone(),
                    author,
                    commits: mr.commits.clone(),
                    data: mr.signatures.clone().unwrap_or_default(),
                    policy: policy.clone(),
                },
                semaphore.clone(),
                tx.clone(),
            );
        }
    }

    /// `mr`'s commits checked against the project's policy: `None` when the
    /// project does not enforce signed commits, the MR is not open, or nothing
    /// was read yet.
    #[cfg(feature = "gui")]
    pub fn signature_report(&self, mr: &TrackedMr) -> Option<crate::signatures::Report> {
        crate::signatures::report_for(mr, self.config.signed_commits.as_ref())
    }
}
