use crate::config::AppConfig;
use crate::models::{MrStatus, SavedMr, SavedState, TrackedMr};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use zeroize::Zeroizing;

// ── projects.toml ─────────────────────────────────────────────────────────────

/// A single GitLab project entry in `projects.toml`.
///
/// Each entry binds a GitLab instance URL to a project ID. The active project
/// is the first entry whose `active` field is `true`, or the first entry overall
/// when none is explicitly marked active.
///
/// All project-scoped settings are optional — omitting them falls back to the
/// compiled-in defaults. `config.json` is no longer needed once all fields are
/// present here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    /// Human-readable alias shown in prompts (e.g. "My Company — Backend").
    pub name: Option<String>,
    /// Base URL of the GitLab instance (e.g. "https://gitlab.com").
    pub gitlab_url: String,
    /// Numeric or string project ID as shown in GitLab project settings.
    pub project_id: String,
    /// When `true`, this project is loaded on startup without a picker.
    /// Defaults to `false`; the first entry is used when none is marked active.
    #[serde(default)]
    pub active: bool,
    /// Branch names whose pipeline status is shown in the MR table.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_branches: Option<Vec<String>>,
    /// Label prefixes whose chips appear in the "Labels" table column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_label_prefixes: Option<Vec<String>>,
    /// Tech-stack calibration for the review-difficulty score.
    /// `diff_difficulty_profile` is accepted as a legacy alias for seamless migration.
    #[serde(
        skip_serializing_if = "Option::is_none",
        alias = "diff_difficulty_profile"
    )]
    pub complexity_profile: Option<crate::models::DifficultyProfile>,
    /// Branches actively tracked in the MR table for this project.
    /// Set by the user via the TUI (Insert mode). Migrated one-shot from
    /// `tracker_state.json` on first startup, then owned exclusively here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracked_branches: Option<Vec<String>>,
    /// How often the MR list is refreshed from GitLab, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_interval_secs: Option<u64>,
    /// Number of days of inactivity above which an MR badge turns red (stale).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_stale_days: Option<u64>,
    /// Number of days of activity below which an MR badge turns green (recent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_recent_days: Option<u64>,
    /// Which optional columns are visible in the MR table for this project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_columns: Option<crate::config::VisibleColumns>,
    /// Label colour overrides: maps a label pattern (e.g. "deploy::*") to bg/fg colours.
    /// Keys may contain `::` and `*` — serialised as quoted TOML keys automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_colors: Option<std::collections::HashMap<String, crate::config::LabelColorConfig>>,
}

/// Root structure of `projects.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectsConfig {
    #[serde(rename = "project")]
    pub projects: Vec<ProjectEntry>,
}

/// Returns the path to `projects.toml` in the XDG config directory.
pub fn projects_toml_path() -> Option<PathBuf> {
    get_save_dir().map(|d| d.join("projects.toml"))
}

/// Loads `projects.toml`, returning an empty config when the file is absent or unparseable.
pub async fn load_projects_toml() -> ProjectsConfig {
    let Some(path) = projects_toml_path() else {
        return ProjectsConfig::default();
    };
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "Failed to parse projects.toml — using empty config");
            ProjectsConfig::default()
        }),
        Err(_) => ProjectsConfig::default(),
    }
}

