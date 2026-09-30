use sqlx::SqlitePool;

use crate::snapshot::MrStatsSnapshot;

/// Typed error returned by all [`SqliteStatsDb`] operations.
#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// Schema migrations, in order (see `SqliteStatsDb::run_migrations`).
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_initial.sql")];

/// A stored snapshot as returned by [`SqliteStatsDb::query`].
///
/// Mirrors [`MrStatsSnapshot`] but with the DB-assigned surrogate key included.
/// The `id` is used internally for the N:N join tables (labels, reviewers).
#[derive(Debug, Clone)]
pub struct StoredSnapshot {
    pub id: i64,
    pub recorded_at: String,
    pub snapshot: MrStatsSnapshot,
}

/// Filter applied when reading snapshots from the database.
#[derive(Debug, Clone, Default)]
pub struct SnapshotQuery {
    /// Restrict to a specific GitLab project ID.
    pub project_id: Option<String>,
    /// Restrict to MRs by this author username.
    pub author: Option<String>,
    /// Restrict to MRs that include this reviewer username.
    pub reviewer: Option<String>,
    /// Restrict to MRs targeting this branch.
    pub target_branch: Option<String>,
    /// Only return snapshots recorded on or after this ISO 8601 date.
    pub from_date: Option<String>,
    /// Only return snapshots recorded before this ISO 8601 date.
    pub to_date: Option<String>,
}

/// SQLite-backed store of MR snapshots — the persistence layer of the stats engine.
pub struct SqliteStatsDb {
    pool: SqlitePool,
}

impl SqliteStatsDb {
    /// Opens (or creates) the SQLite database at the given path and runs all pending migrations.
    ///
    /// `db_path` must be an absolute filesystem path (e.g. `/home/user/.config/gitlab-tracker/stats.db`).
    /// The file is created automatically when absent (`create_if_missing(true)`).
    pub async fn open(db_path: &str) -> Result<Self, StatsError> {
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr as _;

        // Connection settings, applied to every pooled connection (they used to be
        // PRAGMAs inside the schema script): WAL, and enforced foreign keys for the
        // `ON DELETE CASCADE` of labels / reviewers (sqlx's default, made explicit).
        let options = SqliteConnectOptions::from_str(db_path)
            .map_err(StatsError::Sqlx)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);

        let pool = SqlitePool::connect_with(options).await?;
        Self::run_migrations(&pool).await?;
        Ok(Self { pool })
    }

    /// Applies the pending schema migrations in order, each in its own transaction.
    ///
    /// `PRAGMA user_version` records how many of [`MIGRATIONS`] ran, so a new schema
    /// change is a new file appended to the list — never an edit of a published one.
    async fn run_migrations(pool: &SqlitePool) -> Result<(), StatsError> {
        let applied: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(pool)
            .await?;
        for (version, sql) in MIGRATIONS.iter().enumerate().skip(applied.max(0) as usize) {
            let mut tx = pool.begin().await?;
            sqlx::query(sql).execute(&mut *tx).await?;
            // PRAGMA arguments cannot be bound: the value is our own integer.
            sqlx::query(&format!("PRAGMA user_version = {}", version + 1))
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        Ok(())
    }
}

impl SqliteStatsDb {
    /// Records a MR snapshot, ignoring duplicates (same mr_id + project_id + calendar day).
    ///
    /// Idempotency is enforced at the DB level via `INSERT OR IGNORE` combined with
    /// the `UNIQUE(mr_id, project_id, DATE(recorded_at), trigger)` constraint.
    pub async fn upsert_snapshot(&self, snap: &MrStatsSnapshot) -> Result<(), StatsError> {
        self.upsert_snapshot_at(snap, &snap.event_recorded_at())
            .await
    }

