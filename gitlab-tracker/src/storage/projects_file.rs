//! `projects.toml` on disk: atomic writes, corrupt-file backups, the one-time import from `config.json`.

use super::*;

/// Returns the path to `projects.toml` in the XDG config directory.
pub fn projects_toml_path() -> Option<PathBuf> {
    get_save_dir().map(|d| d.join("projects.toml"))
}

/// Writes `content` to a sibling `*.tmp` file then renames it over `path`, so a
/// crash mid-write never leaves a truncated file behind.
pub(crate) async fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, path).await
}

/// Moves an unparseable file to `*.bak` so the next save cannot overwrite the
/// user's data with an empty default.
pub(super) async fn backup_corrupt_file(path: &Path, error: &(dyn std::fmt::Display + Sync)) {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    match tokio::fs::rename(path, &bak).await {
        Ok(()) => tracing::error!(
            error = %error, path = ?path, backup = ?bak,
            "Unparseable file moved to backup — fix it and rename it back"
        ),
        Err(e) => tracing::error!(error = %e, path = ?path, "Failed to back up unparseable file"),
    }
}

/// Loads `projects.toml`, returning an empty config when the file is absent or unparseable.
/// An unparseable file is first moved to `projects.toml.bak`.
pub async fn load_projects_toml() -> ProjectsConfig {
    let Some(path) = projects_toml_path() else {
        return ProjectsConfig::default();
    };
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                backup_corrupt_file(&path, &e).await;
                ProjectsConfig::default()
            }
        },
        Err(_) => ProjectsConfig::default(),
    }
}

/// Persists `projects.toml` to disk.
pub(super) async fn save_projects_toml(cfg: &ProjectsConfig) {
    let Some(path) = projects_toml_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    match toml::to_string_pretty(cfg) {
        Ok(content) => {
            if let Err(e) = write_atomic(&path, &content).await {
                tracing::error!(error = %e, path = ?path, "Failed to write projects.toml");
            }
        }
        Err(e) => tracing::error!(error = %e, "Failed to serialise projects.toml"),
    }
}

/// Attempts a one-time silent migration from the legacy `config.json` format.
///
/// If `projects.toml` does not exist yet but `config.json` contains
/// `project_id` and `gitlab_url` fields (written by an older version of the
/// app), this function creates `projects.toml` from those values — including
/// project-scoped settings (`default_branches`, `table_label_prefixes`,
/// `diff_difficulty_profile`) when present — and returns the resolved entry.
///
/// Returns `None` when the migration is not applicable (file absent, fields
/// missing, or already migrated).
pub(super) async fn try_migrate_from_config_json() -> Option<ProjectEntry> {
    let config_dir = get_save_dir()?;

    // Skip migration when projects.toml already exists.
    let toml_path = config_dir.join("projects.toml");
    if tokio::fs::try_exists(&toml_path).await.unwrap_or(false) {
        return None;
    }

    // Read config.json as a raw JSON value to extract the legacy fields without
    // depending on the current AppConfig struct layout.
    let json_path = config_dir.join("config.json");
    let content = tokio::fs::read_to_string(&json_path).await.ok()?;
    let root: serde_json::Value = serde_json::from_str(&content).ok()?;

    let gitlab_url = root
        .get("gitlab_url")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim_end_matches('/').to_string())?;

    let project_id = root
        .get("project_id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.to_string())?;

    // Migrate project-scoped settings that may exist in the legacy config.json.
    let default_branches: Option<Vec<String>> = root
        .get("default_branches")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .filter(|v: &Vec<String>| !v.is_empty());

    let table_label_prefixes: Option<Vec<String>> = root
        .get("table_label_prefixes")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .filter(|v: &Vec<String>| !v.is_empty());

    // Accept both the new key and the legacy key name.
    let complexity_profile: Option<crate::models::DifficultyProfile> = root
        .get("complexity_profile")
        .or_else(|| root.get("diff_difficulty_profile"))
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Migrate UI/display settings from the legacy config.json.
    let refresh_interval_secs: Option<u64> =
        root.get("refresh_interval_secs").and_then(|v| v.as_u64());

    let activity_stale_days: Option<u64> = root.get("activity_stale_days").and_then(|v| v.as_u64());

    let activity_recent_days: Option<u64> =
        root.get("activity_recent_days").and_then(|v| v.as_u64());

    let visible_columns: Option<crate::config::VisibleColumns> = root
        .get("visible_columns")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let label_colors: Option<std::collections::HashMap<String, crate::config::LabelColorConfig>> =
        root.get("label_colors")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .filter(|m: &std::collections::HashMap<_, _>| !m.is_empty());

    let entry = ProjectEntry {
        name: Some("Migrated from config.json".to_string()),
        gitlab_url: gitlab_url.clone(),
        project_id: project_id.clone(),
        active: true,
        default_branches,
        table_label_prefixes,
        complexity_profile,
        refresh_interval_secs,
        activity_stale_days,
        activity_recent_days,
        visible_columns,
        label_colors,
        ..Default::default()
    };

    // Write projects.toml with the migrated values.
    let cfg = ProjectsConfig {
        projects: vec![entry.clone()],
        ..Default::default()
    };
    save_projects_toml(&cfg).await;
    tracing::info!(
        gitlab_url = %gitlab_url,
        project_id = %project_id,
        "Migrated project settings from config.json to projects.toml"
    );
    println!("✅ Project settings migrated from config.json to projects.toml\n");

    Some(entry)
}

