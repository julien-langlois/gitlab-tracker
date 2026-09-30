use crate::config::{AppConfig, CockpitThresholds};
use crate::models::{MrStatus, SavedMr, SavedState, TrackedMr};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectEntry {
    /// Human-readable alias shown in prompts (e.g. "My Company — Backend").
    pub name: Option<String>,
    /// Base URL of the GitLab instance (e.g. <https://gitlab.com>).
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
    /// Controls whether the operational cockpit pane is displayed when enough vertical space is available.
    /// Defaults to `true` when omitted.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// show_cockpit = false
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_cockpit: Option<bool>,
    /// Thresholds used by the operational cockpit metrics and release risk detection.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// [project.cockpit_thresholds]
    /// stale_days = 7
    /// old_open_warning_days = 7
    /// old_open_alert_days = 14
    /// due_soon_days = 7
    /// complex_score = 0.66
    /// many_commits = 10
    /// many_files = 20
    /// hot_threads = 10
    /// release_urgent_days = 3
    /// release_urgent_remaining = 2
    /// release_soon_days = 7
    /// release_soon_remaining = 5
    /// max_release_summaries = 3
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cockpit_thresholds: Option<CockpitThresholds>,
    /// Stats feature settings for this project (retention policy, sprint duration, …).
    ///
    /// Grouped under a `[project.stats]` sub-table in `projects.toml`, mirroring
    /// the `[project.tracker]` pattern used for issue-tracker plugins.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// [project.stats]
    /// retention_days = 180
    /// sprint_weeks   = 3
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<StatsConfig>,
    /// Which optional columns are visible in the MR table for this project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_columns: Option<crate::config::VisibleColumns>,
    /// Label colour overrides: maps a label pattern (e.g. "deploy::*") to bg/fg colours.
    /// Keys may contain `::` and `*` — serialised as quoted TOML keys automatically.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_colors: Option<std::collections::HashMap<String, crate::config::LabelColorConfig>>,

    /// GitLab username of the person running this instance, as it appears in GitLab
    /// (e.g. `"jdoe"` — without the `@` prefix).
    ///
    /// When set, the **"Assigned to me"** and **"Reviewer: me"** filter entries become
    /// visible in the filter picker popup. These filters match against the `assignee`
    /// and `reviewers` fields respectively using the `@<username>` pattern that GitLab
    /// uses in its display strings.
    ///
    /// The value is per-project because a developer may use a different username on
    /// different GitLab instances (e.g. corporate SSO vs personal gitlab.com account).
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// gitlab_username = "jdoe"
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gitlab_username: Option<String>,

    /// Optional external tracker integration for this specific project.
    ///
    /// Each project can point to a **different** tracker instance (multi-tenant).
    /// The `provider` field selects the plugin; all other fields are provider-specific
    /// and parsed by the plugin itself — `ProjectEntry` stays closed to modification
    /// when new providers are added.
    ///
    /// The API token for each instance is stored in the OS keyring keyed by
    /// `tracker.url`, so multiple instances never clobber each other's credentials.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// [project.tracker]
    /// provider = "redmine"
    /// url      = "https://redmine.example.com"
    ///
    /// [project.tracker.tracker_type_colors]
    /// "Bug" = { bg = "red", fg = "white" }
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracker: Option<TrackerConfig>,

    /// When `true`, automatically discovers and tracks all newly created MRs on
    /// the project at each refresh cycle, even if they were not manually added.
    ///
    /// Uses `GET /projects/:id/merge_requests?state=all` and adds any MR whose
    /// IID is not yet in the tracking list. Independent of the `stats` feature —
    /// useful for any reviewer who wants the tool to self-populate, with or
    /// without analytics enabled.
    ///
    /// When the `stats` feature is also active, discovered MRs are automatically
    /// snapshotted, which improves team-wide coverage of throughput and cycle time
    /// metrics. Defaults to `false`.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// discover_new_mrs = true
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discover_new_mrs: Option<bool>,
}

