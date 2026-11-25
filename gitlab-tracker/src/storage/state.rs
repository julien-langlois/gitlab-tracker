//! The tracked MRs' state: its file name, migration, import, and the background writer.

use super::*;

pub(super) fn tracker_state_file_name(gitlab_url: &str, project_id: &str) -> String {
    // FNV-1a 32-bit constants.
    const FNV_OFFSET: u32 = 2_166_136_261;
    const FNV_PRIME: u32 = 16_777_619;

    let mut hash = FNV_OFFSET;
    // Normalise the URL so trailing slashes do not produce a different hash.
    let url = gitlab_url.trim_end_matches('/');
    for byte in url
        .bytes()
        .chain(b"|".iter().copied())
        .chain(project_id.bytes())
    {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("tracker_{:08x}.json", hash)
}

/// Silently migrates the legacy `tracker_state.json` file to the tenant-scoped
/// name when the new file does not yet exist.
///
/// This is a one-time, zero-friction operation: the user never sees a prompt and
/// no data is lost. If both files already exist (e.g. two different projects were
/// used before this version) the legacy file is left untouched so the user can
/// review it manually.
pub(super) async fn migrate_tracker_state_file(
    config_dir: &std::path::Path,
    gitlab_url: &str,
    project_id: &str,
) {
    let legacy = config_dir.join("tracker_state.json");
    let target_name = tracker_state_file_name(gitlab_url, project_id);
    let target = config_dir.join(&target_name);

    // Only migrate when the legacy file exists and the new file does not.
    let legacy_exists = tokio::fs::try_exists(&legacy).await.unwrap_or(false);
    let target_exists = tokio::fs::try_exists(&target).await.unwrap_or(false);
    if !legacy_exists || target_exists {
        return;
    }

    match tokio::fs::rename(&legacy, &target).await {
        Ok(_) => {
            tracing::info!(
                from = "tracker_state.json",
                to = %target_name,
                "Silently migrated tracker state to tenant-scoped file"
            );
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Could not rename tracker_state.json to tenant-scoped file — \
                 will create a fresh state file on next save"
            );
        }
    }
}

/// Loads the tracker state of a project from `tracker.db` (see [`crate::state_db`]).
///
/// A project not in the database yet has its `tracker_<hash>.json` imported,
/// once, then renamed `tracker_<hash>.json.imported` (kept as a backup). On the
/// way, the older one-shot migrations of that file still run:
///   - Renames `tracker_state.json` to the tenant-scoped `tracker_<hash>.json`
///   - Backfills `tracked_branches` from the state file into `projects.toml`
///
/// Returns `(mrs, branches, last_known_branches, discovery_started_at, dismissed_mr_ids)`.
/// `branches` is sourced (in priority order) from:
///   1. `tracked_branches` in `projects.toml` (already migrated)
///   2. `branches` in the state file (legacy — migrated on the spot)
///   3. Empty vec (first run)
pub async fn load_state_async(
    gitlab_url: &str,
    project_id: &str,
) -> (
    Vec<SavedMr>,
    Vec<String>,
    HashMap<String, HashSet<String>>,
    Option<String>,
    HashSet<String>,
) {
    let empty = || (vec![], vec![], HashMap::new(), None, HashSet::new());
    let Some(config_dir) = get_save_dir() else {
        return empty();
    };
    let db = match crate::state_db::open_in(&config_dir).await {
        Ok(db) => Some(db),
        Err(e) => {
            tracing::error!(error = %e, "Cannot open tracker.db: state read from the JSON file, not saved");
            None
        }
    };
    if let Some(db) = &db {
        match db.load(gitlab_url, project_id).await {
            Ok(Some((mrs, project))) => {
                return (
                    mrs,
                    vec![],
                    project.last_known_branches,
                    project.discovery_started_at,
                    project.dismissed_mr_ids,
                )
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(error = %e, "Cannot read tracker.db");
                return empty();
            }
        }
    }

    let Some(state) = read_state_file(&config_dir, gitlab_url, project_id).await else {
        return empty();
    };
    let branches = state.branches.clone();
    if let Some(db) = &db {
        import_state_file(db, &config_dir, gitlab_url, project_id, state.clone()).await;
    }
    (
        state.mrs,
        branches,
        state.last_known_branches,
        state.discovery_started_at,
        state.dismissed_mr_ids,
    )
}

