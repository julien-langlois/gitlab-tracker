//! Stats recording on MR loads. Split out of `app.rs`.

use super::*;

impl App {
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