/// Stats-feature configuration embedded in each `[[project]]` entry.
///
/// Grouped under `[project.stats]` in `projects.toml`, keeping all stats-related
/// knobs isolated from the top-level project fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatsConfig {
    /// Snapshots older than this many days are purged from the local SQLite
    /// database on startup. Defaults to `365` when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u32>,

    /// Sprint duration in weeks used for throughput forecasts.
    /// Defaults to `2` when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprint_weeks: Option<u32>,
}

/// Provider-agnostic tracker configuration embedded in each `[[project]]` entry.
///
/// The `provider` field acts as a discriminant that tells the runtime which
/// plugin crate to instantiate. All remaining fields are forwarded opaquely to
/// the plugin — `projects.toml` and `storage.rs` never need to change when a
/// new provider is added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerConfig {
    /// Plugin identifier — must match a compiled-in feature flag name.
    /// Accepted values (case-insensitive): `"redmine"`, `"jira"` (future), …
    pub provider: String,

    /// Base URL of the tracker instance (e.g. <https://redmine.example.com>).
    /// Used both as the API root and as the OS keyring account key.
    pub url: String,

    /// All remaining provider-specific fields (colours, patterns, …) are kept
    /// as a raw TOML table and forwarded to the plugin for deserialisation.
    /// Unknown keys are silently ignored, keeping forward-compatibility intact.
    #[serde(flatten)]
    pub extra: toml::Table,
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

/// Writes `content` to a sibling `*.tmp` file then renames it over `path`, so a
/// crash mid-write never leaves a truncated file behind.
async fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, path).await
}

/// Moves an unparseable file to `*.bak` so the next save cannot overwrite the
/// user's data with an empty default.
async fn backup_corrupt_file(path: &Path, error: &dyn std::fmt::Display) {
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
async fn save_projects_toml(cfg: &ProjectsConfig) {
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
///    When `projects.toml` has an entry for that pair, its settings are used;
///    otherwise the project runs on defaults.
/// 2. First entry with `active = true` in `projects.toml`
/// 3. First entry in `projects.toml`
/// 4. One-time silent migration from legacy `config.json`
/// 5. Interactive prompt → saved to `projects.toml`
pub async fn resolve_active_project() -> ProjectEntry {
    // 1. Environment variables — highest priority.
    let env_url = std::env::var("GITLAB_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let env_id = std::env::var("GITLAB_PROJECT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty());

    if let (Some(url), Some(id)) = (env_url, env_id) {
        let gitlab_url = url.trim_end_matches('/').to_string();
        let mut projects_cfg = load_projects_toml().await;
        let entry = find_project_mut(&mut projects_cfg, &gitlab_url, &id)
            .map(|entry| entry.clone())
            .unwrap_or_default();
        return ProjectEntry {
            gitlab_url,
            project_id: id,
            active: true,
            ..entry
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
        let needs_save_json = try_enrich_from_config_json(entry).await;
        if needs_save_json {
            tracing::info!(
                "Backfilled project-scoped settings into projects.toml from config.json"
            );
        }

        if needs_save_json {
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
        ..Default::default()
    };
    projects_cfg.projects.push(entry.clone());
    save_projects_toml(&projects_cfg).await;
    println!("✅ Project saved to projects.toml!\n");

    entry
}

/// Finds the `projects.toml` entry of the project the app is running on.
///
/// Matched by `(gitlab_url, project_id)` — never by position or by the `active`
/// flag: `--project`, `GITLAB_URL` / `GITLAB_PROJECT_ID` can select a project that is
/// not the active one, and saving into the active entry would corrupt it.
fn find_project_mut<'c>(
    cfg: &'c mut ProjectsConfig,
    gitlab_url: &str,
    project_id: &str,
) -> Option<&'c mut ProjectEntry> {
    let url = gitlab_url.trim_end_matches('/');
    cfg.projects
        .iter_mut()
        .find(|p| p.gitlab_url.trim_end_matches('/') == url && p.project_id == project_id)
}

/// Loads `projects.toml`, applies `update` to the current project's entry and saves.
///
/// Returns `false` (and writes nothing) when the project is not in `projects.toml`,
/// e.g. when it was only given through environment variables.
async fn update_project_entry(
    gitlab_url: &str,
    project_id: &str,
    update: impl FnOnce(&mut ProjectEntry),
) -> bool {
    let mut cfg = load_projects_toml().await;
    let Some(entry) = find_project_mut(&mut cfg, gitlab_url, project_id) else {
        tracing::warn!(
            gitlab_url,
            project_id,
            "Current project is not in projects.toml — setting not saved"
        );
        return false;
    };
    update(entry);
    save_projects_toml(&cfg).await;
    true
}

/// Persists the column visibility settings of the current project into `projects.toml`.
///
/// Called whenever the user closes the column picker popup so that the column
/// selection survives restarts without writing to the legacy `config.json`.
pub async fn save_visible_columns_async(
    cols: &crate::config::VisibleColumns,
    gitlab_url: &str,
    project_id: &str,
) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.visible_columns = Some(cols.clone());
    })
    .await;
}

