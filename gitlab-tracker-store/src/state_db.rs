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

    /// The project's tracked MRs, in dashboard order, and its project-wide state;
    /// `None` when the project was never saved here.
    pub async fn load(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<Option<(Vec<SavedMr>, ProjectState)>> {
        let Some(key) = self.state_key(gitlab_url, project_id).await? else {
            return Ok(None);
        };
        let stored = self.read_stored(key).await?;
        let mut tracked: Vec<(&String, &Stored)> = stored
            .iter()
            .filter(|(_, s)| s.position.is_some())
            .collect();
        tracked.sort_by_key(|(_, s)| s.position);
        let mut mrs = Vec::with_capacity(tracked.len());
        for (id, s) in tracked {
            let data: MrData = match serde_json::from_str(&s.data) {
                Ok(data) => data,
                Err(e) => {
                    tracing::error!(mr = %id, error = %e, "Unreadable MR in tracker.db: skipped");
                    continue;
                }
            };
            mrs.push(SavedMr {
                id: id.clone(),
                found_branches: serde_json::from_str(&s.found).unwrap_or_default(),
                flagged: s.flagged,
                snooze: s
                    .snooze
                    .as_deref()
                    .and_then(|j| serde_json::from_str(j).ok()),
                linked_ticket: s
                    .ticket
                    .as_deref()
                    .and_then(|j| serde_json::from_str(j).ok()),
                data,
            });
        }

        let discovery_started_at: Option<String> =
            sqlx::query_scalar("SELECT discovery_started_at FROM projects WHERE id = ?")
                .bind(key)
                .fetch_one(&self.pool)
                .await?;
        let mut last_known_branches: HashMap<String, HashSet<String>> = HashMap::new();
        for row in sqlx::query("SELECT mr_id, branches FROM known_branches WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            let branches: String = row.try_get("branches")?;
            last_known_branches.insert(
                row.try_get("mr_id")?,
                serde_json::from_str(&branches).unwrap_or_default(),
            );
        }
        let dismissed_mr_ids: HashSet<String> =
            sqlx::query_scalar("SELECT mr_id FROM dismissed_mrs WHERE project = ?")
                .bind(key)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .collect();
        self.stored.lock().await.insert(key, stored);
        Ok(Some((
            mrs,
            ProjectState {
                last_known_branches,
                discovery_started_at,
                dismissed_mr_ids,
            },
        )))
    }

    /// Every stored MR of the project, tracked or not.
    async fn read_stored(&self, key: i64) -> anyhow::Result<HashMap<String, Stored>> {
        let rows = sqlx::query(
            "SELECT mr_id, position, fetched, flagged, snooze, linked_ticket, found_branches,
                    mergeability, data
             FROM mrs WHERE project = ?",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?;
        let mut stored = HashMap::with_capacity(rows.len());
        for row in rows {
            stored.insert(
                row.try_get("mr_id")?,
                Stored {
                    position: row.try_get("position")?,
                    fetched: row.try_get("fetched")?,
                    flagged: row.try_get("flagged")?,
                    snooze: row.try_get("snooze")?,
                    ticket: row.try_get("linked_ticket")?,
                    found: row.try_get("found_branches")?,
                    mergeability: row.try_get("mergeability")?,
                    data: row.try_get("data")?,
                },
            );
        }
        Ok(stored)
    }

    /// Saves the project's tracked MRs (in dashboard order) and its state, and
    /// appends what changed since the last save. Returns the number of events.
    pub async fn save(
        &self,
        gitlab_url: &str,
        project_id: &str,
        mrs: &[MrRow],
        project: &ProjectState,
        now: &str,
    ) -> anyhow::Result<usize> {
        self.save_inner(gitlab_url, project_id, mrs, project, now, false)
            .await
    }

    /// Imports a `tracker_<hash>.json` state, marking the project imported.
    pub async fn import(
        &self,
        gitlab_url: &str,
        project_id: &str,
        state: SavedState,
        now: &str,
    ) -> anyhow::Result<ImportReport> {
        let project = ProjectState {
            last_known_branches: state.last_known_branches,
            discovery_started_at: state.discovery_started_at,
            dismissed_mr_ids: state.dismissed_mr_ids,
        };
        let rows: Vec<MrRow> = state.mrs.into_iter().map(MrRow::from).collect();
        let events = self
            .save_inner(gitlab_url, project_id, &rows, &project, now, true)
            .await?;
        Ok(ImportReport {
            mrs: rows.len(),
            events,
        })
    }

    async fn save_inner(
        &self,
        gitlab_url: &str,
        project_id: &str,
        mrs: &[MrRow],
        project: &ProjectState,
        now: &str,
        import: bool,
    ) -> anyhow::Result<usize> {
        // One save at a time: each compares with what the previous one stored.
        let mut cache = self.stored.lock().await;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO projects (gitlab_url, project_id) VALUES (?, ?)
             ON CONFLICT (gitlab_url, project_id) DO NOTHING",
        )
        .bind(gitlab_url)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
        let key: i64 =
            sqlx::query_scalar("SELECT id FROM projects WHERE gitlab_url = ? AND project_id = ?")
                .bind(gitlab_url)
                .bind(project_id)
                .fetch_one(&mut *tx)
                .await?;
        // Taken out while saving: a failed save leaves it to be read again.
        let mut stored = match cache.remove(&key) {
            Some(stored) => stored,
            None => self.read_stored(key).await?,
        };
        let mut events = 0;

        let mut seen = HashSet::new();
        for (position, mr) in mrs.iter().enumerate() {
            if !seen.insert(mr.id.as_str()) {
                continue;
            }
            let old = stored.get(&mr.id);
            let new = stored_row(mr, position as i64, old);
            if old == Some(&new) {
                continue;
            }
            let list = mr_events(old, &new, mr);
            let data_changed = old.is_none_or(|o| o.data != new.data);
            write_mr(&mut tx, key, &mr.id, &new, now).await?;
            if data_changed {
                write_details(&mut tx, key, &mr.id, &new).await?;
            }
            events += list.len();
            write_events(&mut tx, key, &mr.id, &list, now).await?;
            stored.insert(mr.id.clone(), new);
        }
        // Off the dashboard: the row stays, out of it.
        for (id, s) in stored.iter_mut() {
            if s.position.is_some() && !seen.contains(id.as_str()) {
                sqlx::query(
                    "UPDATE mrs SET position = NULL, saved_at = ? WHERE project = ? AND mr_id = ?",
                )
                .bind(now)
                .bind(key)
                .bind(id)
                .execute(&mut *tx)
                .await?;
                write_events(&mut tx, key, id, &[Event::new("untracked")], now).await?;
                events += 1;
                s.position = None;
            }
        }

        sqlx::query(
            "UPDATE projects SET discovery_started_at = ?, state_saved_at = ? WHERE id = ?",
        )
        .bind(&project.discovery_started_at)
        .bind(now)
        .bind(key)
        .execute(&mut *tx)
        .await?;
        if import {
            sqlx::query("UPDATE projects SET imported_at = ? WHERE id = ?")
                .bind(now)
                .bind(key)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM known_branches WHERE project = ?")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        for (id, branches) in &project.last_known_branches {
            let branches = serde_json::to_string(&branches.iter().collect::<BTreeSet<_>>())?;
            sqlx::query("INSERT INTO known_branches (project, mr_id, branches) VALUES (?, ?, ?)")
                .bind(key)
                .bind(id)
                .bind(branches)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM dismissed_mrs WHERE project = ?")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        for id in &project.dismissed_mr_ids {
            sqlx::query("INSERT INTO dismissed_mrs (project, mr_id) VALUES (?, ?)")
                .bind(key)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        cache.insert(key, stored);
        Ok(events)
    }

    /// The events of an MR, oldest seen first: for tests and the import report.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn events_of(
        &self,
        gitlab_url: &str,
        project_id: &str,
        mr_id: &str,
    ) -> anyhow::Result<Vec<Event>> {
        let Some(key) = self.project_key(gitlab_url, project_id).await? else {
            return Ok(vec![]);
        };
        let rows = sqlx::query(
            "SELECT kind, at, actor, subject, old_value, new_value FROM mr_events
             WHERE project = ? AND mr_id = ? ORDER BY id",
        )
        .bind(key)
        .bind(mr_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let kind: String = row.try_get("kind")?;
                Ok(Event {
                    kind: Box::leak(kind.into_boxed_str()),
                    at: row.try_get("at")?,
                    actor: row.try_get("actor")?,
                    subject: row.try_get("subject")?,
                    old: row.try_get("old_value")?,
                    new: row.try_get("new_value")?,
                })
            })
            .collect()
    }
}

