//! The per-project store (tracker.db): merge history, owners, settings, legacy JSON imports.

use super::*;

/// A project's caches in `tracker.db`: the merges (Delivery's pace) and the
/// merged MRs' owners (the reviewer suggestions). The JSON file each one had
/// before (`merges_<hash>.json`, `owners_<hash>.json`) is imported on first
/// use, then renamed `….imported`. `None` without a config directory.
#[cfg(feature = "gui")]
pub fn project_store(gitlab_url: &str, project_id: &str) -> Option<ProjectStore> {
    Some(ProjectStore {
        dir: get_save_dir()?,
        gitlab_url: gitlab_url.to_string(),
        project_id: project_id.to_string(),
    })
}

#[cfg(feature = "gui")]
#[derive(Debug, Clone)]
pub struct ProjectStore {
    dir: PathBuf,
    gitlab_url: String,
    project_id: String,
}

#[cfg(feature = "gui")]
impl ProjectStore {
    pub(super) fn legacy_file(&self, prefix: &str) -> PathBuf {
        let name = tracker_state_file_name(&self.gitlab_url, &self.project_id)
            .replacen("tracker_", prefix, 1);
        self.dir.join(name)
    }

    /// The merges saved by earlier launches, over any period.
    pub async fn load_merges(&self) -> Option<crate::gitlab_detail::MergeHistory> {
        let db = log_err(crate::state_db::open_in(&self.dir).await)?;
        match log_err(db.load_merges(&self.gitlab_url, &self.project_id).await)? {
            Some(history) => Some(history),
            None => {
                let path = self.legacy_file("merges_");
                let history: crate::gitlab_detail::MergeHistory = read_legacy(&path).await?;
                let saved = db
                    .save_merges(
                        &self.gitlab_url,
                        &self.project_id,
                        &history,
                        &Default::default(),
                    )
                    .await;
                imported(&path, log_err(saved).is_some()).await;
                Some(history)
            }
        }
    }

    pub async fn save_merges(
        &self,
        history: &crate::gitlab_detail::MergeHistory,
        known: &crate::gitlab_detail::MergeHistory,
    ) {
        if let Some(db) = log_err(crate::state_db::open_in(&self.dir).await) {
            log_err(
                db.save_merges(&self.gitlab_url, &self.project_id, history, known)
                    .await,
            );
        }
    }

    /// The merged MRs saved by earlier launches, over any period.
    pub async fn load_owners(&self) -> Option<crate::owners::History> {
        let db = log_err(crate::state_db::open_in(&self.dir).await)?;
        match log_err(db.load_owners(&self.gitlab_url, &self.project_id).await)? {
            Some(history) => Some(history),
            None => {
                let path = self.legacy_file("owners_");
                let history: crate::owners::History = read_legacy(&path).await?;
                let saved = db
                    .save_owners(
                        &self.gitlab_url,
                        &self.project_id,
                        &history,
                        &Default::default(),
                    )
                    .await;
                imported(&path, log_err(saved).is_some()).await;
                Some(history)
            }
        }
    }

    pub async fn save_owners(
        &self,
        history: &crate::owners::History,
        known: &crate::owners::History,
    ) {
        if let Some(db) = log_err(crate::state_db::open_in(&self.dir).await) {
            log_err(
                db.save_owners(&self.gitlab_url, &self.project_id, history, known)
                    .await,
            );
        }
    }
}

#[cfg(feature = "gui")]
/// The value, an error logged.
pub(super) fn log_err<T>(result: anyhow::Result<T>) -> Option<T> {
    result
        .map_err(|e| tracing::error!(error = %e, "tracker.db"))
        .ok()
}

#[cfg(feature = "gui")]
/// A JSON file of the versions before `tracker.db`; `None` when absent or
/// unreadable (ignored then: the next read starts over).
pub(super) async fn read_legacy<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(e) => {
            tracing::warn!(error = %e, path = ?path, "Unreadable JSON file, not imported");
            None
        }
    }
}

#[cfg(feature = "gui")]
/// Renames an imported JSON file `….imported`: kept, no longer read.
pub(super) async fn imported(path: &Path, done: bool) {
    if done {
        let mut to = path.as_os_str().to_owned();
        to.push(".imported");
        let _ = tokio::fs::rename(path, &to).await;
        tracing::info!(from = ?path, "Imported into tracker.db");
    }
}

#[cfg(feature = "gui")]
/// A setting saved in `tracker.db` (`gui`: the window's preferences), the JSON
/// file it had before (`legacy`, in the config directory) imported on first use.
pub async fn load_setting<T: serde::de::DeserializeOwned + serde::Serialize>(
    name: &str,
    legacy: &str,
) -> Option<T> {
    let dir = get_save_dir()?;
    let db = log_err(crate::state_db::open_in(&dir).await)?;
    if let Some(json) = log_err(db.setting(name).await)? {
        return serde_json::from_str(&json).ok();
    }
    let path = dir.join(legacy);
    let value: T = read_legacy(&path).await?;
    let json = serde_json::to_string(&value).ok()?;
    imported(&path, log_err(db.set_setting(name, &json).await).is_some()).await;
    Some(value)
}

#[cfg(feature = "gui")]
pub async fn save_setting<T: serde::Serialize>(name: &str, value: &T) {
    let Some(dir) = get_save_dir() else { return };
    let Ok(json) = serde_json::to_string(value) else {
        return;
    };
    if let Some(db) = log_err(crate::state_db::open_in(&dir).await) {
        log_err(db.set_setting(name, &json).await);
    }
}