    /// Same as `upsert_snapshot` but with an explicit `recorded_at` timestamp.
    ///
    /// Used during the startup backfill to place historical snapshots at their
    /// real event date (e.g. `merged_at`) rather than today, so cycle-time
    /// aggregations reflect actual history rather than the backfill date.
    pub async fn upsert_snapshot_at(
        &self,
        snap: &MrStatsSnapshot,
        recorded_at: &str,
    ) -> Result<(), StatsError> {
        // One transaction: the snapshot and its labels/reviewers land atomically.
        let mut tx = self.pool.begin().await?;
        insert_snapshot(&mut tx, snap, recorded_at).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Startup backfill: records each snapshot at its event date
    /// ([`MrStatsSnapshot::event_recorded_at`], like the live recorder), then fills
    /// `created_at` on that MR's rows where it is still NULL (rows inserted before
    /// the field was tracked — INSERT OR IGNORE never updates them). Everything runs
    /// in a single transaction: one fsync instead of one per row. Idempotent.
    pub async fn backfill_snapshots(&self, items: &[MrStatsSnapshot]) -> Result<(), StatsError> {
        let mut tx = self.pool.begin().await?;
        for snap in items {
            insert_snapshot(&mut tx, snap, &snap.event_recorded_at()).await?;
            if let Some(created_at) = &snap.created_at {
                sqlx::query(BACKFILL_CREATED_AT_SQL)
                    .bind(created_at)
                    .bind(&snap.project_id)
                    .bind(&snap.mr_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Startup maintenance, meant to run in the background once the UI is up:
    /// purges snapshots older than `retention_days`, repairs rows written by older
    /// versions ([`Self::repair_snapshots`], backed up to `backup_path` first when
    /// needed), then backfills `items` (the MRs restored from the state file, so
    /// stats are useful without waiting for live events). Each step logs its
    /// outcome; a failing step does not stop the next ones.
    pub async fn startup_maintenance(
        &self,
        project_id: &str,
        retention_days: u32,
        backup_path: Option<&str>,
        items: &[MrStatsSnapshot],
    ) {
        match self.purge_old_snapshots(project_id, retention_days).await {
            Ok(n) if n > 0 => {
                tracing::info!(rows = n, retention_days, "Purged old stats snapshots")
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "Stats purge failed"),
        }
        match self.repair_snapshots(project_id, backup_path).await {
            Ok(summary) if summary != RepairSummary::default() => tracing::info!(
                deleted = summary.deleted,
                redated = summary.redated,
                normalized = summary.normalized,
                backup = ?summary.backup_path,
                "Repaired legacy stats snapshots"
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "Stats repair failed"),
        }
        match self.backfill_snapshots(items).await {
            Ok(()) => tracing::info!(count = items.len(), "Stats backfill complete"),
            Err(e) => tracing::warn!(error = %e, "Stats backfill failed"),
        }
    }

    /// Returns all stored snapshots matching the given filter, ordered by `recorded_at` ASC.
    pub async fn query(&self, filter: &SnapshotQuery) -> Result<Vec<StoredSnapshot>, StatsError> {
        // Build a dynamic WHERE clause from the optional filter fields.
        // Using a Vec of conditions keeps the query readable and avoids a query-builder dependency.
        let mut conditions: Vec<&str> = Vec::new();
        let mut dynamic: Vec<String> = Vec::new();

        if let Some(pid) = &filter.project_id {
            conditions.push("s.project_id = ?");
            dynamic.push(pid.clone());
        }
        if let Some(author) = &filter.author {
            conditions.push("s.author = ?");
            dynamic.push(author.clone());
        }
        if let Some(branch) = &filter.target_branch {
            conditions.push("s.target_branch = ?");
            dynamic.push(branch.clone());
        }
        if let Some(from) = &filter.from_date {
            conditions.push("s.recorded_at >= ?");
            dynamic.push(from.clone());
        }
        if let Some(to) = &filter.to_date {
            conditions.push("s.recorded_at < ?");
            dynamic.push(to.clone());
        }

        // Reviewer sub-filter requires an EXISTS sub-query. It is pushed last so its
        // placeholder is bound after the `dynamic` values below.
        if filter.reviewer.is_some() {
            conditions.push(
                "EXISTS (SELECT 1 FROM mr_reviewers r WHERE r.snapshot_id = s.id AND r.reviewer = ?)",
            );
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT s.*, \
                (SELECT json_group_array(label) FROM mr_labels WHERE snapshot_id = s.id) AS labels_json, \
                (SELECT json_group_array(reviewer) FROM mr_reviewers WHERE snapshot_id = s.id) AS reviewers_json \
             FROM mr_snapshots s {where_clause} ORDER BY s.recorded_at ASC"
        );

        // Dynamic WHERE clause: the SQL text only contains fixed fragments and `?`
        // placeholders; every value goes through `.bind()`.
        let mut q = sqlx::query(&sql);
        for val in &dynamic {
            q = q.bind(val);
        }
        if let Some(reviewer) = &filter.reviewer {
            q = q.bind(reviewer);
        }

        let rows = q.fetch_all(&self.pool).await?;

        // Map raw rows back to StoredSnapshot. Labels and reviewers come back as JSON
        // arrays from correlated sub-selects: one round-trip instead of 1 + 2N queries,
        // and no cartesian product.
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            use sqlx::Row;
            let id: i64 = row.get("id");
            let labels = parse_json_list(row.get("labels_json"))?;
            let reviewers = parse_json_list(row.get("reviewers_json"))?;

            let trigger_str: String = row.get("trigger");
            let trigger = match trigger_str.parse::<crate::snapshot::SnapshotTrigger>() {
                Ok(trigger) => trigger,
                Err(e) => {
                    // Skip the row (e.g. written by a newer version) instead of
                    // counting it as an open-MR refresh or failing the whole report.
                    tracing::warn!(snapshot_id = id, error = %e, "Skipping stats snapshot");
                    continue;
                }
            };

            results.push(StoredSnapshot {
                id,
                recorded_at: row.get("recorded_at"),
                snapshot: MrStatsSnapshot {
                    mr_id: row.get("mr_id"),
                    project_id: row.get("project_id"),
                    title: row.get("title"),
                    trigger,
                    author: row.get("author"),
                    assignee: row.get("assignee"),
                    reviewers,
                    merged_by: row.get("merged_by"),
                    milestone: row.get("milestone"),
                    labels,
                    target_branch: row.get("target_branch"),
                    state: row.get("state"),
                    created_at: row.get("created_at"),
                    merged_at: row.get("merged_at"),
                    updated_at: row.get("updated_at"),
                    files_changed: row.get::<i64, _>("files_changed") as u32,
                    additions: row.get::<i64, _>("additions") as u32,
                    deletions: row.get::<i64, _>("deletions") as u32,
                    commits_count: row.get::<i64, _>("commits_count") as u32,
                    diff_difficulty: row.get("diff_difficulty"),
                    user_notes_count: row.get::<i64, _>("user_notes_count") as u32,
                    pipeline_count: row.get::<i64, _>("pipeline_count") as u32,
                    pipeline_failure_count: row.get::<i64, _>("pipeline_failure_count") as u32,
                },
            });
        }

        Ok(results)
    }

    /// Deletes all snapshots older than `retention_days` for the given project.
    ///
    /// Called once on startup when `stats_retention_days` is set in `projects.toml`.
    pub async fn purge_old_snapshots(
        &self,
        project_id: &str,
        retention_days: u32,
    ) -> Result<u64, StatsError> {
        let cutoff = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(retention_days as i64))
            .map(crate::snapshot::format_timestamp)
            .unwrap_or_default();

        let result =
            sqlx::query("DELETE FROM mr_snapshots WHERE project_id = ? AND recorded_at < ?")
                .bind(project_id)
                .bind(&cutoff)
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected())
    }

    /// Repairs rows written by older versions, on every startup (idempotent: a
    /// clean database is only read):
    /// 1. keeps one `on_merge` / `on_close` row per MR (the most recent `id`) — older
    ///    versions recorded a new one, dated today, on every refresh;
    /// 2. re-dates those rows at the real event time (`merged_at` / `updated_at`),
    ///    which the dedup of step 1 used to throw away with the backfilled row;
    /// 3. normalises every `recorded_at` to the canonical UTC format.
    ///
    /// When anything needs fixing and `backup_path` is given, a consistent copy of
    /// the database is written there first (`VACUUM INTO`).
    pub async fn repair_snapshots(
        &self,
        project_id: &str,
        backup_path: Option<&str>,
    ) -> Result<RepairSummary, StatsError> {
        let pending: i64 = sqlx::query_scalar(&repair_pending_sql())
            .bind(project_id)
            .fetch_one(&self.pool)
            .await?;
        tracing::debug!(project_id, pending, "Stats repair check");
        if pending == 0 {
            return Ok(RepairSummary::default());
        }

        let mut summary = RepairSummary::default();
        if let Some(path) = backup_path {
            // VACUUM INTO refuses to overwrite: drop a previous backup first.
            let _ = std::fs::remove_file(path);
            sqlx::query("VACUUM INTO ?")
                .bind(path)
                .execute(&self.pool)
                .await?;
            summary.backup_path = Some(path.to_string());
        }

        let mut tx = self.pool.begin().await?;
        summary.deleted = sqlx::query(&repair_dedup_sql())
            .bind(project_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        summary.redated = sqlx::query(&repair_redate_sql())
            .bind(project_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        summary.normalized = sqlx::query(&repair_normalize_sql())
            .bind(project_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(summary)
    }
}

/// What [`SqliteStatsDb::repair_snapshots`] changed (all zero on a clean database).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RepairSummary {
    /// Duplicate terminal rows removed.
    pub deleted: u64,
    /// Terminal rows moved to their real merge / close date.
    pub redated: u64,
    /// Rows whose `recorded_at` was rewritten in the canonical format.
    pub normalized: u64,
    /// Where the pre-repair backup was written, if one was needed.
    pub backup_path: Option<String>,
}

// SQLite's `strftime` parses ISO 8601 with fractional seconds and offsets and
// converts to UTC — the same canonical form as `snapshot::format_timestamp`.

/// The real event time of a terminal row, in canonical form.
const EVENT_TIME: &str =
    "strftime('%Y-%m-%dT%H:%M:%SZ', CASE trigger WHEN 'on_merge' THEN merged_at ELSE updated_at END)";
/// A row's `recorded_at`, in canonical form.
const CANONICAL_RECORDED_AT: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', recorded_at)";
const TERMINAL: &str = "trigger IN ('on_merge', 'on_close')";

/// Terminal rows that are not the latest of their (MR, trigger) group.
fn duplicate_terminal_rows() -> String {
    format!(
        "project_id = ?1 AND {TERMINAL} AND id NOT IN \
         (SELECT MAX(id) FROM mr_snapshots WHERE project_id = ?1 AND {TERMINAL} \
          GROUP BY mr_id, trigger)"
    )
}

/// Terminal rows not dated at their real event time.
fn misdated_terminal_rows() -> String {
    format!(
        "project_id = ?1 AND {TERMINAL} AND {EVENT_TIME} IS NOT NULL \
         AND recorded_at != {EVENT_TIME}"
    )
}

/// Rows whose `recorded_at` is not in canonical form.
fn unnormalized_rows() -> String {
    format!(
        "project_id = ?1 AND {CANONICAL_RECORDED_AT} IS NOT NULL \
         AND recorded_at != {CANONICAL_RECORDED_AT}"
    )
}

/// Number of rows [`SqliteStatsDb::repair_snapshots`] would change.
fn repair_pending_sql() -> String {
    format!(
        "SELECT (SELECT COUNT(*) FROM mr_snapshots WHERE {}) \
              + (SELECT COUNT(*) FROM mr_snapshots WHERE {}) \
              + (SELECT COUNT(*) FROM mr_snapshots WHERE {})",
        duplicate_terminal_rows(),
        misdated_terminal_rows(),
        unnormalized_rows()
    )
}

/// Step 1: one terminal row per (MR, trigger), the most recently inserted.
fn repair_dedup_sql() -> String {
    format!(
        "DELETE FROM mr_snapshots WHERE {}",
        duplicate_terminal_rows()
    )
}

/// Step 2: terminal rows dated at the real event (no UNIQUE clash after step 1).
fn repair_redate_sql() -> String {
    format!(
        "UPDATE OR IGNORE mr_snapshots \
         SET recorded_at = {EVENT_TIME}, recorded_date = substr({EVENT_TIME}, 1, 10) \
         WHERE {}",
        misdated_terminal_rows()
    )
}

/// Step 3: canonical `recorded_at` everywhere. `OR IGNORE`: a row whose UTC day
/// changes and clashes with an existing one keeps its old (still readable) value.
fn repair_normalize_sql() -> String {
    format!(
        "UPDATE OR IGNORE mr_snapshots \
         SET recorded_at = {CANONICAL_RECORDED_AT}, \
             recorded_date = substr({CANONICAL_RECORDED_AT}, 1, 10) \
         WHERE {}",
        unnormalized_rows()
    )
}

/// Only fills `created_at` where it is NULL — never overwrites existing data.
const BACKFILL_CREATED_AT_SQL: &str =
    "UPDATE mr_snapshots SET created_at = ? WHERE project_id = ? AND mr_id = ? AND created_at IS NULL";

/// Inserts one snapshot (ignored when it already exists for this day + trigger)
/// plus its labels and reviewers, on the caller's transaction.
async fn insert_snapshot(
    conn: &mut sqlx::SqliteConnection,
    snap: &MrStatsSnapshot,
    recorded_at: &str,
) -> Result<(), StatsError> {
    // One storage format for every row (UTC, seconds, `Z`): SQL filters compare
    // `recorded_at` as text, and `recorded_date` must be the UTC calendar day.
    let recorded_at = crate::snapshot::normalize_timestamp(recorded_at);
    // Derive the calendar date (YYYY-MM-DD) from recorded_at so it can be used
    // directly in the UNIQUE constraint column without relying on DATE() expressions,
    // which are not allowed in UNIQUE constraints on SQLite < 3.37.
    let recorded_date = recorded_at.get(..10).unwrap_or(&recorded_at).to_string();

    let row = sqlx::query(
        r#"
        INSERT OR IGNORE INTO mr_snapshots (
            recorded_at, recorded_date, trigger, mr_id, project_id, title, author, assignee,
            merged_by, milestone, target_branch, state,
            created_at, merged_at, updated_at,
            files_changed, additions, deletions, commits_count, diff_difficulty,
            user_notes_count, pipeline_count, pipeline_failure_count
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?,
            ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?, ?
        )
        RETURNING id
        "#,
    )
    .bind(&recorded_at)
    .bind(&recorded_date)
    .bind(snap.trigger.as_str())
    .bind(&snap.mr_id)
    .bind(&snap.project_id)
    .bind(&snap.title)
    .bind(&snap.author)
    .bind(&snap.assignee)
    .bind(&snap.merged_by)
    .bind(&snap.milestone)
    .bind(&snap.target_branch)
    .bind(&snap.state)
    .bind(&snap.created_at)
    .bind(&snap.merged_at)
    .bind(&snap.updated_at)
    .bind(snap.files_changed)
    .bind(snap.additions)
    .bind(snap.deletions)
    .bind(snap.commits_count)
    .bind(snap.diff_difficulty)
    .bind(snap.user_notes_count)
    .bind(snap.pipeline_count)
    .bind(snap.pipeline_failure_count)
    .fetch_optional(&mut *conn)
    .await?;

    // INSERT OR IGNORE returns no row when the UNIQUE constraint fires —
    // that means this snapshot already exists for this day+trigger, which is correct.
    let Some(row) = row else { return Ok(()) };
    let snapshot_id: i64 = sqlx::Row::get(&row, "id");

    for label in &snap.labels {
        sqlx::query("INSERT INTO mr_labels (snapshot_id, label) VALUES (?, ?)")
            .bind(snapshot_id)
            .bind(label)
            .execute(&mut *conn)
            .await?;
    }
    for reviewer in &snap.reviewers {
        sqlx::query("INSERT INTO mr_reviewers (snapshot_id, reviewer) VALUES (?, ?)")
            .bind(snapshot_id)
            .bind(reviewer)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Decodes a `json_group_array(...)` column into a list of strings.
fn parse_json_list(json: String) -> Result<Vec<String>, StatsError> {
    serde_json::from_str(&json).map_err(|e| StatsError::InvalidData(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SnapshotTrigger;

    fn snap(mr_id: &str, created_at: Option<&str>) -> MrStatsSnapshot {
        MrStatsSnapshot {
            mr_id: mr_id.into(),
            project_id: "p".into(),
            title: "t".into(),
            trigger: SnapshotTrigger::OnMerge,
            author: "a".into(),
            assignee: None,
            reviewers: vec!["r1".into(), "r2".into()],
            merged_by: None,
            milestone: None,
            labels: vec!["bug".into()],
            target_branch: "main".into(),
            state: "merged".into(),
            created_at: created_at.map(Into::into),
            merged_at: Some("2024-01-02T00:00:00Z".into()),
            updated_at: None,
            files_changed: 1,
            additions: 2,
            deletions: 3,
            commits_count: 1,
            diff_difficulty: None,
            user_notes_count: 0,
            pipeline_count: 0,
            pipeline_failure_count: 0,
        }
    }

    #[tokio::test]
    async fn migrations_are_versioned_and_idempotent() {
        let dir = std::env::temp_dir().join(format!("gt-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stats.db");
        let path = path.to_str().unwrap();
        let version = |db: &SqliteStatsDb| {
            let pool = db.pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>("PRAGMA user_version")
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            }
        };
        let db = SqliteStatsDb::open(path).await.unwrap();
        assert_eq!(version(&db).await, MIGRATIONS.len() as i64);
        db.upsert_snapshot(&snap("1", None)).await.unwrap();
        drop(db);
        // Reopening applies nothing and keeps the data.
        let db = SqliteStatsDb::open(path).await.unwrap();
        assert_eq!(version(&db).await, MIGRATIONS.len() as i64);
        assert_eq!(db.query(&SnapshotQuery::default()).await.unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn repair_fixes_legacy_rows_and_throughput() {
        use crate::aggregator::{QueryFilter, TimeWindow};
        // A file database, as in the app (`VACUUM INTO` backs up the real file).
        let dir = std::env::temp_dir().join(format!("gt-repair-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = SqliteStatsDb::open(dir.join("stats.db").to_str().unwrap())
            .await
            .unwrap();
        let fmt = |days_ago: i64| {
            crate::snapshot::format_timestamp(chrono::Utc::now() - chrono::Duration::days(days_ago))
        };
        // Legacy bug: a MR merged 60 days ago re-recorded "today" on two refresh days.
        let merged_at = fmt(60); // computed once: `fmt` moves with the clock
        let mut merged = snap("1", None);
        merged.merged_at = Some(merged_at.clone());
        db.upsert_snapshot_at(&merged, &fmt(1)).await.unwrap();
        db.upsert_snapshot_at(&merged, &fmt(0)).await.unwrap();
        // Legacy format: an open-MR refresh stored with `to_rfc3339()`'s offset.
        let mut open = snap("2", None);
        open.trigger = SnapshotTrigger::OnRefresh;
        open.merged_at = None;
        db.upsert_snapshot_at(&open, &fmt(2)).await.unwrap();
        sqlx::query("UPDATE mr_snapshots SET recorded_at = replace(recorded_at, 'Z', '.5+00:00') WHERE mr_id = '2'")
            .execute(&db.pool)
            .await
            .unwrap();

        let last_30_days = QueryFilter {
            window: Some(TimeWindow::LastDays(30)),
            project_id: Some("p".into()),
            ..Default::default()
        };
        let merged_last_30 = || async {
            crate::aggregator::aggregate(&db, &last_30_days)
                .await
                .unwrap()
                .merged_count
        };
        assert_eq!(
            merged_last_30().await,
            1,
            "legacy rows count an old merge as recent"
        );

        let backup = dir.join("stats.db.bak");
        let backup = backup.to_str().unwrap();
        let summary = db.repair_snapshots("p", Some(backup)).await.unwrap();
        assert_eq!(
            (summary.deleted, summary.redated, summary.normalized),
            (1, 1, 1)
        );
        assert!(
            std::path::Path::new(backup).exists(),
            "backup written before repairing"
        );
        std::fs::remove_file(backup).unwrap();

        let rows = db.query(&SnapshotQuery::default()).await.unwrap();
        let merge_rows: Vec<_> = rows.iter().filter(|r| r.snapshot.mr_id == "1").collect();
        assert_eq!(merge_rows.len(), 1);
        assert_eq!(merge_rows[0].recorded_at, merged_at);
        assert!(rows
            .iter()
            .all(|r| r.recorded_at.ends_with('Z') && r.recorded_at.len() == 20));
        assert_eq!(
            merged_last_30().await,
            0,
            "the old merge left the 30-day window"
        );

        // Idempotent: a clean database is only read, no new backup.
        let again = db.repair_snapshots("p", Some(backup)).await.unwrap();
        assert_eq!(again, RepairSummary::default());
        assert!(!std::path::Path::new(backup).exists());

        // And the fixed recorder cannot re-create the bug: re-recording is a no-op.
        db.upsert_snapshot(&merged).await.unwrap();
        assert_eq!(merged_last_30().await, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn backfill_roundtrip_and_created_at_patch() {
        let db = SqliteStatsDb::open(":memory:").await.unwrap();
        let at = "2024-01-02T00:00:00Z".to_string();

        // First run: created_at unknown. Second run: same row (ignored) but created_at patched.
        // `snap` is an OnMerge merged at `at`: the backfill records it at that date.
        db.backfill_snapshots(&[snap("1", None), snap("2", None)])
            .await
            .unwrap();
        db.backfill_snapshots(&[snap("1", Some("2024-01-01T00:00:00Z"))])
            .await
            .unwrap();
        assert!(db
            .query(&SnapshotQuery::default())
            .await
            .unwrap()
            .iter()
            .all(|r| r.recorded_at == at));

        let rows = db.query(&SnapshotQuery::default()).await.unwrap();
        assert_eq!(rows.len(), 2);
        let one = rows.iter().find(|r| r.snapshot.mr_id == "1").unwrap();
        assert_eq!(one.snapshot.labels, vec!["bug".to_string()]);
        assert_eq!(one.snapshot.reviewers.len(), 2);
        assert_eq!(
            one.snapshot.created_at.as_deref(),
            Some("2024-01-01T00:00:00Z")
        );

        let by_reviewer = db
            .query(&SnapshotQuery {
                reviewer: Some("r2".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_reviewer.len(), 2);
    }
}
