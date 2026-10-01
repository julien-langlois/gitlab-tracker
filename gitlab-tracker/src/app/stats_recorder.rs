//! Stats recording on MR loads. Split out of `app.rs`.

use super::*;
use gitlab_tracker_stats::{ArrivalKind, BranchArrival};

impl App {
    /// Records the arrival of `mr_id` on the `reached` branches (same SHA: merge,
    /// fast-forward, reset), dated now — the refresh that first saw it there.
    pub(super) fn record_sha_arrivals(
        &self,
        mr_id: &str,
        reached: Vec<String>,
        tx: &UnboundedSender<AppEvent>,
    ) {
        let Some(mr) = self.mrs.iter().find(|mr| mr.id == mr_id) else {
            return;
        };
        let now = chrono::Utc::now();
        let arrivals = reached
            .into_iter()
            .map(|branch| self.branch_arrival(mr, branch, now, ArrivalKind::Merge))
            .collect();
        self.write_branch_arrivals(arrivals, tx);
    }

    /// Records the MRs fully copied to `branch` according to its fresh `index`,
    /// dated at the copy's commit date. Already-recorded ones are no-ops in the DB.
    pub(super) fn record_copy_arrivals(
        &self,
        branch: &str,
        index: &BranchCommits,
        tx: &UnboundedSender<AppEvent>,
    ) {
        let arrivals = self
            .mrs
            .iter()
            .filter(|mr| mr.target_branch != branch)
            .filter(|mr| matches!(&mr.status, MrStatus::MergedIn(found) if !found.contains(branch)))
            .filter_map(|mr| {
                let landed = mr.landed_fingerprint.as_ref();
                let at = index.copied_at(mr.sha.as_deref(), landed, &mr.commits)?;
                let at = chrono::DateTime::from_timestamp(at, 0)?;
                let how = match index.presence_of(mr.sha.as_deref(), landed, &mr.commits) {
                    BranchPresence::CherryPicked => ArrivalKind::CherryPick,
                    _ => ArrivalKind::Copy,
                };
                Some(self.branch_arrival(mr, branch.to_string(), at, how))
            })
            .collect();
        self.write_branch_arrivals(arrivals, tx);
    }

    fn branch_arrival(
        &self,
        mr: &TrackedMr,
        branch: String,
        arrived_at: chrono::DateTime<chrono::Utc>,
        how: ArrivalKind,
    ) -> BranchArrival {
        BranchArrival {
            project_id: self.project_id.clone(),
            mr_id: mr.id.clone(),
            branch,
            arrived_at,
            how,
            merged_at: mr.merged_at.clone(),
        }
    }

    /// Writes `arrivals` in the background; the report is refreshed (debounced)
    /// only when one of them actually changed the DB.
    fn write_branch_arrivals(&self, arrivals: Vec<BranchArrival>, tx: &UnboundedSender<AppEvent>) {
        let Some(db) = self.stats_db.clone() else {
            return;
        };
        if arrivals.is_empty() {
            return;
        }
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut changed = false;
            for arrival in &arrivals {
                match db.record_branch_arrival(arrival).await {
                    Ok(written) => changed |= written,
                    Err(e) => {
                        tracing::warn!(error = %e, mr_id = %arrival.mr_id, "Failed to record branch arrival")
                    }
                }
            }
            if changed {
                let _ = tx.send(AppEvent::StatsSnapshotRecorded);
            }
        });
    }

    /// Records a stats snapshot for `mr_id` after a GitLab load:
    /// - `OnMerge` / `OnClose` on every load of a merged / closed MR — dated at the
    ///   real merge / close time (`event_recorded_at`), so only the first one is
    ///   stored and the others hit the UNIQUE constraint;
    /// - `OnRefresh` at most once per calendar day for open MRs.
    ///
    /// `refresh_report`: ask for a (debounced) report refresh once written — only
    /// after the startup sync, to avoid one recompute per restored MR.
    pub(super) fn record_stats_snapshot(
        &mut self,
        mr_id: &str,
        refresh_report: bool,
        tx: &UnboundedSender<AppEvent>,
    ) {
        use gitlab_tracker_stats::snapshot::SnapshotTrigger;

        let Some(db) = self.stats_db.clone() else {
            return;
        };
        let Some(mr) = self.mrs.find_mut(mr_id) else {
            return;
        };
        let trigger = match &mr.state {
            GitlabMrState::Merged => SnapshotTrigger::OnMerge,
            GitlabMrState::Closed => SnapshotTrigger::OnClose,
            GitlabMrState::Opened => {
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                if self.stats_last_refresh_date.get(&mr.id) == Some(&today) {
                    return;
                }
                self.stats_last_refresh_date.insert(mr.id.clone(), today);
                SnapshotTrigger::OnRefresh
            }
        };
        let snap = mr.stats_snapshot(trigger, &self.project_id, &self.config.complexity_profile);
        let project_id = self.project_id.clone();
        let tx = refresh_report.then(|| tx.clone());
        tokio::spawn(async move {
            if let Err(e) = db.upsert_snapshot(&snap).await {
                tracing::warn!(project_id = %project_id, error = %e, "Failed to record stats snapshot");
                return;
            }
            // The report itself is recomputed on the next Tick (debounced), not once
            // per recorded snapshot.
            if let Some(tx) = tx {
                let _ = tx.send(AppEvent::StatsSnapshotRecorded);
            }
        });
    }
}