/// Enriches a `ProjectEntry` that is missing fields by reading them from the
/// legacy `config.json` — without touching `projects.toml` if those fields are
/// already populated.
///
/// This handles upgrades from older versions of the app where `projects.toml`
/// only contained `gitlab_url` + `project_id`. All fields (project-scoped and
/// display settings) are backfilled in one pass.
///
/// Returns `true` when the entry was modified and `projects.toml` needs saving.
pub(super) async fn try_enrich_from_config_json(entry: &mut ProjectEntry) -> bool {
    // Nothing to enrich — all fields already set.
    if entry.default_branches.is_some()
        && entry.table_label_prefixes.is_some()
        && entry.complexity_profile.is_some()
        && entry.refresh_interval_secs.is_some()
        && entry.activity_stale_days.is_some()
        && entry.activity_recent_days.is_some()
        && entry.visible_columns.is_some()
        && entry.label_colors.is_some()
    {
        return false;
    }

    let Some(config_dir) = get_save_dir() else {
        return false;
    };
    let json_path = config_dir.join("config.json");
    let Ok(content) = tokio::fs::read_to_string(&json_path).await else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };

    let mut changed = false;

    if entry.default_branches.is_none() {
        let val: Option<Vec<String>> = root
            .get("default_branches")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .filter(|v: &Vec<String>| !v.is_empty());
        if val.is_some() {
            entry.default_branches = val;
            changed = true;
        }
    }

    if entry.table_label_prefixes.is_none() {
        let val: Option<Vec<String>> = root
            .get("table_label_prefixes")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .filter(|v: &Vec<String>| !v.is_empty());
        if val.is_some() {
            entry.table_label_prefixes = val;
            changed = true;
        }
    }

    if entry.complexity_profile.is_none() {
        // Accept both the new key and the legacy key name.
        let val: Option<crate::models::DifficultyProfile> = root
            .get("complexity_profile")
            .or_else(|| root.get("diff_difficulty_profile"))
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        if val.is_some() {
            entry.complexity_profile = val;
            changed = true;
        }
    }

    if entry.refresh_interval_secs.is_none() {
        if let Some(val) = root.get("refresh_interval_secs").and_then(|v| v.as_u64()) {
            entry.refresh_interval_secs = Some(val);
            changed = true;
        }
    }

    if entry.activity_stale_days.is_none() {
        if let Some(val) = root.get("activity_stale_days").and_then(|v| v.as_u64()) {
            entry.activity_stale_days = Some(val);
            changed = true;
        }
    }

    if entry.activity_recent_days.is_none() {
        if let Some(val) = root.get("activity_recent_days").and_then(|v| v.as_u64()) {
            entry.activity_recent_days = Some(val);
            changed = true;
        }
    }

    if entry.visible_columns.is_none() {
        let val: Option<crate::config::VisibleColumns> = root
            .get("visible_columns")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        if val.is_some() {
            entry.visible_columns = val;
            changed = true;
        }
    }

    if entry.label_colors.is_none() {
        let val: Option<std::collections::HashMap<String, crate::config::LabelColorConfig>> = root
            .get("label_colors")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .filter(|m: &std::collections::HashMap<_, _>| !m.is_empty());
        if val.is_some() {
            entry.label_colors = val;
            changed = true;
        }
    }

    changed
}
