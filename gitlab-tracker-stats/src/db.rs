use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::snapshot::MrStatsSnapshot;

/// Typed error returned by all [`StatsDb`] operations.
#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// A stored snapshot as returned by [`StatsDb::query`].
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
    /// Restrict to MRs attached to this milestone title.
    pub milestone: Option<String>,
    /// Restrict to MRs targeting this branch.
    pub target_branch: Option<String>,
    /// Only return snapshots recorded on or after this ISO 8601 date.
    pub from_date: Option<String>,
    /// Only return snapshots recorded before this ISO 8601 date.
    pub to_date: Option<String>,
    /// Only return snapshots with this trigger type ("on_merge", "on_close", "on_refresh").
    pub trigger: Option<String>,
}

/// Persistence contract for the stats engine.
///
/// The trait boundary keeps [`SqliteStatsDb`] swappable with an in-memory
/// implementation for unit tests, without any changes to the aggregation logic.
#[async_trait]
pub trait StatsDb: Send + Sync {
    /// Records a MR snapshot, ignoring duplicates (same mr_id + project_id + calendar day).
    ///
    /// Idempotency is enforced at the DB level via `INSERT OR IGNORE` combined with
    /// the `UNIQUE(mr_id, project_id, DATE(recorded_at), trigger)` constraint.
    async fn upsert_snapshot(&self, snap: &MrStatsSnapshot) -> Result<(), StatsError>;

    /// Same as [`upsert_snapshot`] but with an explicit `recorded_at` timestamp.
    ///
    /// Used during the startup backfill to place historical snapshots at their
    /// real event date (e.g. `merged_at`) rather than today, so cycle-time
    /// aggregations reflect actual history rather than the backfill date.
    async fn upsert_snapshot_at(
        &self,
        snap: &MrStatsSnapshot,
        recorded_at: &str,
    ) -> Result<(), StatsError>;

    /// Returns all stored snapshots matching the given filter, ordered by `recorded_at` ASC.
    async fn query(&self, filter: &SnapshotQuery) -> Result<Vec<StoredSnapshot>, StatsError>;

    /// Deletes all snapshots older than `retention_days` for the given project.
    ///
    /// Called once on startup when `stats_retention_days` is set in `projects.toml`.
    async fn purge_old_snapshots(
        &self,
        project_id: &str,
        retention_days: u32,
    ) -> Result<u64, StatsError>;
}