/// The project's `tracker_<hash>.json`, as the versions before `tracker.db`
/// wrote it, with its own one-shot migrations; `None` when absent or unreadable
/// (moved aside to `.bak` then).
pub(super) async fn read_state_file(
    config_dir: &Path,
    gitlab_url: &str,
    project_id: &str,
) -> Option<SavedState> {
    // One-shot silent rename: tracker_state.json → tracker_<hash>.json.
    migrate_tracker_state_file(config_dir, gitlab_url, project_id).await;

    let file_name = tracker_state_file_name(gitlab_url, project_id);
    let path = config_dir.join(&file_name);
    let content = tokio::fs::read_to_string(&path).await.ok()?;
    let state = match serde_json::from_str::<SavedState>(&content) {
        Ok(state) => state,
        Err(e) => {
            backup_corrupt_file(&path, &e).await;
            return None;
        }
    };
    // One-shot migration: if the state file has branches and
    // projects.toml does not yet, backfill projects.toml now.
    if !state.branches.is_empty() {
        let mut cfg = load_projects_toml().await;
        // Target the project this state file belongs to, not the first one.
        let entry = find_project_mut(&mut cfg, gitlab_url, project_id);
        if let Some(entry) = entry.filter(|p| p.tracked_branches.is_none()) {
            entry.tracked_branches = Some(state.branches.clone());
            save_projects_toml(&cfg).await;
            tracing::info!(
                "Migrated tracked branches from {} to projects.toml",
                file_name
            );
        }
    }
    Some(state)
}

/// Imports `state` (read from `tracker_<hash>.json`) into the database, then
/// renames the file `….json.imported`: kept, no longer read.
pub(super) async fn import_state_file(
    db: &crate::state_db::StateDb,
    config_dir: &Path,
    gitlab_url: &str,
    project_id: &str,
    state: SavedState,
) -> Option<crate::state_db::ImportReport> {
    let path = config_dir.join(tracker_state_file_name(gitlab_url, project_id));
    let now = chrono::Utc::now().to_rfc3339();
    match db.import(gitlab_url, project_id, state, &now).await {
        Ok(report) => {
            let mut done = path.as_os_str().to_owned();
            done.push(".imported");
            let _ = tokio::fs::rename(&path, &done).await;
            tracing::info!(
                mrs = report.mrs,
                events = report.events,
                from = ?path,
                "Tracker state imported into tracker.db"
            );
            Some(report)
        }
        Err(e) => {
            tracing::error!(error = %e, path = ?path, "Cannot import the tracker state into tracker.db");
            None
        }
    }
}

/// Imports a project's `tracker_<hash>.json` into `tracker.db` by hand
/// (`gitlab-tracker import-state`), with its `merges_`/`owners_<hash>.json` and
/// `gui.json`: what the first launch does on its own.
/// `Ok(None)` when there is no file to import; an error when the project is
/// already in the database.
pub async fn import_state_command(
    gitlab_url: &str,
    project_id: &str,
) -> anyhow::Result<Option<crate::state_db::ImportReport>> {
    let config_dir = get_save_dir().ok_or_else(|| anyhow::anyhow!("No config directory"))?;
    let db = crate::state_db::open_in(&config_dir).await?;
    if db.has_project(gitlab_url, project_id).await? {
        anyhow::bail!(
            "This project is already in {}",
            config_dir.join(crate::state_db::DB_FILE).display()
        );
    }
    // The window's caches of the project, and its preferences, with it.
    #[cfg(feature = "gui")]
    {
        if let Some(store) = project_store(gitlab_url, project_id) {
            store.load_merges().await;
            store.load_owners().await;
        }
        load_setting::<serde_json::Value>("gui", "gui.json").await;
    }
    let Some(state) = read_state_file(&config_dir, gitlab_url, project_id).await else {
        return Ok(None);
    };
    import_state_file(&db, &config_dir, gitlab_url, project_id, state)
        .await
        .map(Some)
        .ok_or_else(|| anyhow::anyhow!("The import failed: see the log"))
}