/// Persists `projects.toml` to disk.
async fn save_projects_toml(cfg: &ProjectsConfig) {
    let Some(path) = projects_toml_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    match toml::to_string_pretty(cfg) {
        Ok(content) => {
            if let Err(e) = tokio::fs::write(&path, content).await {
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
/// app), this function creates `projects.toml` from those values and returns
/// the resolved `(gitlab_url, project_id)` pair.
///
/// Returns `None` when the migration is not applicable (file absent, fields
/// missing, or already migrated).
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
async fn try_migrate_from_config_json() -> Option<ProjectEntry> {
    let config_dir = get_save_dir()?;

    // Skip migration when projects.toml already exists.
    let toml_path = config_dir.join("projects.toml");
    if toml_path.exists() {
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
        tracked_branches: None, // migrated later from tracker_state.json
        refresh_interval_secs,
        activity_stale_days,
        activity_recent_days,
        visible_columns,
        label_colors,
    };

    // Write projects.toml with the migrated values.
    let cfg = ProjectsConfig {
        projects: vec![entry.clone()],
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
async fn try_enrich_from_config_json(entry: &mut ProjectEntry) -> bool {
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

/// Resolves the active `ProjectEntry` using the following priority:
///
/// 1. `GITLAB_URL` + `GITLAB_PROJECT_ID` environment variables (both required).
///    Project-scoped settings are not available via env vars — defaults are used.
/// 2. First entry with `active = true` in `projects.toml`
/// 3. First entry in `projects.toml`
/// 4. One-time silent migration from legacy `config.json`
/// 5. Interactive prompt → saved to `projects.toml`
pub async fn resolve_active_project() -> ProjectEntry {
    // 1. Environment variables — highest priority, no file I/O needed.
    let env_url = std::env::var("GITLAB_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let env_id = std::env::var("GITLAB_PROJECT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty());

    if let (Some(url), Some(id)) = (env_url, env_id) {
        return ProjectEntry {
            name: None,
            gitlab_url: url.trim_end_matches('/').to_string(),
            project_id: id,
            active: true,
            default_branches: None,
            table_label_prefixes: None,
            complexity_profile: None,
            tracked_branches: None,
            refresh_interval_secs: None,
            activity_stale_days: None,
            activity_recent_days: None,
            visible_columns: None,
            label_colors: None,
        };
    }

    // 2 & 3. projects.toml — active entry or first entry.
    let mut projects_cfg = load_projects_toml().await;

    if !projects_cfg.projects.is_empty() {
        let idx = projects_cfg
            .projects
            .iter()
            .position(|p| p.active)
            .unwrap_or(0);
        let entry = &mut projects_cfg.projects[idx];
        entry.gitlab_url = entry.gitlab_url.trim_end_matches('/').to_string();

        // Backfill project-scoped fields that were absent when projects.toml
        // was first created (one-time enrichment from config.json).
        if try_enrich_from_config_json(entry).await {
            tracing::info!(
                "Backfilled project-scoped settings into projects.toml from config.json"
            );
            save_projects_toml(&projects_cfg).await;
        }

        return projects_cfg.projects[idx].clone();
    }

    // 4. One-time migration from legacy config.json.
    if let Some(migrated) = try_migrate_from_config_json().await {
        return migrated;
    }

    // 5. Interactive prompt — first run with no projects configured yet.
    println!("⚙️  No project configured yet. Let's set one up.\n");

    print!("GitLab URL [https://gitlab.com]: ");
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    let gitlab_url = if std::io::stdin().read_line(&mut input).is_ok() {
        let v = input.trim().to_string();
        if v.is_empty() {
            "https://gitlab.com".to_string()
        } else {
            v
        }
    } else {
        "https://gitlab.com".to_string()
    };

    let project_id = prompt_required("GitLab Project ID");

    print!("Project name (optional label): ");
    let _ = std::io::stdout().flush();
    let mut name_input = String::new();
    let name = if std::io::stdin().read_line(&mut name_input).is_ok() {
        let v = name_input.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    } else {
        None
    };

    let entry = ProjectEntry {
        name,
        gitlab_url: gitlab_url.trim_end_matches('/').to_string(),
        project_id: project_id.clone(),
        active: true,
        default_branches: None,
        table_label_prefixes: None,
        complexity_profile: None,
        tracked_branches: None,
        refresh_interval_secs: None,
        activity_stale_days: None,
        activity_recent_days: None,
        visible_columns: None,
        label_colors: None,
    };
    projects_cfg.projects.push(entry.clone());
    save_projects_toml(&projects_cfg).await;
    println!("✅ Project saved to projects.toml!\n");

    entry
}

/// Persists the column visibility settings into `projects.toml` for the given entry index.
///
/// Called whenever the user closes the column picker popup so that the column
/// selection survives restarts without writing to the legacy `config.json`.
pub async fn save_visible_columns_async(cols: &crate::config::VisibleColumns, project_idx: usize) {
    let mut cfg = load_projects_toml().await;
    if let Some(entry) = cfg.projects.get_mut(project_idx) {
        entry.visible_columns = Some(cols.clone());
        save_projects_toml(&cfg).await;
    }
}

/// Persists the active branch list into `projects.toml` for the given entry index.
///
/// Called whenever the user adds or removes a branch in the TUI so that the
/// branch list survives restarts without touching `tracker_state.json`.
pub async fn save_branches_async(branches: &[String], project_idx: usize) {
    let mut cfg = load_projects_toml().await;
    if let Some(entry) = cfg.projects.get_mut(project_idx) {
        entry.tracked_branches = if branches.is_empty() {
            None
        } else {
            Some(branches.to_vec())
        };
        save_projects_toml(&cfg).await;
    }
}

/// Prompts the user interactively for a required config value (read from stdin).
fn prompt_required(label: &str) -> String {
    loop {
        print!("{}: ", label);
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let value = input.trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
        println!("  ⚠️  This field is required, please enter a value.");
    }
}

/// Service name used consistently for all keyring read/write operations.
const KEYRING_SERVICE: &str = "gitlab-tracker";
/// Account name used consistently for all keyring read/write operations.
const KEYRING_ACCOUNT: &str = "gitlab_token";
/// Legacy service name used before the naming was unified (underscore variant).
/// Only used during the one-time migration in `migrate_legacy_keyring_entry`.
const KEYRING_SERVICE_LEGACY: &str = "gitlab_tracker";

/// Migrates a token stored under the legacy keyring service name (`gitlab_tracker`)
/// to the canonical one (`gitlab-tracker`), then deletes the legacy entry.
///
/// This is a silent, one-time operation: if the canonical entry already exists,
/// or if no legacy entry is found, nothing happens.
pub fn migrate_legacy_keyring_entry() {
    // Skip migration if the canonical entry already holds a token.
    if let Ok(canonical) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        if let Ok(existing) = canonical.get_password() {
            if !existing.trim().is_empty() {
                tracing::debug!(
                    "Canonical keyring entry already populated — skipping legacy migration"
                );
                return;
            }
        }
    }

    // Attempt to read from the legacy entry.
    let legacy_token = match keyring::Entry::new(KEYRING_SERVICE_LEGACY, KEYRING_ACCOUNT) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) if !pwd.trim().is_empty() => Zeroizing::new(pwd.trim().to_string()),
            _ => return,
        },
        Err(_) => return,
    };

    tracing::info!(
        from = KEYRING_SERVICE_LEGACY,
        to = KEYRING_SERVICE,
        "Migrating token from legacy keyring entry to canonical entry"
    );

    // Write to the canonical entry.
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(&legacy_token) {
                tracing::error!(error = %e, "Failed to write token to canonical keyring entry during migration");
                return;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to open canonical keyring entry during migration");
            return;
        }
    }

    // Delete the legacy entry now that the token is safely copied.
    match keyring::Entry::new(KEYRING_SERVICE_LEGACY, KEYRING_ACCOUNT) {
        Ok(entry) => {
            if let Err(e) = entry.delete_password() {
                tracing::warn!(error = %e, "Token migrated but failed to delete legacy keyring entry");
            } else {
                tracing::info!("Legacy keyring entry deleted successfully");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Token migrated but could not open legacy keyring entry for deletion");
        }
    }
}

/// Resolves the GitLab PAT using the following priority chain:
///   1. `GITLAB_TOKEN` environment variable
///   2. OS keyring (via the `keyring` crate)
///   3. Interactive prompt (hidden input via `rpassword`, no terminal echo)
///
/// The returned value is wrapped in `Zeroizing<String>` so the secret bytes
/// are overwritten in memory as soon as the caller drops the value.
///
/// # Panics
/// Panics if no token is provided — the program cannot function without one.
pub fn get_or_prompt_token() -> Zeroizing<String> {
    // 1. Environment variable takes priority (CI / dotenv workflows).
    if let Ok(tok) = std::env::var("GITLAB_TOKEN") {
        let tok = Zeroizing::new(tok);
        if !tok.trim().is_empty() {
            tracing::info!("GITLAB_TOKEN loaded from environment variable");
            return Zeroizing::new(tok.trim().to_string());
        }
    }

    // 2. Try the OS keyring.
    tracing::debug!(
        service = KEYRING_SERVICE,
        account = KEYRING_ACCOUNT,
        "Attempting to read token from OS keyring"
    );
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        Ok(entry) => match entry.get_password() {
            Ok(password) => {
                let password = Zeroizing::new(password);
                if !password.trim().is_empty() {
                    tracing::info!("GITLAB_TOKEN loaded from OS keyring");
                    return Zeroizing::new(password.trim().to_string());
                }
                tracing::warn!("Keyring entry found but token is empty — falling back to prompt");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read token from OS keyring — falling back to prompt");
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "Failed to open keyring entry — falling back to prompt");
        }
    }

    // 3. Interactive prompt as a last resort.
    // `rpassword` disables terminal echo so the PAT never appears on screen
    // and cannot end up in shell history, screen recordings or logs.
    println!("🔑 No GITLAB_TOKEN found in environment or system Keyring.");
    match rpassword::prompt_password("Please enter your GitLab Personal Access Token: ") {
        Ok(raw) => {
            let token = Zeroizing::new(raw);
            if !token.trim().is_empty() {
                let token = Zeroizing::new(token.trim().to_string());
                tracing::debug!(
                    service = KEYRING_SERVICE,
                    account = KEYRING_ACCOUNT,
                    "Saving token to OS keyring"
                );
                match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
                    Ok(entry) => match entry.set_password(&token) {
                        Ok(_) => {
                            tracing::info!("Token successfully saved to OS keyring");
                            println!("✅ Token securely saved to OS Keyring!\n");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to save token to OS keyring");
                        }
                    },
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to open keyring entry for writing");
                    }
                }
                return token;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to read token from prompt");
        }
    }

    // Panic is intentional here: without a token the program cannot function.
    // In a future refactor this should become a Result<String, TokenError>.
    panic!("Error: Personal Access Token is required to run gitlab_tracker.");
}

pub fn get_save_dir() -> Option<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "gitlab-tracker", "gitlab-tracker")?;
    Some(project_dirs.config_dir().to_path_buf())
}