/// SQLite-backed implementation of [`StatsDb`].
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

        let options = SqliteConnectOptions::from_str(db_path)
            .map_err(StatsError::Sqlx)?
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;
        Self::run_migrations(&pool).await?;
        Ok(Self { pool })
    }

    /// Applies the embedded schema migrations in order.
    ///
    /// Using inline SQL rather than the `sqlx::migrate!` macro avoids the
    /// compile-time `DATABASE_URL` requirement, which would break `cargo build`
    /// in environments where the DB does not exist yet.
    async fn run_migrations(pool: &SqlitePool) -> Result<(), StatsError> {
        sqlx::query(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS mr_snapshots (
                id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                recorded_at            TEXT    NOT NULL,
                -- Stores the calendar date (YYYY-MM-DD) of recorded_at for use in the UNIQUE
                -- constraint. Expressions (e.g. DATE()) are not allowed in UNIQUE constraints
                -- on older SQLite versions (< 3.37), so we materialise the value explicitly.
                recorded_date          TEXT    NOT NULL,
                trigger                TEXT    NOT NULL,
                mr_id                  TEXT    NOT NULL,
                project_id             TEXT    NOT NULL,
                title                  TEXT    NOT NULL,
                author                 TEXT    NOT NULL,
                assignee               TEXT,
                merged_by              TEXT,
                milestone              TEXT,
                target_branch          TEXT    NOT NULL,
                state                  TEXT    NOT NULL,
                created_at             TEXT,
                merged_at              TEXT,
                updated_at             TEXT,
                files_changed          INTEGER NOT NULL DEFAULT 0,
                additions              INTEGER NOT NULL DEFAULT 0,
                deletions              INTEGER NOT NULL DEFAULT 0,
                commits_count          INTEGER NOT NULL DEFAULT 0,
                diff_difficulty        REAL,
                user_notes_count       INTEGER NOT NULL DEFAULT 0,
                pipeline_count         INTEGER NOT NULL DEFAULT 0,
                pipeline_failure_count INTEGER NOT NULL DEFAULT 0,
                -- Idempotency: one snapshot per MR, per project, per calendar day, per trigger.
                UNIQUE(mr_id, project_id, recorded_date, trigger)
            );

            CREATE TABLE IF NOT EXISTS mr_labels (
                snapshot_id INTEGER NOT NULL REFERENCES mr_snapshots(id) ON DELETE CASCADE,
                label       TEXT    NOT NULL
            );

            CREATE TABLE IF NOT EXISTS mr_reviewers (
                snapshot_id INTEGER NOT NULL REFERENCES mr_snapshots(id) ON DELETE CASCADE,
                reviewer    TEXT    NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_snapshots_project_date
                ON mr_snapshots(project_id, recorded_date);

            CREATE INDEX IF NOT EXISTS idx_snapshots_author
                ON mr_snapshots(author);

            CREATE INDEX IF NOT EXISTS idx_snapshots_milestone
                ON mr_snapshots(milestone);
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl StatsDb for SqliteStatsDb {
    async fn upsert_snapshot(&self, snap: &MrStatsSnapshot) -> Result<(), StatsError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.upsert_snapshot_at(snap, &now).await
    }

    async fn upsert_snapshot_at(
        &self,
        snap: &MrStatsSnapshot,
        recorded_at: &str,
    ) -> Result<(), StatsError> {
        let trigger = snap.trigger.as_str();

        // Derive the calendar date (YYYY-MM-DD) from recorded_at so it can be used
        // directly in the UNIQUE constraint column without relying on DATE() expressions,
        // which are not allowed in UNIQUE constraints on SQLite < 3.37.
        let recorded_date = recorded_at.get(..10).unwrap_or(recorded_at);

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
        .bind(recorded_at)
        .bind(recorded_date)
        .bind(trigger)
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
        .fetch_optional(&self.pool)
        .await?;

        // INSERT OR IGNORE returns no row when the UNIQUE constraint fires —
        // that means this snapshot already exists for this day+trigger, which is correct.
        let Some(row) = row else { return Ok(()) };

        let snapshot_id: i64 = sqlx::Row::get(&row, "id");

        // Insert labels.
        for label in &snap.labels {
            sqlx::query("INSERT INTO mr_labels (snapshot_id, label) VALUES (?, ?)")
                .bind(snapshot_id)
                .bind(label)
                .execute(&self.pool)
                .await?;
        }

        // Insert reviewers.
        for reviewer in &snap.reviewers {
            sqlx::query("INSERT INTO mr_reviewers (snapshot_id, reviewer) VALUES (?, ?)")
                .bind(snapshot_id)
                .bind(reviewer)
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    async fn query(&self, filter: &SnapshotQuery) -> Result<Vec<StoredSnapshot>, StatsError> {
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
        if let Some(milestone) = &filter.milestone {
            conditions.push("s.milestone = ?");
            dynamic.push(milestone.clone());
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
        if let Some(trigger) = &filter.trigger {
            conditions.push("s.trigger = ?");
            dynamic.push(trigger.clone());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Reviewer sub-filter requires a separate EXISTS sub-query.
        let reviewer_clause = if filter.reviewer.is_some() {
            "AND EXISTS (SELECT 1 FROM mr_reviewers r WHERE r.snapshot_id = s.id AND r.reviewer = ?)"
        } else {
            ""
        };

        let sql = format!(
            "SELECT s.* FROM mr_snapshots s {where_clause} {reviewer_clause} ORDER BY s.recorded_at ASC"
        );

        // sqlx does not support fully dynamic binding without a query builder, so we
        // fall back to raw queries with manual binding via `query()` + `.bind()` chaining.
        // This is safe because all values come from trusted application code, not user input.
        let mut q = sqlx::query(&sql);
        for val in &dynamic {
            q = q.bind(val);
        }
        if let Some(reviewer) = &filter.reviewer {
            q = q.bind(reviewer);
        }

        let rows = q.fetch_all(&self.pool).await?;

        // Map raw rows back to StoredSnapshot. Labels and reviewers are loaded separately
        // per snapshot to avoid a cartesian product in the main query.
        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            use sqlx::Row;
            let id: i64 = row.get("id");

            let labels: Vec<String> =
                sqlx::query_scalar("SELECT label FROM mr_labels WHERE snapshot_id = ?")
                    .bind(id)
                    .fetch_all(&self.pool)
                    .await?;

            let reviewers: Vec<String> =
                sqlx::query_scalar("SELECT reviewer FROM mr_reviewers WHERE snapshot_id = ?")
                    .bind(id)
                    .fetch_all(&self.pool)
                    .await?;

            let trigger_str: String = row.get("trigger");
            let trigger = match trigger_str.as_str() {
                "on_merge" => crate::snapshot::SnapshotTrigger::OnMerge,
                "on_close" => crate::snapshot::SnapshotTrigger::OnClose,
                _ => crate::snapshot::SnapshotTrigger::OnRefresh,
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

    async fn purge_old_snapshots(
        &self,
        project_id: &str,
        retention_days: u32,
    ) -> Result<u64, StatsError> {
        let cutoff = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(retention_days as i64))
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();

        let result =
            sqlx::query("DELETE FROM mr_snapshots WHERE project_id = ? AND recorded_at < ?")
                .bind(project_id)
                .bind(&cutoff)
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected())
    }
}