pub async fn save_state_async(
    mrs: &[TrackedMr],
    last_known_branches: &HashMap<String, HashSet<String>>,
    discovery_started_at: Option<&str>,
    dismissed_mr_ids: &HashSet<String>,
    gitlab_url: &str,
    project_id: &str,
) {
    if let Some(file) = StateFile::new(
        mrs,
        last_known_branches,
        discovery_started_at,
        dismissed_mr_ids,
        gitlab_url,
        project_id,
    ) {
        file.write().await;
    }
}

/// The tracker state of a project, copied out of the app and ready to write:
/// [`save_state_async`] writes it at once, a [`StateWriter`] in the background.
pub struct StateFile {
    dir: PathBuf,
    gitlab_url: String,
    project_id: String,
    mrs: Vec<crate::state_db::MrRow>,
    project: crate::state_db::ProjectState,
}

impl StateFile {
    /// `None` when there is no config directory to save to.
    pub fn new(
        mrs: &[TrackedMr],
        last_known_branches: &HashMap<String, HashSet<String>>,
        discovery_started_at: Option<&str>,
        dismissed_mr_ids: &HashSet<String>,
        gitlab_url: &str,
        project_id: &str,
    ) -> Option<Self> {
        let dir = get_save_dir()?;
        let mrs = mrs
            .iter()
            .map(|m| crate::state_db::MrRow {
                id: m.id.clone(),
                // Only a read tells the branches: loading or failed, the stored ones stay.
                found_branches: match &m.status {
                    MrStatus::MergedIn(set) => Some(set.clone()),
                    _ => None,
                },
                flagged: m.flagged,
                snooze: m.snooze.clone(),
                linked_ticket: m.linked_ticket.clone(),
                mergeability: m.mergeability,
                data: m.data.clone(),
            })
            .collect();
        Some(Self {
            dir,
            gitlab_url: gitlab_url.to_string(),
            project_id: project_id.to_string(),
            mrs,
            project: crate::state_db::ProjectState {
                last_known_branches: last_known_branches.clone(),
                discovery_started_at: discovery_started_at.map(str::to_string),
                dismissed_mr_ids: dismissed_mr_ids.clone(),
            },
        })
    }

    pub async fn write(self) {
        let now = chrono::Utc::now().to_rfc3339();
        let saved = match crate::state_db::open_in(&self.dir).await {
            Ok(db) => {
                db.save(
                    &self.gitlab_url,
                    &self.project_id,
                    &self.mrs,
                    &self.project,
                    &now,
                )
                .await
            }
            Err(e) => Err(e),
        };
        if let Err(e) = saved {
            tracing::error!(error = %e, "Failed to save the tracker state to tracker.db");
        }
    }
}

/// Writes [`StateFile`]s in the background, one at a time and newest last: a save
/// returns at once, and the saves made while a write runs collapse into the newest.
#[derive(Clone, Default)]
pub struct StateWriter(std::sync::Arc<StateWriterInner>);

#[derive(Default)]
pub(super) struct StateWriterInner {
    /// The newest state not written yet.
    pub(super) latest: std::sync::Mutex<Option<StateFile>>,
    /// Held while writing: two writes would race on the same temporary file, and
    /// an older state could land last.
    pub(super) writing: tokio::sync::Mutex<()>,
}

// The TUI saves with `save_state_async`.
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
impl StateWriter {
    /// Queues `file` and returns: a background task writes it (needs a tokio runtime).
    pub fn save(&self, file: StateFile) {
        *self
            .0
            .latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(file);
        let writer = self.clone();
        tokio::spawn(async move { writer.flush().await });
    }

    /// Writes the newest queued state, after the write in progress if any.
    pub async fn flush(&self) {
        let _writing = self.0.writing.lock().await;
        let file = self
            .0
            .latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(file) = file {
            file.write().await;
        }
    }
}
