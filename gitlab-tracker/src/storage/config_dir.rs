//! The configuration directory, `config.json` and its `GITLAB_TRACKER_*` overrides, saved views.

use super::*;

#[cfg(not(test))]
pub fn get_save_dir() -> Option<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "gitlab-tracker", "gitlab-tracker")?;
    Some(project_dirs.config_dir().to_path_buf())
}

pub use gitlab_tracker_store::restrict_to_owner;

/// Under test the real config directory is never used: a test gets the directory
/// it installed with `tests::TempConfigDir` (per thread), and none otherwise — so
/// every load starts empty and every save is a no-op.
#[cfg(test)]
pub fn get_save_dir() -> Option<PathBuf> {
    TEST_SAVE_DIR.with(|dir| dir.borrow().clone())
}

#[cfg(test)]
thread_local! {
    pub(super) static TEST_SAVE_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Loads `AppConfig` using Figment with the following priority chain (highest → lowest):
///
/// 1. Environment variables prefixed with `GITLAB_TRACKER_`
///    (e.g. `GITLAB_TRACKER_REFRESH_INTERVAL_SECS=300`), see [`apply_env_overrides`]
/// 2. `config.json` in the XDG config directory
/// 3. Compiled-in defaults from `AppConfig::default()`
///
/// `main` then applies the project's `projects.toml` fields and calls
/// [`apply_env_overrides`] again, so the environment also wins over `projects.toml`.
///
/// When `config.json` does not exist yet, it is created with the default values
/// so the user has a ready-to-edit template on first run.
///
/// # Migration note
/// The legacy per-variable env overrides (`DEFAULT_BRANCHES`, `TABLE_LABEL_PREFIXES`,
/// `ACTIVITY_RECENT_DAYS`, `ACTIVITY_STALE_DAYS`) are still honoured via the
/// `GITLAB_TRACKER_` prefix mapping — e.g. `GITLAB_TRACKER_DEFAULT_BRANCHES=main,dev`.
pub async fn load_or_create_config_async() -> AppConfig {
    use figment::{
        providers::{Format, Json, Serialized},
        Figment,
    };

    let default_config = AppConfig::default();

    let mut figment = Figment::from(Serialized::defaults(&default_config));

    // Layer config.json on top of defaults when it exists.
    if let Some(config_dir) = get_save_dir() {
        let config_path = config_dir.join("config.json");

        if tokio::fs::try_exists(&config_path).await.unwrap_or(false) {
            figment = figment.merge(Json::file(&config_path));
        } else {
            // First run — write a default config.json template for the user.
            let _ = tokio::fs::create_dir_all(&config_dir).await;
            if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                let _ = tokio::fs::write(&config_path, json).await;
            }
        }
    }

    let config = figment.extract().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Invalid config.json — using default settings");
        default_config
    });
    apply_env_overrides(config)
}

/// Overlays the `GITLAB_TRACKER_*` environment variables on `config` (highest
/// priority). Prefix stripped and lowercased → `AppConfig` field names, e.g.
/// `GITLAB_TRACKER_REFRESH_INTERVAL_SECS=300`.
pub fn apply_env_overrides(config: AppConfig) -> AppConfig {
    apply_env_overrides_with_prefix(config, "GITLAB_TRACKER_")
}

pub(super) fn apply_env_overrides_with_prefix(config: AppConfig, prefix: &str) -> AppConfig {
    use figment::{
        providers::{Env, Serialized},
        Figment,
    };
    let overridden = Figment::from(Serialized::defaults(&config))
        .merge(Env::prefixed(prefix).map(|key| key.as_str().to_lowercase().into()))
        .extract::<AppConfig>();
    match overridden {
        // `#[serde(skip)]` fields do not survive the round trip: carry them
        // over, the per-project ones from `projects.toml` among them (a new
        // skipped field must be added here, or it is lost at startup).
        Ok(overridden) => AppConfig {
            gitlab_label_colors: config.gitlab_label_colors,
            gitlab_username: config.gitlab_username,
            hotfix_branches: config.hotfix_branches,
            release_note_tickets: config.release_note_tickets,
            freeze_periods: config.freeze_periods,
            signed_commits: config.signed_commits,
            ..overridden
        },
        Err(e) => {
            // One bad value (e.g. `GITLAB_TRACKER_DEFAULT_BRANCHES=main,dev` instead of
            // `[main,dev]`) invalidates the whole extraction: say so, and keep the
            // config without any environment override.
            tracing::warn!(error = %e, "Invalid GITLAB_TRACKER_* value — environment overrides ignored");
            config
        }
    }
}

/// Computes a short, stable FNV-1a 32-bit hash of the `(gitlab_url, project_id)` pair
/// and returns the tenant-scoped state file name, e.g. `tracker_fa123ffb.json`.
///
/// FNV-1a was chosen because it requires no external dependency, has excellent
/// distribution for short strings, and produces a compact 8-hex-char suffix that
/// is human-readable in a file listing.
///
/// The hash is computed over the canonical form `"<url>|<project_id>"` so that
/// different (url, id) pairs never collide even when one is a prefix of the other.
/// Saves the merge request table's views of a project.
#[cfg(feature = "gui")]
pub async fn save_views_async(
    views: &[crate::saved_views::SavedView],
    gitlab_url: &str,
    project_id: &str,
) -> bool {
    let views = views.to_vec();
    update_project_entry(gitlab_url, project_id, |entry| entry.views = views).await
}
