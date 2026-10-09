//! Stats recording on MR loads. Split out of `app.rs`.

use super::*;
use gitlab_tracker_stats::{ArrivalKind, BranchArrival};

/// Whether recording a snapshot asks for a (debounced) report refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReportRefresh {
    /// Startup load in progress: no recompute per restored MR.
    Never,
    /// Once the snapshot is stored, even as a duplicate: the load completing the
    /// startup sync, so the report covers everything recorded during it.
    Always,
    /// Only when the snapshot added a row (loads after the startup sync).
    IfWritten,
}

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
    /// - `OnRefresh` at most once per calendar day for open MRs (a failed write
    ///   releases the day, see `AppEvent::StatsSnapshotFailed`).
    ///
    /// `refresh_report`: when to ask for a (debounced) report refresh, which is
    /// what the cockpit Flow counters are read from.
    pub(super) fn record_stats_snapshot(
        &mut self,
        mr_id: &str,
        refresh_report: ReportRefresh,
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
        let tx = tx.clone();
        tokio::spawn(async move {
            let written = match db.upsert_snapshot(&snap).await {
                Ok(written) => written,
                Err(e) => {
                    tracing::warn!(project_id = %project_id, error = %e, "Failed to record stats snapshot");
                    // Terminal snapshots are retried on every load anyway.
                    if snap.trigger == SnapshotTrigger::OnRefresh {
                        let _ = tx.send(AppEvent::StatsSnapshotFailed { mr_id: snap.mr_id });
                    }
                    return;
                }
            };
            // The report itself is recomputed on the next Tick (debounced), not once
            // per recorded snapshot.
            let refresh = match refresh_report {
                ReportRefresh::Never => false,
                ReportRefresh::Always => true,
                ReportRefresh::IfWritten => written,
            };
            if refresh {
                let _ = tx.send(AppEvent::StatsSnapshotRecorded);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::test_app;
    use super::*;
    use crate::models::CommitFingerprint;
    use gitlab_tracker_stats::db::SnapshotQuery;
    use gitlab_tracker_stats::snapshot::SnapshotTrigger;
    use gitlab_tracker_stats::SqliteStatsDb;

    const MERGED_AT: &str = "2026-01-05T10:00:00Z";

    async fn app_with_db() -> (App, Arc<SqliteStatsDb>) {
        let db = Arc::new(SqliteStatsDb::open(":memory:").await.unwrap());
        let mut app = test_app();
        app.stats_db = Some(db.clone());
        (app, db)
    }

    fn mr(id: &str, state: GitlabMrState) -> TrackedMr {
        let mut mr = TrackedMr::placeholder(id.into(), format!("MR {id}"), None);
        mr.data.state = state;
        mr.data.target_branch = "main".into();
        if state == GitlabMrState::Merged {
            mr.status = MrStatus::MergedIn(["main".to_string()].into());
            mr.data.merged_at = Some(MERGED_AT.into());
        }
        mr
    }

    /// Runs `act`, waits for the background writes it spawned and returns how many
    /// report refreshes they asked for. (A writer keeps its sender until it is done:
    /// the channel closes when the last one finishes.)
    async fn refresh_requests(
        app: &mut App,
        act: impl FnOnce(&mut App, &UnboundedSender<AppEvent>),
    ) -> usize {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        act(app, &tx);
        drop(tx);
        let mut requests = 0;
        while let Some(event) = rx.recv().await {
            if matches!(event, AppEvent::StatsSnapshotRecorded) {
                requests += 1;
            }
        }
        requests
    }

    async fn triggers_of(db: &SqliteStatsDb, mr_id: &str) -> Vec<SnapshotTrigger> {
        let query = SnapshotQuery {
            project_id: Some("1".into()),
            ..SnapshotQuery::default()
        };
        db.query(&query)
            .await
            .unwrap()
            .into_iter()
            .filter(|stored| stored.snapshot.mr_id == mr_id)
            .map(|stored| stored.snapshot.trigger)
            .collect()
    }

    #[tokio::test]
    async fn open_mrs_are_snapshotted_once_a_day_and_terminal_ones_once() {
        let (mut app, db) = app_with_db().await;
        app.mrs = vec![
            mr("1", GitlabMrState::Opened),
            mr("2", GitlabMrState::Merged),
        ];

        let requests = refresh_requests(&mut app, |app, tx| {
            app.record_stats_snapshot("1", ReportRefresh::IfWritten, tx);
            // Same day: skipped before reaching the DB.
            app.record_stats_snapshot("1", ReportRefresh::IfWritten, tx);
            app.record_stats_snapshot("404", ReportRefresh::IfWritten, tx);
        })
        .await;
        assert_eq!(requests, 1);
        assert_eq!(triggers_of(&db, "1").await, [SnapshotTrigger::OnRefresh]);
        assert!(app.stats_last_refresh_date.contains_key("1"));

        // A merged MR is recorded on every load, but dated at its merge: one row,
        // and one report refresh — when the merge is first seen (e.g. an MR created
        // and merged between two polls, discovered already merged).
        for expected in [1, 0] {
            let requests = refresh_requests(&mut app, |app, tx| {
                app.record_stats_snapshot("2", ReportRefresh::IfWritten, tx)
            })
            .await;
            assert_eq!(requests, expected);
        }
        // The load completing the startup sync refreshes even without a new row.
        let requests = refresh_requests(&mut app, |app, tx| {
            app.record_stats_snapshot("2", ReportRefresh::Always, tx)
        })
        .await;
        assert_eq!(requests, 1);
        assert_eq!(triggers_of(&db, "2").await, [SnapshotTrigger::OnMerge]);
        assert!(!app.stats_last_refresh_date.contains_key("2"));
    }

    #[tokio::test]
    async fn a_failed_daily_snapshot_releases_the_day() {
        let (mut app, db) = app_with_db().await;
        app.mrs = vec![mr("1", GitlabMrState::Opened)];
        let record = |app: &mut App, tx: &UnboundedSender<AppEvent>| {
            app.record_stats_snapshot("1", ReportRefresh::IfWritten, tx)
        };
        refresh_requests(&mut app, record).await;
        assert!(app.stats_last_refresh_date.contains_key("1"));

        // What the background write sends when the DB refuses the row.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        app.apply_event(
            AppEvent::StatsSnapshotFailed { mr_id: "1".into() },
            Arc::new(Semaphore::new(1)),
            &tx,
            &mut HashMap::new(),
        );
        assert!(!app.stats_last_refresh_date.contains_key("1"));

        // The next load reaches the DB again instead of being skipped until tomorrow.
        refresh_requests(&mut app, record).await;
        assert!(app.stats_last_refresh_date.contains_key("1"));
        assert_eq!(triggers_of(&db, "1").await, [SnapshotTrigger::OnRefresh]);
    }

    #[tokio::test]
    async fn startup_backfill_records_without_asking_for_a_report() {
        let (mut app, db) = app_with_db().await;
        app.mrs = vec![mr("3", GitlabMrState::Closed)];

        let requests = refresh_requests(&mut app, |app, tx| {
            app.record_stats_snapshot("3", ReportRefresh::Never, tx)
        })
        .await;
        assert_eq!(requests, 0);
        assert_eq!(triggers_of(&db, "3").await, [SnapshotTrigger::OnClose]);
    }

    #[tokio::test]
    async fn sha_arrivals_are_recorded_once_per_branch() {
        let (mut app, db) = app_with_db().await;
        app.mrs = vec![mr("10", GitlabMrState::Merged)];
        let reach = |app: &mut App, tx: &UnboundedSender<AppEvent>| {
            app.record_sha_arrivals("10", vec!["staging".into(), "prod".into()], tx);
        };

        assert_eq!(refresh_requests(&mut app, reach).await, 1);
        // Seen again at the next refresh: nothing changes, no report refresh.
        assert_eq!(refresh_requests(&mut app, reach).await, 0);
        let unknown = refresh_requests(&mut app, |app, tx| {
            app.record_sha_arrivals("404", vec!["staging".into()], tx)
        })
        .await;
        assert_eq!(unknown, 0);

        let mut arrivals = db.query_branch_arrivals("1", None, None).await.unwrap();
        arrivals.sort_by(|a, b| a.branch.cmp(&b.branch));
        let summary: Vec<_> = arrivals
            .iter()
            .map(|a| (a.mr_id.as_str(), a.branch.as_str(), a.how))
            .collect();
        assert_eq!(
            summary,
            [
                ("10", "prod", ArrivalKind::Merge),
                ("10", "staging", ArrivalKind::Merge)
            ]
        );
        assert!(arrivals.iter().all(|a| a.merged_at.is_some()));
    }

    #[tokio::test]
    async fn copies_are_dated_at_the_copy_commit_and_typed_by_how_they_were_found() {
        const PICKED_AT: i64 = 1_767_700_000;
        const COPIED_AT: i64 = 1_767_800_000;
        let fingerprint = CommitFingerprint {
            author_email: "alice@example.com".into(),
            authored_at: 1_767_000_000,
            title: "Fix login".into(),
        };

        let (mut app, db) = app_with_db().await;
        let mut picked = mr("20", GitlabMrState::Merged);
        picked.data.sha = Some("aaa".into());
        let mut copied = mr("21", GitlabMrState::Merged);
        copied.data.sha = Some("bbb".into());
        copied.data.landed_fingerprint = Some(fingerprint.clone());
        // Not candidates: already known on prod, targeting prod, or still open.
        let mut already_there = mr("22", GitlabMrState::Merged);
        already_there.data.sha = Some("ccc".into());
        already_there.status = MrStatus::MergedIn(["main".to_string(), "prod".to_string()].into());
        let mut targets_prod = mr("23", GitlabMrState::Merged);
        targets_prod.data.sha = Some("ddd".into());
        targets_prod.data.target_branch = "prod".into();
        let mut open = mr("24", GitlabMrState::Opened);
        open.data.sha = Some("eee".into());
        // Merged, but its commit is nowhere on prod.
        let mut absent = mr("25", GitlabMrState::Merged);
        absent.data.sha = Some("fff".into());
        app.mrs = vec![picked, copied, already_there, targets_prod, open, absent];

        let index = BranchCommits {
            shas: HashSet::new(),
            picked_from: ["aaa", "ccc", "ddd", "eee"]
                .into_iter()
                .map(|sha| (sha.to_string(), PICKED_AT))
                .collect(),
            fingerprints: HashMap::from([(fingerprint, COPIED_AT)]),
        };
        let requests = refresh_requests(&mut app, |app, tx| {
            app.record_copy_arrivals("prod", &index, tx)
        })
        .await;
        assert_eq!(requests, 1);

        let mut arrivals = db.query_branch_arrivals("1", None, None).await.unwrap();
        arrivals.sort_by(|a, b| a.mr_id.cmp(&b.mr_id));
        let summary: Vec<_> = arrivals
            .iter()
            .map(|a| {
                (
                    a.mr_id.as_str(),
                    a.branch.as_str(),
                    a.how,
                    a.arrived_at.timestamp(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("20", "prod", ArrivalKind::CherryPick, PICKED_AT),
                ("21", "prod", ArrivalKind::Copy, COPIED_AT)
            ]
        );
    }

    #[tokio::test]
    async fn without_a_stats_db_nothing_is_recorded() {
        let mut app = test_app();
        app.mrs = vec![
            mr("1", GitlabMrState::Opened),
            mr("2", GitlabMrState::Merged),
        ];
        let requests = refresh_requests(&mut app, |app, tx| {
            app.record_stats_snapshot("1", ReportRefresh::IfWritten, tx);
            app.record_sha_arrivals("2", vec!["prod".into()], tx);
            app.record_copy_arrivals("prod", &BranchCommits::default(), tx);
        })
        .await;
        assert_eq!(requests, 0);
        // The daily guard is not consumed: the snapshot is taken once a DB exists.
        assert!(app.stats_last_refresh_date.is_empty());
    }
}