/// Persists the tracked branch list of the current project into `projects.toml`.
///
/// Called whenever the user adds or removes a branch in the TUI so that the
/// branch list survives restarts without touching `tracker_state.json`.
pub async fn save_branches_async(branches: &[String], gitlab_url: &str, project_id: &str) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.tracked_branches = (!branches.is_empty()).then(|| branches.to_vec());
    })
    .await;
}

/// Persists a TOML-edited project settings table into `projects.toml`.
///
/// The settings dashboard edits a generic TOML table so plugin-provided settings
/// can be saved without hardcoding their fields in the main crate. The table is
/// deserialised back into `ProjectEntry` before saving to preserve validation and
/// the canonical typed storage model. It replaces the entry of the **current**
/// project (`gitlab_url`, `project_id`), whatever entry is marked active.
pub async fn save_project_settings_async(
    project_table: &toml::Table,
    gitlab_url: &str,
    project_id: &str,
) -> Option<ProjectEntry> {
    let project: ProjectEntry = toml::Value::Table(project_table.clone()).try_into().ok()?;
    let saved = project.clone();
    update_project_entry(gitlab_url, project_id, |entry| *entry = project)
        .await
        .then_some(saved)
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
/// Legacy account name — flat, instance-agnostic key used before multi-tenant support.
/// Only used during the one-time migrations in `migrate_legacy_keyring_entry`.
const KEYRING_ACCOUNT_LEGACY: &str = "gitlab_token";
/// Legacy service name used before the naming was unified (underscore variant).
/// Only used during the one-time migration in `migrate_legacy_keyring_entry`.
const KEYRING_SERVICE_LEGACY: &str = "gitlab_tracker";

/// Derives a stable, per-instance keyring account name from the GitLab instance URL.
///
/// Using the URL as the account key enables multi-tenant setups: each GitLab
/// instance stores its token independently so switching projects never clobbers
/// another instance's credentials.
///
/// Example: `"https://gitlab.example.com"` → `"gitlab_token::https://gitlab.example.com"`
fn account_for(gitlab_url: &str) -> String {
    format!("gitlab_token::{}", gitlab_url.trim_end_matches('/'))
}

/// Migrates tokens stored under legacy keyring keys to the current per-instance key.
///
/// Two migration steps are performed silently in order:
///   1. `gitlab_tracker` / `gitlab_token`  →  `gitlab-tracker` / `gitlab_token`  (service rename)
///   2. `gitlab-tracker` / `gitlab_token`  →  `gitlab-tracker` / `gitlab_token::<url>` (multi-tenant)
///
/// Each step is skipped if the target already contains a token, or if the source is empty.
/// This is a one-time, non-destructive operation safe to run on every startup.
pub fn migrate_legacy_keyring_entry(gitlab_url: &str) {
    let canonical_account = account_for(gitlab_url);

    // ── Step 1: service name rename (gitlab_tracker → gitlab-tracker) ────────
    // Skip if the intermediate flat canonical entry already exists.
    let flat_token = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) if !pwd.trim().is_empty() => Some(Zeroizing::new(pwd.trim().to_string())),
            _ => None,
        },
        Err(_) => None,
    };

    if flat_token.is_none() {
        // Try to pull from the very legacy entry (underscore service name).
        if let Ok(legacy_entry) =
            keyring::Entry::new(KEYRING_SERVICE_LEGACY, KEYRING_ACCOUNT_LEGACY)
        {
            if let Ok(pwd) = legacy_entry.get_password() {
                let pwd = Zeroizing::new(pwd.trim().to_string());
                if !pwd.is_empty() {
                    tracing::info!(
                        from = KEYRING_SERVICE_LEGACY,
                        to = KEYRING_SERVICE,
                        "Migrating token from legacy service name to canonical service name"
                    );
                    if let Ok(canonical_flat) =
                        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY)
                    {
                        if let Err(e) = canonical_flat.set_password(&pwd) {
                            tracing::error!(error = %e, "Failed to write to canonical flat entry during step-1 migration");
                        } else {
                            // Clean up the legacy service entry.
                            if let Err(e) = legacy_entry.delete_credential() {
                                tracing::warn!(error = %e, "Step-1 migration succeeded but failed to delete legacy entry");
                            } else {
                                tracing::info!(
                                    "Legacy keyring entry (step 1) deleted successfully"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Step 2: flat account → per-instance URL account ──────────────────────
    // Skip if the per-instance entry already holds a token.
    if let Ok(per_instance) = keyring::Entry::new(KEYRING_SERVICE, &canonical_account) {
        if let Ok(existing) = per_instance.get_password() {
            if !existing.trim().is_empty() {
                tracing::debug!(
                    account = %canonical_account,
                    "Per-instance keyring entry already populated — skipping step-2 migration"
                );
                return;
            }
        }
    }

    // Read the flat legacy token (written by step 1 or already present).
    let flat_token = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY) {
        Ok(entry) => match entry.get_password() {
            Ok(pwd) if !pwd.trim().is_empty() => Zeroizing::new(pwd.trim().to_string()),
            _ => return,
        },
        Err(_) => return,
    };

    tracing::info!(
        url = %gitlab_url,
        from = KEYRING_ACCOUNT_LEGACY,
        to = %canonical_account,
        "Migrating token from flat account key to per-instance account key"
    );

    // Write to the per-instance entry.
    match keyring::Entry::new(KEYRING_SERVICE, &canonical_account) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(&flat_token) {
                tracing::error!(error = %e, "Failed to write to per-instance keyring entry during step-2 migration");
                return;
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to open per-instance keyring entry during step-2 migration");
            return;
        }
    }

    // Delete the flat legacy entry now that the token is safely copied.
    match keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_LEGACY) {
        Ok(entry) => {
            if let Err(e) = entry.delete_credential() {
                tracing::warn!(error = %e, "Step-2 migration succeeded but failed to delete flat legacy entry");
            } else {
                tracing::info!("Flat legacy keyring entry (step 2) deleted successfully");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Step-2 migration succeeded but could not open flat legacy entry for deletion: {e}");
        }
    }
}

/// Resolves the GitLab PAT for a specific GitLab instance: `GITLAB_TOKEN`, then the
/// OS keyring entry for that URL, then a hidden prompt saved to the keyring
/// (see [`gitlab_tracker_core::secrets::resolve_secret`]). `None` when no token is
/// given — the caller stops, as nothing works without one.
pub fn get_or_prompt_token(gitlab_url: &str) -> Option<Zeroizing<String>> {
    gitlab_tracker_core::secrets::resolve_secret(&gitlab_tracker_core::secrets::SecretSource {
        env_var: "GITLAB_TOKEN",
        keyring_service: KEYRING_SERVICE,
        keyring_account: &account_for(gitlab_url),
        label: "GitLab Personal Access Token",
        prompt_hint: None,
    })
}

pub fn get_save_dir() -> Option<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "gitlab-tracker", "gitlab-tracker")?;
    Some(project_dirs.config_dir().to_path_buf())
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

        if config_path.exists() {
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

fn apply_env_overrides_with_prefix(config: AppConfig, prefix: &str) -> AppConfig {
    use figment::{
        providers::{Env, Serialized},
        Figment,
    };
    let overridden = Figment::from(Serialized::defaults(&config))
        .merge(Env::prefixed(prefix).map(|key| key.as_str().to_lowercase().into()))
        .extract::<AppConfig>();
    match overridden {
        // `#[serde(skip)]` fields do not survive the round trip: carry them over.
        Ok(overridden) => AppConfig {
            gitlab_label_colors: config.gitlab_label_colors,
            gitlab_username: config.gitlab_username,
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
fn tracker_state_file_name(gitlab_url: &str, project_id: &str) -> String {
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
async fn migrate_tracker_state_file(
    config_dir: &std::path::Path,
    gitlab_url: &str,
    project_id: &str,
) {
    let legacy = config_dir.join("tracker_state.json");
    let target_name = tracker_state_file_name(gitlab_url, project_id);
    let target = config_dir.join(&target_name);

    // Only migrate when the legacy file exists and the new file does not.
    if !legacy.exists() || target.exists() {
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

/// Loads the tracker state and performs one-shot silent migrations:
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
    let Some(config_dir) = get_save_dir() else {
        return (vec![], vec![], HashMap::new(), None, HashSet::new());
    };

    // One-shot silent rename: tracker_state.json → tracker_<hash>.json.
    migrate_tracker_state_file(&config_dir, gitlab_url, project_id).await;

    let file_name = tracker_state_file_name(gitlab_url, project_id);
    let path = config_dir.join(&file_name);

    if let Ok(content) = tokio::fs::read_to_string(&path).await {
        let parsed = serde_json::from_str::<SavedState>(&content);
        if let Err(e) = &parsed {
            backup_corrupt_file(&path, e).await;
        }
        if let Ok(state) = parsed {
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
            let discovery_started_at = state.discovery_started_at.clone();
            let dismissed_mr_ids = state.dismissed_mr_ids.clone();
            return (
                state.mrs,
                state.branches,
                state.last_known_branches,
                discovery_started_at,
                dismissed_mr_ids,
            );
        }
    }

    (vec![], vec![], HashMap::new(), None, HashSet::new())
}

pub async fn save_state_async(
    mrs: &[TrackedMr],
    last_known_branches: &HashMap<String, HashSet<String>>,
    discovery_started_at: Option<&str>,
    dismissed_mr_ids: &HashSet<String>,
    gitlab_url: &str,
    project_id: &str,
) {
    let state = SavedState {
        mrs: mrs
            .iter()
            .map(|m| SavedMr {
                id: m.id.clone(),
                found_branches: match &m.status {
                    MrStatus::MergedIn(set) => set.clone(),
                    _ => HashSet::new(),
                },
                flagged: m.flagged,
                linked_ticket: m.linked_ticket.clone(),
                data: m.data.clone(),
            })
            .collect(),
        // branches is no longer persisted here — it lives in projects.toml.
        branches: vec![],
        last_known_branches: last_known_branches.clone(),
        discovery_started_at: discovery_started_at.map(|s| s.to_string()),
        dismissed_mr_ids: dismissed_mr_ids.clone(),
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        if let Some(config_dir) = get_save_dir() {
            let file_name = tracker_state_file_name(gitlab_url, project_id);
            let path = config_dir.join(file_name);
            let _ = tokio::fs::create_dir_all(&config_dir).await;
            if let Err(e) = write_atomic(&path, &json).await {
                tracing::error!(error = %e, path = ?path, "Failed to write tracker state");
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn legacy_state_file_still_parses() {
        // Older versions stored `null` for optional fields and omitted newer ones.
        let legacy = r#"{"mrs": [{
            "id": "42", "title": "Fix login", "sha": null, "found_branches": ["main"],
            "description": null, "author": null, "assignee": "None", "milestone": null,
            "web_url": null, "labels": null
        }], "branches": []}"#;
        let state: SavedState = serde_json::from_str(legacy).expect("legacy file parses");
        let mr = &state.mrs[0];
        assert_eq!(
            (mr.id.as_str(), mr.data.title.as_str()),
            ("42", "Fix login")
        );
        assert_eq!(mr.data.description, "");
        assert!(mr.data.labels.is_empty() && mr.data.pipelines.is_empty());
        assert!(mr.found_branches.contains("main"));
        // Written back flat, as before (no nested "data" object).
        let json = serde_json::to_value(&state.mrs[0]).unwrap();
        assert!(json.get("data").is_none() && json.get("title").is_some());
    }

    #[test]
    fn saves_target_the_current_project_not_the_active_one() {
        let project = |url: &str, id: &str, active| ProjectEntry {
            gitlab_url: url.into(),
            project_id: id.into(),
            active,
            ..Default::default()
        };
        let mut cfg = ProjectsConfig {
            projects: vec![
                project("https://gitlab.a.com", "1", true),
                project("https://gitlab.b.com", "2", false),
            ],
        };
        // Started with `--project 2`: the second entry is edited, the active one untouched.
        let entry = find_project_mut(&mut cfg, "https://gitlab.b.com/", "2").unwrap();
        entry.tracked_branches = Some(vec!["main".into()]);
        assert_eq!(cfg.projects[0].tracked_branches, None);
        assert_eq!(
            cfg.projects[1].tracked_branches.as_deref(),
            Some(&["main".to_string()][..])
        );
        // Same id on another instance, or a project only known from env vars: no match.
        assert!(find_project_mut(&mut cfg, "https://gitlab.a.com", "2").is_none());
        assert!(find_project_mut(&mut cfg, "https://env-only.example", "9").is_none());
    }

    #[test]
    fn env_list_syntax_for_figment() {
        use figment::providers::{Env, Serialized};
        use figment::Figment;
        let parse = |value: &str| {
            // Unique prefix: tests share the process environment.
            std::env::set_var("GT_ENVTEST_DEFAULT_BRANCHES", value);
            let result = Figment::from(Serialized::defaults(AppConfig::default()))
                .merge(Env::prefixed("GT_ENVTEST_").map(|k| k.as_str().to_lowercase().into()))
                .extract::<AppConfig>()
                .map(|c| c.default_branches)
                .ok();
            std::env::remove_var("GT_ENVTEST_DEFAULT_BRANCHES");
            result
        };
        // A bare comma list is a string, not a sequence: extraction fails (and the
        // whole config used to fall back to defaults silently). The README documents
        // the bracket syntax.
        assert!(parse("main,staging").is_none());
        assert_eq!(parse("[main,staging]").unwrap(), ["main", "staging"]);

        std::env::set_var("GT_ENVTEST2_TABLE_LABEL_PREFIXES", "[deploy::,review::]");
        let prefixes = Figment::from(Serialized::defaults(AppConfig::default()))
            .merge(Env::prefixed("GT_ENVTEST2_").map(|k| k.as_str().to_lowercase().into()))
            .extract::<AppConfig>()
            .map(|c| c.table_label_prefixes)
            .ok();
        std::env::remove_var("GT_ENVTEST2_TABLE_LABEL_PREFIXES");
        assert_eq!(prefixes.unwrap(), ["deploy::", "review::"]);
    }

    use super::*;

    #[test]
    fn env_overrides_win_and_keep_runtime_fields() {
        // Unique prefix: tests share the process environment.
        std::env::set_var("GT_ENVTEST3_ACTIVITY_STALE_DAYS", "21");
        let project_config = AppConfig {
            activity_stale_days: 7, // as set from projects.toml
            gitlab_username: Some("jdoe".into()),
            ..AppConfig::default()
        };
        let config = apply_env_overrides_with_prefix(project_config, "GT_ENVTEST3_");
        std::env::remove_var("GT_ENVTEST3_ACTIVITY_STALE_DAYS");
        assert_eq!(config.activity_stale_days, 21);
        assert_eq!(config.gitlab_username.as_deref(), Some("jdoe"));
    }

    #[tokio::test]
    async fn atomic_write_and_corrupt_backup() {
        let dir = std::env::temp_dir().join(format!("gt-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");

        write_atomic(&path, "{broken").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        assert!(!dir.join("state.json.tmp").exists());

        backup_corrupt_file(&path, &"parse error").await;
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("state.json.bak")).unwrap(),
            "{broken"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