/// The project caches (the merges, the merged MRs' owners) and the settings:
/// the window's.
impl StateDb {
    /// The project's key, its row created when absent.
    async fn project_key_or_insert(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<i64> {
        sqlx::query(
            "INSERT INTO projects (gitlab_url, project_id) VALUES (?, ?)
             ON CONFLICT (gitlab_url, project_id) DO NOTHING",
        )
        .bind(gitlab_url)
        .bind(project_id)
        .execute(&self.pool)
        .await?;
        self.project_key(gitlab_url, project_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("project row missing"))
    }

    /// Up to when the project's read `what` is complete: `None` when it was
    /// never saved here.
    async fn read_at(&self, key: i64, what: &str) -> anyhow::Result<Option<Option<String>>> {
        Ok(
            sqlx::query_scalar("SELECT read_at FROM project_reads WHERE project = ? AND what = ?")
                .bind(key)
                .bind(what)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn set_read_at(
        tx: &mut sqlx::SqliteConnection,
        key: i64,
        what: &str,
        read_at: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO project_reads (project, what, read_at) VALUES (?, ?, ?)
             ON CONFLICT (project, what) DO UPDATE SET read_at = excluded.read_at",
        )
        .bind(key)
        .bind(what)
        .bind(read_at)
        .execute(&mut *tx)
        .await?;
        Ok(())
    }

    /// A setting's JSON value, `None` when never saved.
    pub async fn setting(&self, name: &str) -> anyhow::Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM settings WHERE name = ?")
                .bind(name)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn set_setting(&self, name: &str, value: &str) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO settings (name, value) VALUES (?, ?)
             ON CONFLICT (name) DO UPDATE SET value = excluded.value",
        )
        .bind(name)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every merge of the project saved here, over any period; `None` when the
    /// merges were never saved.
    pub async fn load_merges(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<Option<gitlab_tracker_gitlab::history::MergeHistory>> {
        let Some(key) = self.project_key(gitlab_url, project_id).await? else {
            return Ok(None);
        };
        let Some(read_at) = self.read_at(key, "merges").await? else {
            return Ok(None);
        };
        let mut merged = BTreeMap::new();
        for row in sqlx::query("SELECT mr_iid, merged_at FROM project_merges WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            merged.insert(iid as u64, row.try_get("merged_at")?);
        }
        Ok(Some(gitlab_tracker_gitlab::history::MergeHistory {
            merged,
            read_at,
        }))
    }

    /// Saves the merges of `history` not in `known` (as they were before the
    /// read), and up to when it is complete. Merges out of the period stay.
    pub async fn save_merges(
        &self,
        gitlab_url: &str,
        project_id: &str,
        history: &gitlab_tracker_gitlab::history::MergeHistory,
        known: &gitlab_tracker_gitlab::history::MergeHistory,
    ) -> anyhow::Result<()> {
        let key = self.project_key_or_insert(gitlab_url, project_id).await?;
        let mut tx = self.pool.begin().await?;
        for (iid, at) in &history.merged {
            if known.merged.get(iid) == Some(at) {
                continue;
            }
            sqlx::query(
                "INSERT INTO project_merges (project, mr_iid, merged_at) VALUES (?, ?, ?)
                 ON CONFLICT (project, mr_iid) DO UPDATE SET merged_at = excluded.merged_at",
            )
            .bind(key)
            .bind(*iid as i64)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        }
        Self::set_read_at(&mut tx, key, "merges", history.read_at.as_deref()).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Every merged MR of the project saved here, with its people and files,
    /// over any period; `None` when they were never saved.
    pub async fn load_owners(
        &self,
        gitlab_url: &str,
        project_id: &str,
    ) -> anyhow::Result<Option<gitlab_tracker_gitlab::history::History>> {
        use gitlab_tracker_gitlab::history::{History, Merge};
        let Some(key) = self.project_key(gitlab_url, project_id).await? else {
            return Ok(None);
        };
        let Some(read_at) = self.read_at(key, "owners").await? else {
            return Ok(None);
        };
        let mut merges: BTreeMap<u64, Merge> = BTreeMap::new();
        for row in sqlx::query("SELECT mr_iid, merged_at, author FROM merged_mrs WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            merges.insert(
                iid as u64,
                Merge {
                    merged_at: row.try_get("merged_at")?,
                    author: row.try_get("author")?,
                    reviewers: Vec::new(),
                    approvers: Vec::new(),
                    paths: Vec::new(),
                },
            );
        }
        for row in sqlx::query(
            "SELECT mr_iid, role, login FROM merged_mr_people WHERE project = ?
             ORDER BY mr_iid, role, position",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            let role: String = row.try_get("role")?;
            if let Some(m) = merges.get_mut(&(iid as u64)) {
                let list = if role == "approver" {
                    &mut m.approvers
                } else {
                    &mut m.reviewers
                };
                list.push(row.try_get("login")?);
            }
        }
        for row in sqlx::query(
            "SELECT mr_iid, path FROM merged_mr_paths WHERE project = ? ORDER BY mr_iid, position",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?
        {
            let iid: i64 = row.try_get("mr_iid")?;
            if let Some(m) = merges.get_mut(&(iid as u64)) {
                m.paths.push(row.try_get("path")?);
            }
        }
        let mut people = BTreeMap::new();
        for row in sqlx::query("SELECT login, name FROM people WHERE project = ?")
            .bind(key)
            .fetch_all(&self.pool)
            .await?
        {
            people.insert(row.try_get("login")?, row.try_get("name")?);
        }
        Ok(Some(History {
            merges,
            people,
            read_at,
        }))
    }

    /// Saves the merged MRs and names of `history` that are new or changed
    /// since `known`, and up to when it is complete. Older ones stay.
    pub async fn save_owners(
        &self,
        gitlab_url: &str,
        project_id: &str,
        history: &gitlab_tracker_gitlab::history::History,
        known: &gitlab_tracker_gitlab::history::History,
    ) -> anyhow::Result<()> {
        let key = self.project_key_or_insert(gitlab_url, project_id).await?;
        let mut tx = self.pool.begin().await?;
        for (iid, m) in &history.merges {
            if known.merges.get(iid) == Some(m) {
                continue;
            }
            let iid = *iid as i64;
            sqlx::query(
                "INSERT INTO merged_mrs (project, mr_iid, merged_at, author) VALUES (?, ?, ?, ?)
                 ON CONFLICT (project, mr_iid) DO UPDATE SET
                     merged_at = excluded.merged_at, author = excluded.author",
            )
            .bind(key)
            .bind(iid)
            .bind(&m.merged_at)
            .bind(&m.author)
            .execute(&mut *tx)
            .await?;
            sqlx::query("DELETE FROM merged_mr_people WHERE project = ? AND mr_iid = ?")
                .bind(key)
                .bind(iid)
                .execute(&mut *tx)
                .await?;
            let people = m
                .reviewers
                .iter()
                .enumerate()
                .map(|(i, l)| ("reviewer", i, l))
                .chain(
                    m.approvers
                        .iter()
                        .enumerate()
                        .map(|(i, l)| ("approver", i, l)),
                );
            for (role, position, login) in people {
                sqlx::query(
                    "INSERT INTO merged_mr_people (project, mr_iid, role, position, login)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(key)
                .bind(iid)
                .bind(role)
                .bind(position as i64)
                .bind(login)
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query("DELETE FROM merged_mr_paths WHERE project = ? AND mr_iid = ?")
                .bind(key)
                .bind(iid)
                .execute(&mut *tx)
                .await?;
            for (position, path) in m.paths.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO merged_mr_paths (project, mr_iid, position, path) VALUES (?, ?, ?, ?)",
                )
                .bind(key)
                .bind(iid)
                .bind(position as i64)
                .bind(path)
                .execute(&mut *tx)
                .await?;
            }
        }
        for (login, name) in &history.people {
            if known.people.get(login) == Some(name) {
                continue;
            }
            sqlx::query(
                "INSERT INTO people (project, login, name) VALUES (?, ?, ?)
                 ON CONFLICT (project, login) DO UPDATE SET name = excluded.name",
            )
            .bind(key)
            .bind(login)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        }
        Self::set_read_at(&mut tx, key, "owners", history.read_at.as_deref()).await?;
        tx.commit().await?;
        Ok(())
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

/// What changed between the stored MR and the new one.
fn mr_events(old: Option<&Stored>, new: &Stored, mr: &MrRow) -> Vec<Event> {
    let mut list = Vec::new();
    if old.is_none_or(|o| o.position.is_none()) {
        list.push(Event::new("tracked"));
    }
    let new_data: MrData = serde_json::from_str(&new.data).unwrap_or_default();
    let old_data: Option<MrData> = old
        .filter(|o| o.fetched && o.data != new.data)
        .and_then(|o| serde_json::from_str(&o.data).ok());
    match (&old_data, old) {
        (Some(old_data), _) => list.extend(data_events(old_data, &new_data)),
        // First real read (new, or after its placeholder): the history it tells.
        (None, o) if new.fetched && o.is_none_or(|o| !o.fetched) => {
            list.extend(history_of(&new_data))
        }
        _ => {}
    }

    let branches =
        |json: &str| -> BTreeSet<String> { serde_json::from_str(json).unwrap_or_default() };
    let before = old.map(|o| branches(&o.found)).unwrap_or_default();
    let after = branches(&new.found);
    for b in after.difference(&before) {
        list.push(Event::new("branch_reached").subject(b));
    }
    for b in before.difference(&after) {
        list.push(Event::new("branch_left").subject(b));
    }

    let old_m = old.and_then(|o| o.mergeability.as_deref());
    if new.mergeability.as_deref() != old_m
        && new_data.state == GitlabMrState::Opened
        && new.mergeability.is_some()
    {
        list.push(Event::new("mergeability").change(old_m, new.mergeability.as_deref()));
    }
    if old.is_some_and(|o| o.flagged != new.flagged) || (old.is_none() && new.flagged) {
        list.push(Event::new(if new.flagged {
            "flagged"
        } else {
            "unflagged"
        }));
    }
    if old.map(|o| o.snooze.is_some()).unwrap_or(false) != new.snooze.is_some() {
        list.push(Event::new(if new.snooze.is_some() {
            "snoozed"
        } else {
            "unsnoozed"
        }));
    }
    let old_ticket: Option<gitlab_tracker_core::LinkedTicket> = old
        .and_then(|o| o.ticket.as_deref())
        .and_then(|j| serde_json::from_str(j).ok());
    match (&old_ticket, &mr.linked_ticket) {
        (Some(a), Some(b)) if a.id == b.id && a.status != b.status => list.push(
            Event::new("ticket_status")
                .subject(&b.id)
                .change(Some(&a.status), Some(&b.status)),
        ),
        (a, Some(b)) if a.as_ref().map(|a| &a.id) != Some(&b.id) => list.push(
            Event::new("ticket_linked")
                .subject(&b.id)
                .change(a.as_ref().map(|a| a.id.as_str()), Some(&b.id)),
        ),
        (Some(a), None) => list.push(Event::new("ticket_unlinked").subject(&a.id)),
        _ => {}
    }
    list
}

/// The history an MR's data tells on its own, dated by GitLab: for an MR seen
/// for the first time (or imported).
pub fn history_of(d: &MrData) -> Vec<Event> {
    let mut list = vec![Event::new("opened")
        .at(d.created_at.as_deref())
        .actor(Some(&d.author))];
    if let Some(r) = &d.review {
        if let Some(at) = &r.ready_at {
            list.push(Event::new("ready").at(Some(at)));
        }
        for (login, at) in &r.review_requested_at {
            list.push(Event::new("review_requested").at(Some(at)).subject(login));
        }
        if let Some(at) = &r.first_review_at {
            list.push(Event::new("first_review").at(Some(at)));
        }
        if let Some(at) = &r.last_push_at {
            list.push(Event::new("pushed").at(Some(at)));
        }
        for (login, at) in &r.approvals {
            list.push(Event::new("approved").at(Some(at)).actor(Some(login)));
        }
    }
    // Approvals read from GitLab with no date (older reads).
    for login in &d.approved_by {
        if !d
            .review
            .as_ref()
            .is_some_and(|r| r.approvals.contains_key(login))
        {
            list.push(Event::new("approved").actor(Some(login)));
        }
    }
    for e in d.milestone_events.iter().flatten() {
        list.push(
            Event::new(if e.added {
                "milestone_set"
            } else {
                "milestone_removed"
            })
            .at(Some(&e.at))
            .actor(e.by.as_deref())
            .subject(e.milestone.as_deref().unwrap_or_default()),
        );
    }
    for p in &d.pipelines {
        list.push(
            Event::new("pipeline")
                .at(p.created_at.as_deref())
                .subject(p.id)
                .change(None, Some(&name_of(&p.status))),
        );
    }
    for t in &d.threads {
        list.push(
            Event::new("thread_opened")
                .at(Some(&t.created_at))
                .actor(Some(&t.author))
                .subject(t.note_id)
                .change(None, Some(&t.excerpt)),
        );
        if t.resolved {
            list.push(Event::new("thread_resolved").subject(t.note_id));
        }
    }
    match d.state {
        GitlabMrState::Merged => list.push(
            Event::new("merged")
                .at(d.merged_at.as_deref())
                .actor(d.merged_by.as_deref()),
        ),
        GitlabMrState::Closed => list.push(Event::new("closed").at(d.updated_at.as_deref())),
        _ => {}
    }
    list
}

/// What changed between two reads of an MR.
pub fn data_events(old: &MrData, new: &MrData) -> Vec<Event> {
    let mut list = Vec::new();
    let mut field = |kind: &'static str, a: Option<&str>, b: Option<&str>| {
        if a != b {
            list.push(Event::new(kind).change(a, b));
        }
    };
    field("title", Some(&old.title), Some(&new.title));
    field(
        "description",
        Some(&old.description),
        Some(&new.description),
    );
    field("assignee", old.assignee.as_deref(), new.assignee.as_deref());
    field(
        "milestone",
        old.milestone.as_deref(),
        new.milestone.as_deref(),
    );
    field(
        "target_branch",
        Some(&old.target_branch),
        Some(&new.target_branch),
    );
    field("head", old.sha.as_deref(), new.sha.as_deref());
    if old.auto_merge != new.auto_merge {
        list.push(Event::new(if new.auto_merge {
            "auto_merge_set"
        } else {
            "auto_merge_cancelled"
        }));
    }
    if old.state != new.state {
        let (kind, at, actor) = match new.state {
            GitlabMrState::Merged => ("merged", new.merged_at.as_deref(), new.merged_by.as_deref()),
            GitlabMrState::Closed => ("closed", new.updated_at.as_deref(), None),
            _ => ("reopened", new.updated_at.as_deref(), None),
        };
        list.push(
            Event::new(kind)
                .at(at)
                .actor(actor)
                .change(Some(&name_of(&old.state)), Some(&name_of(&new.state))),
        );
    }

    let set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
    let (a, b) = (set(&old.labels), set(&new.labels));
    for l in b.difference(&a) {
        list.push(Event::new("label_added").subject(l));
    }
    for l in a.difference(&b) {
        list.push(Event::new("label_removed").subject(l));
    }
    let (a, b) = (set(&old.reviewers), set(&new.reviewers));
    for l in b.difference(&a) {
        list.push(Event::new("reviewer_added").subject(l));
    }
    for l in a.difference(&b) {
        list.push(Event::new("reviewer_removed").subject(l));
    }
    let approved_at = |login: &str| {
        new.review
            .as_ref()
            .and_then(|r| r.approvals.get(login))
            .map(String::as_str)
    };
    let (a, b) = (set(&old.approved_by), set(&new.approved_by));
    for l in b.difference(&a) {
        list.push(Event::new("approved").at(approved_at(l)).actor(Some(l)));
    }
    for l in a.difference(&b) {
        list.push(Event::new("unapproved").actor(Some(l)));
    }

    let empty = Default::default();
    let (ra, rb) = (
        old.review.as_ref().unwrap_or(&empty),
        new.review.as_ref().unwrap_or(&empty),
    );
    for (login, at) in &rb.review_requested_at {
        if ra.review_requested_at.get(login) != Some(at) {
            list.push(Event::new("review_requested").at(Some(at)).subject(login));
        }
    }
    if ra.first_review_at.is_none() && rb.first_review_at.is_some() {
        list.push(Event::new("first_review").at(rb.first_review_at.as_deref()));
    }
    if rb.ready_at.is_some() && ra.ready_at != rb.ready_at {
        list.push(Event::new("ready").at(rb.ready_at.as_deref()));
    }
    if rb.last_push_at.is_some() && ra.last_push_at != rb.last_push_at {
        list.push(Event::new("pushed").at(rb.last_push_at.as_deref()));
    }

    let pipelines: BTreeMap<u64, &gitlab_tracker_gitlab::models::Pipeline> =
        old.pipelines.iter().map(|p| (p.id, p)).collect();
    for p in &new.pipelines {
        let status = name_of(&p.status);
        match pipelines.get(&p.id) {
            None => list.push(
                Event::new("pipeline")
                    .at(p.created_at.as_deref())
                    .subject(p.id)
                    .change(None, Some(&status)),
            ),
            Some(before) if before.status != p.status => list.push(
                Event::new("pipeline")
                    .at(p.updated_at.as_deref())
                    .subject(p.id)
                    .change(Some(&name_of(&before.status)), Some(&status)),
            ),
            _ => {}
        }
    }

    let threads: BTreeMap<u64, &gitlab_tracker_gitlab::models::MrThread> =
        old.threads.iter().map(|t| (t.note_id, t)).collect();
    for t in &new.threads {
        match threads.get(&t.note_id) {
            None => list.push(
                Event::new("thread_opened")
                    .at(Some(&t.created_at))
                    .actor(Some(&t.author))
                    .subject(t.note_id)
                    .change(None, Some(&t.excerpt)),
            ),
            Some(before) => {
                if t.notes > before.notes {
                    list.push(
                        Event::new("thread_replied")
                            .at(Some(&t.last_note_at))
                            .subject(t.note_id)
                            .change(Some(&before.notes.to_string()), Some(&t.notes.to_string())),
                    );
                }
                if t.resolved != before.resolved {
                    list.push(
                        Event::new(if t.resolved {
                            "thread_resolved"
                        } else {
                            "thread_reopened"
                        })
                        .at(Some(&t.last_note_at))
                        .subject(t.note_id),
                    );
                }
            }
        }
    }

    let size = |d: &MrData| {
        d.diff_stats.as_ref().map(|s| {
            format!(
                "+{} -{} · {} files · {} commits",
                s.additions, s.deletions, s.files_changed, s.commits_count
            )
        })
    };
    if let (Some(a), Some(b)) = (size(old), size(new)) {
        if a != b {
            list.push(Event::new("size").change(Some(&a), Some(&b)));
        }
    }

    let known: BTreeSet<(&str, bool)> = old
        .milestone_events
        .iter()
        .flatten()
        .map(|e| (e.at.as_str(), e.added))
        .collect();
    for e in new.milestone_events.iter().flatten() {
        if !known.contains(&(e.at.as_str(), e.added)) {
            list.push(
                Event::new(if e.added {
                    "milestone_set"
                } else {
                    "milestone_removed"
                })
                .at(Some(&e.at))
                .actor(e.by.as_deref())
                .subject(e.milestone.as_deref().unwrap_or_default()),
            );
        }
    }
    list
}

async fn write_mr(
    tx: &mut sqlx::SqliteConnection,
    key: i64,
    id: &str,
    s: &Stored,
    now: &str,
) -> anyhow::Result<()> {
    let d: MrData = serde_json::from_str(&s.data).unwrap_or_default();
    sqlx::query(
        "INSERT INTO mrs (project, mr_id, position, fetched, title, state, author, assignee,
             source_branch, target_branch, milestone, web_url, created_at, updated_at,
             merged_at, merged_by, mergeability, flagged, found_branches, snooze,
             linked_ticket, data, first_seen_at, saved_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (project, mr_id) DO UPDATE SET
             position = excluded.position, fetched = excluded.fetched,
             title = excluded.title, state = excluded.state, author = excluded.author,
             assignee = excluded.assignee, source_branch = excluded.source_branch,
             target_branch = excluded.target_branch, milestone = excluded.milestone,
             web_url = excluded.web_url, created_at = excluded.created_at,
             updated_at = excluded.updated_at, merged_at = excluded.merged_at,
             merged_by = excluded.merged_by, mergeability = excluded.mergeability,
             flagged = excluded.flagged, found_branches = excluded.found_branches,
             snooze = excluded.snooze, linked_ticket = excluded.linked_ticket,
             data = excluded.data, saved_at = excluded.saved_at",
    )
    .bind(key)
    .bind(id)
    .bind(s.position)
    .bind(s.fetched)
    .bind(&d.title)
    .bind(name_of(&d.state))
    .bind(&d.author)
    .bind(&d.assignee)
    .bind(&d.source_branch)
    .bind(&d.target_branch)
    .bind(&d.milestone)
    .bind(&d.web_url)
    .bind(&d.created_at)
    .bind(&d.updated_at)
    .bind(&d.merged_at)
    .bind(&d.merged_by)
    .bind(&s.mergeability)
    .bind(s.flagged)
    .bind(&s.found)
    .bind(&s.snooze)
    .bind(&s.ticket)
    .bind(&s.data)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

/// The queryable copies of an MR's data: current labels and people, and every
/// pipeline and thread seen (kept when GitLab's lists drop them).
async fn write_details(
    tx: &mut sqlx::SqliteConnection,
    key: i64,
    id: &str,
    s: &Stored,
) -> anyhow::Result<()> {
    let d: MrData = serde_json::from_str(&s.data).unwrap_or_default();
    sqlx::query("DELETE FROM mr_labels WHERE project = ? AND mr_id = ?")
        .bind(key)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for label in d.labels.iter().collect::<BTreeSet<_>>() {
        sqlx::query("INSERT INTO mr_labels (project, mr_id, label) VALUES (?, ?, ?)")
            .bind(key)
            .bind(id)
            .bind(label)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM mr_people WHERE project = ? AND mr_id = ?")
        .bind(key)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let people = d
        .reviewers
        .iter()
        .map(|l| ("reviewer", l))
        .chain(d.approved_by.iter().map(|l| ("approver", l)))
        .collect::<BTreeSet<_>>();
    for (role, login) in people {
        sqlx::query("INSERT INTO mr_people (project, mr_id, role, login) VALUES (?, ?, ?, ?)")
            .bind(key)
            .bind(id)
            .bind(role)
            .bind(login)
            .execute(&mut *tx)
            .await?;
    }
    for p in &d.pipelines {
        sqlx::query(
            "INSERT INTO mr_pipelines (project, mr_id, pipeline_id, status, sha, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (project, mr_id, pipeline_id) DO UPDATE SET
                 status = excluded.status, sha = excluded.sha,
                 created_at = excluded.created_at, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(id)
        .bind(i64::try_from(p.id).unwrap_or(i64::MAX))
        .bind(name_of(&p.status))
        .bind(&p.sha)
        .bind(&p.created_at)
        .bind(&p.updated_at)
        .execute(&mut *tx)
        .await?;
    }
    for t in &d.threads {
        sqlx::query(
            "INSERT INTO mr_threads (project, mr_id, note_id, author, excerpt, created_at,
                 last_note_at, notes, resolved)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (project, mr_id, note_id) DO UPDATE SET
                 excerpt = excluded.excerpt, last_note_at = excluded.last_note_at,
                 notes = excluded.notes, resolved = excluded.resolved",
        )
        .bind(key)
        .bind(id)
        .bind(i64::try_from(t.note_id).unwrap_or(i64::MAX))
        .bind(&t.author)
        .bind(&t.excerpt)
        .bind(&t.created_at)
        .bind(&t.last_note_at)
        .bind(t.notes)
        .bind(t.resolved)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

async fn write_events(
    tx: &mut sqlx::SqliteConnection,
    key: i64,
    id: &str,
    events: &[Event],
    now: &str,
) -> anyhow::Result<()> {
    for e in events {
        sqlx::query(
            "INSERT INTO mr_events (project, mr_id, seen_at, at, kind, actor, subject,
                 old_value, new_value)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(key)
        .bind(id)
        .bind(now)
        .bind(&e.at)
        .bind(e.kind)
        .bind(&e.actor)
        .bind(&e.subject)
        .bind(&e.old)
        .bind(&e.new)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitlab_tracker_gitlab::models::{MrThread, Pipeline, PipelineState, ReviewActivity};
    use std::path::PathBuf;

    const URL: &str = "https://gitlab.example";

    fn mr(id: &str) -> MrRow {
        MrRow {
            id: id.into(),
            flagged: false,
            snooze: None,
            linked_ticket: None,
            found_branches: Some(HashSet::new()),
            mergeability: MergeabilityStatus::Unknown,
            data: MrData {
                title: format!("MR {id}"),
                author: "ana".into(),
                web_url: format!("https://g/{id}"),
                source_branch: "feature".into(),
                target_branch: "develop".into(),
                created_at: Some("2026-11-16T08:00:00Z".into()),
                ..MrData::default()
            },
        }
    }

    async fn db() -> (StateDb, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "gt-state-db-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (StateDb::open(&dir.join(DB_FILE)).await.unwrap(), dir)
    }

    #[tokio::test]
    async fn a_database_from_a_newer_version_is_left_untouched() {
        let (db, dir) = db().await;
        let newer = MIGRATIONS.len() + 1;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {newer}"
        )))
        .execute(&db.pool)
        .await
        .unwrap();
        drop(db);
        let err = StateDb::open(&dir.join(DB_FILE))
            .await
            .err()
            .expect("refused");
        assert!(err.to_string().contains("newer version"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn kinds(db: &StateDb, id: &str) -> Vec<(&'static str, Option<String>)> {
        db.events_of(URL, "1", id)
            .await
            .unwrap()
            .into_iter()
            .map(|e| (e.kind, e.subject.or(e.actor)))
            .collect()
    }

    #[tokio::test]
    async fn each_save_appends_what_changed_and_nothing_when_nothing_did() {
        let (db, dir) = db().await;
        let state = ProjectState::default();
        let mut a = mr("1");
        db.save(URL, "1", std::slice::from_ref(&a), &state, "d1")
            .await
            .unwrap();
        assert_eq!(
            kinds(&db, "1").await,
            [("tracked", None), ("opened", Some("ana".into()))]
        );

        // Nothing new: no row, no event.
        assert_eq!(
            db.save(URL, "1", std::slice::from_ref(&a), &state, "d2")
                .await
                .unwrap(),
            0
        );

        // The next day: reviewed, approved, a pipeline, a thread, a label; then merged.
        a.data.labels = vec!["bug".into()];
        a.data.approved_by = vec!["bob".into()];
        a.data.review = Some(ReviewActivity {
            approvals: [("bob".to_string(), "2026-11-17T09:00:00Z".to_string())].into(),
            ..ReviewActivity::default()
        });
        a.data.pipelines = vec![Pipeline {
            id: 7,
            status: PipelineState::Running,
            ..Pipeline::default()
        }];
        a.data.threads = vec![MrThread {
            note_id: 70,
            author: "bob".into(),
            excerpt: "why?".into(),
            created_at: "2026-11-17T08:00:00Z".into(),
            last_note_at: "2026-11-17T08:00:00Z".into(),
            notes: 1,
            resolved: false,
        }];
        a.mergeability = MergeabilityStatus::Mergeable;
        db.save(URL, "1", std::slice::from_ref(&a), &state, "d3")
            .await
            .unwrap();
        let events = db.events_of(URL, "1", "1").await.unwrap();
        let approved = events.iter().find(|e| e.kind == "approved").unwrap();
        assert_eq!(approved.at.as_deref(), Some("2026-11-17T09:00:00Z"));
        assert_eq!(
            kinds(&db, "1").await[2..],
            [
                ("label_added", Some("bug".into())),
                ("approved", Some("bob".into())),
                ("pipeline", Some("7".into())),
                ("thread_opened", Some("70".into())),
                ("mergeability", None),
            ]
        );

        a.data.state = GitlabMrState::Merged;
        a.data.merged_by = Some("bob".into());
        a.data.pipelines[0].status = PipelineState::Success;
        a.data.threads[0].resolved = true;
        a.data.pipelines.clear(); // GitLab's list moves on: the table keeps it
        a.found_branches = Some(["develop".to_string()].into());
        db.save(URL, "1", std::slice::from_ref(&a), &state, "d4")
            .await
            .unwrap();
        assert_eq!(
            kinds(&db, "1").await[7..],
            [
                ("merged", Some("bob".into())),
                ("thread_resolved", Some("70".into())),
                ("branch_reached", Some("develop".into())),
            ]
        );
        let pipelines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mr_pipelines")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(pipelines, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_loading_placeholder_keeps_what_was_read_and_removal_keeps_the_history() {
        let (db, dir) = db().await;
        let state = ProjectState::default();
        let mut a = mr("1");
        a.found_branches = Some(["develop".to_string()].into());
        a.data.approved_by = vec!["bob".into()];
        db.save(URL, "1", &[a.clone(), mr("2")], &state, "d1")
            .await
            .unwrap();

        // Restarting: read again, shown as a placeholder, its status loading.
        let placeholder = MrRow {
            found_branches: None,
            data: gitlab_tracker_gitlab::models::TrackedMr::placeholder(
                "1".into(),
                "MR 1".into(),
                None,
            )
            .data,
            ..a.clone()
        };
        assert_eq!(
            db.save(URL, "1", &[placeholder, mr("2")], &state, "d2")
                .await
                .unwrap(),
            0
        );
        let (mrs, _) = db.load(URL, "1").await.unwrap().unwrap();
        assert_eq!(mrs[0].data.approved_by, ["bob"]);
        assert!(mrs[0].found_branches.contains("develop"));

        // Removed from the dashboard: not loaded, its row and history kept;
        // added back, it comes back with them.
        db.save(URL, "1", &[mr("2")], &state, "d3").await.unwrap();
        let (mrs, _) = db.load(URL, "1").await.unwrap().unwrap();
        assert_eq!(mrs.len(), 1);
        assert_eq!(kinds(&db, "1").await.last().unwrap().0, "untracked");
        db.save(URL, "1", &[mr("2"), a], &state, "d4")
            .await
            .unwrap();
        let (mrs, _) = db.load(URL, "1").await.unwrap().unwrap();
        let ids: Vec<&str> = mrs.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["2", "1"]);
        assert_eq!(kinds(&db, "1").await.last().unwrap().0, "tracked");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn projects_are_kept_apart() {
        let (db, dir) = db().await;
        let one = ProjectState {
            discovery_started_at: Some("2026-01-01".into()),
            dismissed_mr_ids: ["9".to_string()].into(),
            // By MR: its branches, none for one known on none (kept: not new).
            last_known_branches: [
                ("1".to_string(), ["main".to_string()].into()),
                ("4".to_string(), HashSet::new()),
            ]
            .into(),
        };
        db.save(URL, "1", &[mr("1")], &one, "d1").await.unwrap();
        db.save(URL, "2", &[mr("5")], &ProjectState::default(), "d1")
            .await
            .unwrap();
        let (mrs, state) = db.load(URL, "1").await.unwrap().unwrap();
        assert_eq!((mrs.len(), mrs[0].id.as_str()), (1, "1"));
        assert_eq!(state, one);
        assert!(db.load(URL, "3").await.unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
