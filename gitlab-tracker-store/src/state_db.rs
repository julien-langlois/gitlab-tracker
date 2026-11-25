//! The tracker's state in SQLite (`tracker.db`, next to `projects.toml`): the
//! tracked MRs of every project, enough to start with them, and what changed on
//! them day after day.
//!
//! Each save compares every MR with what was stored and appends what changed to
//! `mr_events` (approved, merged, pipeline, label_added…); an MR seen for the
//! first time gets the history its GitLab data already tells (opened, review
//! requests, approvals, pipelines, threads, milestone moves), dated by GitLab.
//! Pipelines and threads are kept in their own tables once seen, after GitLab's
//! lists move on; an MR removed from the dashboard keeps its row and history.
//!
//! `tracker_<hash>.json`, which held the state before, is imported once, the first
//! time a project is opened (see `storage::load_state_async`).

use gitlab_tracker_gitlab::models::{
    GitlabMrState, MergeabilityStatus, MrData, SavedMr, SavedState,
};
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

mod caches;
mod events;
mod tracked;
mod writes;

pub use events::*;
use writes::*;

/// The database file, in the config directory.
pub const DB_FILE: &str = "tracker.db";

/// Schema migrations, in order; `PRAGMA user_version` counts those applied.
/// A schema change is a new file appended here, never an edit of a published one.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_tracker_state.sql"),
    include_str!("../migrations/0002_project_history.sql"),
];

/// One MR to save, as the app holds it.
#[derive(Debug, Clone)]
pub struct MrRow {
    pub id: String,
    pub flagged: bool,
    pub snooze: Option<gitlab_tracker_gitlab::snooze::Snooze>,
    pub linked_ticket: Option<gitlab_tracker_core::LinkedTicket>,
    /// The tracked branches it reached; `None` while its status tells nothing
    /// (loading, failed): the stored ones are kept.
    pub found_branches: Option<HashSet<String>>,
    /// `Unknown` (not read yet this session) keeps the stored one.
    pub mergeability: MergeabilityStatus,
    pub data: MrData,
}

impl From<SavedMr> for MrRow {
    fn from(saved: SavedMr) -> Self {
        Self {
            id: saved.id,
            flagged: saved.flagged,
            snooze: saved.snooze,
            linked_ticket: saved.linked_ticket,
            found_branches: Some(saved.found_branches),
            mergeability: MergeabilityStatus::Unknown,
            data: saved.data,
        }
    }
}

/// The project-wide part of the state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectState {
    /// By MR id, the tracked branches it was on.
    pub last_known_branches: HashMap<String, HashSet<String>>,
    pub discovery_started_at: Option<String>,
    pub dismissed_mr_ids: HashSet<String>,
}

/// What an import wrote, for the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub mrs: usize,
    pub events: usize,
}

/// An MR as stored: compared with the next save to tell what changed.
#[derive(Debug, Clone, PartialEq)]
struct Stored {
    position: Option<i64>,
    fetched: bool,
    flagged: bool,
    snooze: Option<String>,
    ticket: Option<String>,
    /// JSON array, sorted.
    found: String,
    mergeability: Option<String>,
    /// JSON `MrData`.
    data: String,
}

/// A change to append to `mr_events`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Event {
    pub kind: &'static str,
    pub at: Option<String>,
    pub actor: Option<String>,
    pub subject: Option<String>,
    pub old: Option<String>,
    pub new: Option<String>,
}

impl Event {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }
    fn at(mut self, at: Option<&str>) -> Self {
        self.at = at.map(str::to_string);
        self
    }
    fn actor(mut self, actor: Option<&str>) -> Self {
        self.actor = actor.filter(|a| !a.is_empty()).map(str::to_string);
        self
    }
    fn subject(mut self, subject: impl ToString) -> Self {
        self.subject = Some(subject.to_string());
        self
    }
    fn change(mut self, old: Option<&str>, new: Option<&str>) -> Self {
        self.old = old.map(str::to_string);
        self.new = new.map(str::to_string);
        self
    }
}

pub struct StateDb {
    pool: SqlitePool,
    /// The stored MRs per project (its key), read once per project.
    stored: tokio::sync::Mutex<HashMap<i64, HashMap<String, Stored>>>,
}

#[cfg(not(any(test, feature = "test-support")))]
static OPEN: std::sync::LazyLock<std::sync::Mutex<HashMap<std::path::PathBuf, Arc<StateDb>>>> =
    std::sync::LazyLock::new(Default::default);