pub async fn load_or_create_config_async() -> AppConfig {
    if let Some(config_dir) = get_save_dir() {
        let config_path = config_dir.join("config.json");
        if let Ok(content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(mut config) = serde_json::from_str::<AppConfig>(&content) {
                if let Ok(env_branches) = std::env::var("DEFAULT_BRANCHES") {
                    config.default_branches = env_branches
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Ok(env_prefixes) = std::env::var("TABLE_LABEL_PREFIXES") {
                    config.table_label_prefixes = env_prefixes
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Ok(val) = std::env::var("ACTIVITY_RECENT_DAYS") {
                    if let Ok(days) = val.trim().parse::<u64>() {
                        config.activity_recent_days = days;
                    }
                }
                if let Ok(val) = std::env::var("ACTIVITY_STALE_DAYS") {
                    if let Ok(days) = val.trim().parse::<u64>() {
                        config.activity_stale_days = days;
                    }
                }
                return config;
            }
        }

        let default_config = AppConfig::default();
        let _ = tokio::fs::create_dir_all(&config_dir).await;
        if let Ok(json) = serde_json::to_string_pretty(&default_config) {
            let _ = tokio::fs::write(config_path, json).await;
        }
        return default_config;
    }
    AppConfig::default()
}

/// Loads the tracker state and performs a one-shot silent migration of
/// `branches` from `tracker_state.json` into `projects.toml` when needed.
///
/// Returns `(mrs, branches, last_known_branches)`.
/// `branches` is sourced (in priority order) from:
///   1. `tracked_branches` in `projects.toml` (already migrated)
///   2. `branches` in `tracker_state.json` (legacy — migrated on the spot)
///   3. Empty vec (first run)
pub async fn load_state_async() -> (Vec<SavedMr>, Vec<String>, HashMap<String, HashSet<String>>) {
    if let Some(config_dir) = get_save_dir() {
        let path = config_dir.join("tracker_state.json");
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(state) = serde_json::from_str::<SavedState>(&content) {
                // One-shot migration: if tracker_state.json has branches and
                // projects.toml does not yet, backfill projects.toml now.
                if !state.branches.is_empty() {
                    let mut cfg = load_projects_toml().await;
                    let needs_migration = cfg
                        .projects
                        .first()
                        .map(|p| p.tracked_branches.is_none())
                        .unwrap_or(false);
                    if needs_migration {
                        if let Some(entry) = cfg.projects.first_mut() {
                            entry.tracked_branches = Some(state.branches.clone());
                            save_projects_toml(&cfg).await;
                            tracing::info!(
                                "Migrated tracked branches from tracker_state.json to projects.toml"
                            );
                        }
                    }
                }
                return (state.mrs, state.branches, state.last_known_branches);
            }
        }
    }
    (vec![], vec![], HashMap::new())
}

pub async fn save_state_async(
    mrs: &[TrackedMr],
    last_known_branches: &HashMap<String, HashSet<String>>,
) {
    let state = SavedState {
        mrs: mrs
            .iter()
            .map(|m| SavedMr {
                id: m.id.clone(),
                title: m.title.clone(),
                sha: m.sha.clone(),
                found_branches: match &m.status {
                    MrStatus::MergedIn(set) => set.clone(),
                    _ => HashSet::new(),
                },
                description: Some(m.description.clone()),
                author: Some(m.author.clone()),
                assignee: Some(m.assignee.clone()),
                reviewers: m.reviewers.clone(),
                milestone: Some(m.milestone.clone()),
                milestone_due_date: m.milestone_due_date.clone(),
                web_url: Some(m.web_url.clone()),
                labels: Some(m.labels.clone()),
                updated_at: m.updated_at.clone(),
                source_branch: Some(m.source_branch.clone()),
                target_branch: Some(m.target_branch.clone()),
                state: m.state.clone(),
                merged_by: m.merged_by.clone(),
                merged_at: m.merged_at.clone(),
                pipelines: m.pipelines.clone(),
                user_notes_count: m.user_notes_count,
                flagged: m.flagged,
                linked_ticket: m.linked_ticket.clone(),
                diff_stats: m.diff_stats.clone(),
            })
            .collect(),
        // branches is no longer persisted here — it lives in projects.toml.
        branches: vec![],
        last_known_branches: last_known_branches.clone(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        if let Some(config_dir) = get_save_dir() {
            let path = config_dir.join("tracker_state.json");
            let _ = tokio::fs::create_dir_all(&config_dir).await;
            let _ = tokio::fs::write(path, json).await;
        }
    }
}