/// The database of the config directory `dir`, opened (and created) on first use
/// and shared afterwards. Under test (or the `test-support` feature, for the
/// app's tests), a fresh one each time: every test has its own runtime, which a
/// shared pool would outlive.
pub async fn open_in(dir: &Path) -> anyhow::Result<Arc<StateDb>> {
    let path = dir.join(DB_FILE);
    #[cfg(not(any(test, feature = "test-support")))]
    if let Some(db) = OPEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&path)
    {
        return Ok(db.clone());
    }
    crate::restrict_to_owner(dir);
    let db = Arc::new(StateDb::open(&path).await?);
    #[cfg(not(any(test, feature = "test-support")))]
    let db = OPEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(path)
        .or_insert(db)
        .clone();
    Ok(db)
}

impl StateDb {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
        use sqlx::Connection as _;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        // Migrations on a connection of their own, before the pool: see
        // `SqliteStatsDb::open` (a pooled connection with the schema of before an
        // `ALTER TABLE … ADD COLUMN` makes sqlx panic on its rows).
        let mut migrating = sqlx::SqliteConnection::connect_with(&options).await?;
        run_migrations(&mut migrating).await?;
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::Connection::close(migrating).await?;
        Ok(Self {
            pool,
            stored: Default::default(),
        })
    }

    /// The project's key, `None` when it was never saved here.
    async fn project_key(&self, gitlab_url: &str, project_id: &str) -> anyhow::Result<Option<i64>> {
        Ok(
            sqlx::query_scalar("SELECT id FROM projects WHERE gitlab_url = ? AND project_id = ?")
                .bind(gitlab_url)
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// The project's key when its tracked MRs were saved here (or imported):
    /// `None` for one never saved, or with only its caches here.
    async fn state_key(&self, gitlab_url: &str, project_id: &str) -> anyhow::Result<Option<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM projects
             WHERE gitlab_url = ? AND project_id = ? AND state_saved_at IS NOT NULL",
        )
        .bind(gitlab_url)
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Whether the project has its tracked MRs here (saved or imported).
    pub async fn has_project(&self, gitlab_url: &str, project_id: &str) -> anyhow::Result<bool> {
        Ok(self.state_key(gitlab_url, project_id).await?.is_some())
    }
}

async fn run_migrations(conn: &mut sqlx::SqliteConnection) -> anyhow::Result<()> {
    let applied: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *conn)
        .await?;
    // A newer version wrote it (the user went back to an older binary): its
    // schema is unknown here, and writing to it could lose data.
    if applied > MIGRATIONS.len() as i64 {
        anyhow::bail!(
            "tracker.db was written by a newer version of gitlab-tracker (schema {applied}, \
             this version knows {}): update gitlab-tracker",
            MIGRATIONS.len()
        );
    }
    for (version, sql) in MIGRATIONS.iter().enumerate().skip(applied.max(0) as usize) {
        let mut tx = sqlx::Connection::begin(&mut *conn).await?;
        // A migration file of this crate (`include_str!`): a literal.
        sqlx::raw_sql(*sql).execute(&mut *tx).await?;
        // PRAGMA arguments cannot be bound: the value is our own integer.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {}",
            version + 1
        )))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    }
    Ok(())
}

/// A serde enum's name (`"merged"`, `"NeedsRebase"`).
fn name_of<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What `mr` stores, what was stored kept where it says nothing new.
fn stored_row(mr: &MrRow, position: i64, old: Option<&Stored>) -> Stored {
    let fetched = !mr.data.web_url.is_empty();
    // The placeholder of an MR being read again says nothing: keep what was read.
    let (data, fetched) = match old {
        Some(old) if old.fetched && !fetched => (old.data.clone(), true),
        _ => (
            serde_json::to_string(&mr.data).unwrap_or_else(|_| "{}".into()),
            fetched,
        ),
    };
    let found = match &mr.found_branches {
        Some(set) => serde_json::to_string(&set.iter().collect::<BTreeSet<_>>())
            .unwrap_or_else(|_| "[]".into()),
        None => old.map_or_else(|| "[]".into(), |o| o.found.clone()),
    };
    let mergeability = match mr.mergeability {
        MergeabilityStatus::Unknown => old.and_then(|o| o.mergeability.clone()),
        m => Some(name_of(&m)),
    };
    Stored {
        position: Some(position),
        fetched,
        flagged: mr.flagged,
        snooze: mr
            .snooze
            .as_ref()
            .and_then(|s| serde_json::to_string(s).ok()),
        ticket: mr
            .linked_ticket
            .as_ref()
            .and_then(|t| serde_json::to_string(t).ok()),
        found,
        mergeability,
        data,
    }
}

#[cfg(test)]
mod tests;
