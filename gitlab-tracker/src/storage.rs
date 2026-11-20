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
    /// What each tracked branch is for, and which branch feeds a feature branch;
    /// what is left out is guessed from the names (see [`crate::branch_flow`]).
    ///
    /// ```toml
    /// [project.branch_flow]
    /// redesign_develop = { role = "feature", parent = "develop" }
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_flow: Option<std::collections::BTreeMap<String, crate::branch_flow::BranchSetting>>,
    /// How often the MR list is refreshed from GitLab, in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_interval_secs: Option<u64>,
    /// Number of days of inactivity above which an MR badge turns red (stale).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_stale_days: Option<u64>,
    /// Number of days of activity below which an MR badge turns green (recent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_recent_days: Option<u64>,
    /// Hours an MR may wait for my review before a reminder notification (0 = never).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_reminder_hours: Option<u64>,
    /// The source branches of hotfixes, as patterns separated by commas (`*`:
    /// any text). Absent: `hotfix/*`; empty: none.
    ///
    /// ```toml
    /// hotfix_branches = "hotfix/*, fix/prod-*"
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotfix_branches: Option<String>,
    /// Each line of the draft release notes starts with the MR's ticket,
    /// linked (`[Feature #123](…) - Title (!42)`). Absent: off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_note_tickets: Option<bool>,
    /// Deploy freezes declared in the app, used when GitLab has none for the
    /// project: a start cron, `->`, an end cron and an optional IANA zone (the
    /// machine's when left out), several separated by `;`.
    ///
    /// ```toml
    /// freeze_periods = "0 18 * * 5 -> 0 7 * * 1 Europe/Paris; 0 0 20 12 * -> 0 0 5 1 *"
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze_periods: Option<String>,
    /// Controls whether the operational cockpit pane is displayed when enough vertical space is available.
    /// Defaults to `true` when omitted.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// show_cockpit = false
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_cockpit: Option<bool>,
    /// Hides merged MRs from the table while keeping them tracked. Defaults to `false`.
    /// Toggled with `m` in the TUI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_merged: Option<bool>,
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
    /// The merge request table's views saved by name (`[[project.views]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<crate::saved_views::SavedView>,
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

    /// Whether every commit of an open MR must be signed, and what is tolerated.
    /// Off when omitted.
    ///
    /// Example in `projects.toml`:
    /// ```toml
    /// [project.signed_commits]
    /// enforce = true
    /// tolerate_gitlab_rebase = true   # a signature lost in a rebase from GitLab's UI
    /// require_same_patch = true       # …only for a copy with the same changes
    /// require_verified = true         # an unknown or revoked key is an anomaly
    /// accept_system_signed = false    # commits signed by the instance's own key
    /// exempt_bots = "renovate-bot bot@renovate.example"
    /// ```
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_commits: Option<SignedCommitsConfig>,
}

/// `[project.signed_commits]`: see [`ProjectEntry::signed_commits`]. Each
/// option left out takes [`crate::signatures::Policy`]'s default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedCommitsConfig {
    #[serde(default)]
    pub enforce: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tolerate_gitlab_rebase: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_same_patch: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept_system_signed: Option<bool>,
    /// `username email` pairs, separated by commas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exempt_bots: Option<String>,
}

impl SignedCommitsConfig {
    /// The policy to check MRs against, `None` when not enforced.
    pub fn policy(&self) -> Option<crate::signatures::Policy> {
        let default = crate::signatures::Policy::default();
        self.enforce.then(|| crate::signatures::Policy {
            tolerate_gitlab_rebase: self
                .tolerate_gitlab_rebase
                .unwrap_or(default.tolerate_gitlab_rebase),
            require_same_patch: self
                .require_same_patch
                .unwrap_or(default.require_same_patch),
            require_verified: self.require_verified.unwrap_or(default.require_verified),
            accept_system_signed: self
                .accept_system_signed
                .unwrap_or(default.accept_system_signed),
            exempt_bots: crate::signatures::BotAccount::parse_list(
                self.exempt_bots.as_deref().unwrap_or_default(),
            ),
        })
    }
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
    /// Language of the desktop window: a code such as `fr`, or absent to follow
    /// the system. Read by the GUI only; kept here so every save preserves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(rename = "project")]
    pub projects: Vec<ProjectEntry>,
}

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
async fn backup_corrupt_file(path: &Path, error: &(dyn std::fmt::Display + Sync)) {
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

    // 5. Interactive prompt — first run with no projects configured yet. Reading
    //    stdin blocks, so it runs off the async runtime.
    let entry = tokio::task::spawn_blocking(prompt_new_project)
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
    projects_cfg.projects.push(entry.clone());
    save_projects_toml(&projects_cfg).await;
    println!("✅ Project saved to projects.toml!\n");

    entry
}

/// Whether no project can be found: no `GITLAB_URL` + `GITLAB_PROJECT_ID`, an
/// empty `projects.toml`, and no legacy `config.json` to migrate (migrated here
/// when there is one). The window then asks for the first project itself.
#[cfg(feature = "gui")]
pub async fn needs_first_project() -> bool {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if env("GITLAB_URL").is_some() && env("GITLAB_PROJECT_ID").is_some() {
        return false;
    }
    if !load_projects_toml().await.projects.is_empty() {
        return false;
    }
    try_migrate_from_config_json().await.is_none()
}

/// Adds `entry` to `projects.toml` (the window's project form), as the only
/// `active` entry when `active`. An entry for the same project is replaced.
#[cfg(feature = "gui")]
pub async fn add_project(entry: ProjectEntry, active: bool) {
    let mut cfg = load_projects_toml().await;
    if active {
        for p in &mut cfg.projects {
            p.active = false;
        }
    }
    let entry = ProjectEntry { active, ..entry };
    match cfg
        .projects
        .iter_mut()
        .find(|p| is_same_project(p, &entry.gitlab_url, &entry.project_id))
    {
        Some(existing) => *existing = entry,
        None => cfg.projects.push(entry),
    }
    save_projects_toml(&cfg).await;
}

/// Whether a GitLab token is available for `gitlab_url` without asking
/// (`GITLAB_TOKEN` or the keyring). Blocks on the keyring: off the UI thread.
#[cfg(feature = "gui")]
pub fn has_gitlab_token(gitlab_url: &str) -> bool {
    migrate_legacy_keyring_entry(gitlab_url);
    gitlab_tracker_core::secrets::has_secret(
        gitlab_tracker_core::secrets::EnvToken {
            var: "GITLAB_TOKEN",
            url_var: "GITLAB_URL",
            instance_url: gitlab_url,
        },
        KEYRING_SERVICE,
        &account_for(gitlab_url),
    )
}

/// The GitLab token for `gitlab_url` without asking, `None` when there is none.
/// Blocks on the keyring.
#[cfg(feature = "gui")]
pub fn stored_gitlab_token(gitlab_url: &str) -> Option<Zeroizing<String>> {
    if !has_gitlab_token(gitlab_url) {
        return None;
    }
    get_or_prompt_token(gitlab_url)
}

/// Stores the GitLab token for `gitlab_url` in the OS keyring. Blocks.
#[cfg(feature = "gui")]
pub fn save_gitlab_token(gitlab_url: &str, token: &str) -> Result<(), String> {
    gitlab_tracker_core::secrets::store_secret(KEYRING_SERVICE, &account_for(gitlab_url), token)
}

/// First-run onboarding: asks for the GitLab URL, project ID and an optional name.
fn prompt_new_project() -> ProjectEntry {
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

    ProjectEntry {
        name,
        gitlab_url: gitlab_url.trim_end_matches('/').to_string(),
        project_id,
        active: true,
        ..Default::default()
    }
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
    cfg.projects
        .iter_mut()
        .find(|p| is_same_project(p, gitlab_url, project_id))
}

/// Whether `entry` is the project `(gitlab_url, project_id)` (trailing `/` ignored).
pub fn is_same_project(entry: &ProjectEntry, gitlab_url: &str, project_id: &str) -> bool {
    entry.gitlab_url.trim_end_matches('/') == gitlab_url.trim_end_matches('/')
        && entry.project_id == project_id
}

/// Marks `(gitlab_url, project_id)` as the only `active` entry of `projects.toml`,
/// so the next launch opens it. No-op when the project is not listed.
pub async fn set_active_project_async(gitlab_url: &str, project_id: &str) {
    let mut cfg = load_projects_toml().await;
    if !mark_active(&mut cfg, gitlab_url, project_id) {
        return;
    }
    save_projects_toml(&cfg).await;
}

/// Saves the window's language (`None`: follow the system) in `projects.toml`.
#[cfg(feature = "gui")]
pub async fn set_language_async(language: Option<String>) {
    let mut cfg = load_projects_toml().await;
    cfg.language = language;
    save_projects_toml(&cfg).await;
}

/// Sets `active` on the matching entry only; `false` when none matches.
fn mark_active(cfg: &mut ProjectsConfig, gitlab_url: &str, project_id: &str) -> bool {
    if !cfg
        .projects
        .iter()
        .any(|p| is_same_project(p, gitlab_url, project_id))
    {
        return false;
    }
    for p in &mut cfg.projects {
        p.active = is_same_project(p, gitlab_url, project_id);
    }
    true
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

/// Persists the "hide merged MRs" toggle of the current project into `projects.toml`.
pub async fn save_hide_merged_async(hide: bool, gitlab_url: &str, project_id: &str) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.hide_merged = Some(hide);
    })
    .await;
}

/// Saves the GitLab username of the project's token owner, read from GitLab by
/// the window, so the "me" views work in the terminal too.
#[cfg(feature = "gui")]
pub async fn save_gitlab_username_async(username: &str, gitlab_url: &str, project_id: &str) {
    let username = username.to_string();
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.gitlab_username = Some(username);
    })
    .await;
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

/// Persists the branch roles and parents of the current project into `projects.toml`.
pub async fn save_branch_flow_async(
    flow: Option<&std::collections::BTreeMap<String, crate::branch_flow::BranchSetting>>,
    gitlab_url: &str,
    project_id: &str,
) {
    update_project_entry(gitlab_url, project_id, |entry| {
        entry.branch_flow = flow.cloned();
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
        env: gitlab_tracker_core::secrets::EnvToken {
            var: "GITLAB_TOKEN",
            url_var: "GITLAB_URL",
            instance_url: gitlab_url,
        },
        keyring_service: KEYRING_SERVICE,
        keyring_account: &account_for(gitlab_url),
        label: "GitLab Personal Access Token",
        prompt_hint: None,
    })
}

#[cfg(not(test))]
pub fn get_save_dir() -> Option<PathBuf> {
    let project_dirs = directories::ProjectDirs::from("com", "gitlab-tracker", "gitlab-tracker")?;
    Some(project_dirs.config_dir().to_path_buf())
}

/// Creates `dir` if needed and, on Unix, makes it private to its owner (`0700`):
/// its logs, caches and `.env` describe the user's projects and may hold tokens
/// (`.env`), so other local accounts must not read them. Best effort: a failure is
/// logged later by the code that needs the directory.
pub fn restrict_to_owner(dir: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
}

/// Under test the real config directory is never used: a test gets the directory
/// it installed with `tests::TempConfigDir` (per thread), and none otherwise — so
/// every load starts empty and every save is a no-op.
#[cfg(test)]
pub fn get_save_dir() -> Option<PathBuf> {
    TEST_SAVE_DIR.with(|dir| dir.borrow().clone())
}

#[cfg(test)]
thread_local! {
    static TEST_SAVE_DIR: std::cell::RefCell<Option<PathBuf>> =
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

fn apply_env_overrides_with_prefix(config: AppConfig, prefix: &str) -> AppConfig {
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
    fn legacy_file(&self, prefix: &str) -> PathBuf {
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
fn log_err<T>(result: anyhow::Result<T>) -> Option<T> {
    result
        .map_err(|e| tracing::error!(error = %e, "tracker.db"))
        .ok()
}

#[cfg(feature = "gui")]
/// A JSON file of the versions before `tracker.db`; `None` when absent or
/// unreadable (ignored then: the next read starts over).
async fn read_legacy<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
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
async fn imported(path: &Path, done: bool) {
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
async fn read_state_file(
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
async fn import_state_file(
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
struct StateWriterInner {
    /// The newest state not written yet.
    latest: std::sync::Mutex<Option<StateFile>>,
    /// Held while writing: two writes would race on the same temporary file, and
    /// an older state could land last.
    writing: tokio::sync::Mutex<()>,
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

#[cfg(test)]
mod tests {

    #[test]
    fn mark_active_keeps_a_single_active_entry() {
        use super::{mark_active, ProjectEntry, ProjectsConfig};
        let entry = |id: &str, active: bool| ProjectEntry {
            gitlab_url: "https://g".into(),
            project_id: id.into(),
            active,
            ..ProjectEntry::default()
        };
        let mut cfg = ProjectsConfig {
            projects: vec![entry("1", true), entry("2", false)],
            ..Default::default()
        };
        assert!(mark_active(&mut cfg, "https://g/", "2"));
        let active: Vec<bool> = cfg.projects.iter().map(|p| p.active).collect();
        assert_eq!(active, [false, true]);
        assert!(
            !mark_active(&mut cfg, "https://g", "3"),
            "unknown project: untouched"
        );
        assert!(cfg.projects[1].active);
    }

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
            ..Default::default()
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
            // The per-project settings kept out of config.json.
            hotfix_branches: "fix/prod-*".into(),
            release_note_tickets: true,
            freeze_periods: "0 18 * * 5 -> 0 7 * * 1 Europe/Paris".into(),
            signed_commits: Some(crate::signatures::Policy::default()),
            ..AppConfig::default()
        };
        let config = apply_env_overrides_with_prefix(project_config, "GT_ENVTEST3_");
        std::env::remove_var("GT_ENVTEST3_ACTIVITY_STALE_DAYS");
        assert_eq!(config.activity_stale_days, 21);
        assert_eq!(config.gitlab_username.as_deref(), Some("jdoe"));
        assert_eq!(config.hotfix_branches, "fix/prod-*");
        assert!(config.release_note_tickets);
        assert_eq!(
            config.freeze_periods,
            "0 18 * * 5 -> 0 7 * * 1 Europe/Paris"
        );
        assert!(config.signed_commits.is_some());
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

    /// Points `get_save_dir` at a fresh temporary directory for the current test
    /// (current thread — `#[tokio::test]` runs on it), removed on drop.
    struct TempConfigDir(PathBuf);

    impl TempConfigDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("gt-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TEST_SAVE_DIR.with(|d| *d.borrow_mut() = Some(dir.clone()));
            Self(dir)
        }

        fn path(&self, file: &str) -> PathBuf {
            self.0.join(file)
        }
    }

    impl Drop for TempConfigDir {
        fn drop(&mut self) {
            TEST_SAVE_DIR.with(|d| *d.borrow_mut() = None);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const URL: &str = "https://gitlab.example";

    fn project(url: &str, id: &str, active: bool) -> ProjectEntry {
        ProjectEntry {
            gitlab_url: url.into(),
            project_id: id.into(),
            active,
            ..Default::default()
        }
    }

    async fn write_projects(entries: Vec<ProjectEntry>) {
        save_projects_toml(&ProjectsConfig {
            projects: entries,
            ..Default::default()
        })
        .await;
    }

    #[test]
    fn state_file_and_keyring_account_are_scoped_per_project() {
        let name = tracker_state_file_name(URL, "42");
        assert!(name.starts_with("tracker_") && name.ends_with(".json"));
        assert_eq!(name.len(), "tracker_.json".len() + 8);
        // A trailing slash is the same instance; another id or instance is not.
        assert_eq!(
            name,
            tracker_state_file_name("https://gitlab.example/", "42")
        );
        assert_ne!(name, tracker_state_file_name(URL, "43"));
        assert_ne!(name, tracker_state_file_name("https://other.example", "42"));
        // "<url>|<id>" is hashed: moving a character across the separator changes it.
        assert_ne!(
            tracker_state_file_name("https://g/1", "2"),
            tracker_state_file_name("https://g/", "12")
        );
        assert_eq!(
            account_for("https://gitlab.example/"),
            "gitlab_token::https://gitlab.example"
        );
    }

    #[test]
    fn tracker_section_keeps_provider_fields_apart_from_provider_and_url() {
        let cfg: ProjectsConfig = toml::from_str(
            r##"
            [[project]]
            gitlab_url = "https://gitlab.example"
            project_id = "1"

            [project.tracker]
            provider = "redmine"
            url = "https://redmine.example"
            ticket_patterns = ['RM-(\d+)']

            [project.tracker.status_transitions.gitlab_state]
            merged = "5"
            "##,
        )
        .unwrap();
        let tracker = cfg.projects[0].tracker.as_ref().unwrap();
        assert_eq!(tracker.provider, "redmine");
        assert_eq!(tracker.url, "https://redmine.example");
        // What the plugin receives: its own fields only — it must not expect `url`.
        let mut keys: Vec<&str> = tracker.extra.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["status_transitions", "ticket_patterns"]);

        // Written back flat under [project.tracker], as it was read.
        let written = toml::to_string_pretty(&cfg).unwrap();
        let reread: ProjectsConfig = toml::from_str(&written).unwrap();
        let reread = reread.projects[0].tracker.as_ref().unwrap();
        assert_eq!(reread.url, tracker.url);
        assert_eq!(reread.extra, tracker.extra);
    }

    #[tokio::test]
    async fn without_a_config_dir_loads_are_empty_and_saves_are_no_ops() {
        assert!(get_save_dir().is_none());
        assert!(load_projects_toml().await.projects.is_empty());
        save_branches_async(&["main".into()], URL, "1").await;
        let (mrs, branches, known, discovery, dismissed) = load_state_async(URL, "1").await;
        assert!(mrs.is_empty() && branches.is_empty() && known.is_empty());
        assert!(discovery.is_none() && dismissed.is_empty());
    }

    #[cfg(feature = "gui")]
    #[tokio::test]
    async fn the_window_adds_projects_without_duplicates() {
        let _dir = TempConfigDir::new("add-project");
        write_projects(vec![project(URL, "1", true)]).await;
        // Opened in a tab: added behind, the active one kept.
        add_project(project(&format!("{URL}/"), "2", true), false).await;
        // Added again (same instance and id): replaced, not doubled.
        let renamed = ProjectEntry {
            name: Some("Two".into()),
            ..project(URL, "2", false)
        };
        add_project(renamed, false).await;
        let cfg = load_projects_toml().await;
        let ids: Vec<(&str, bool)> = cfg
            .projects
            .iter()
            .map(|p| (p.project_id.as_str(), p.active))
            .collect();
        assert_eq!(ids, [("1", true), ("2", false)]);
        assert_eq!(cfg.projects[1].name.as_deref(), Some("Two"));
        // The first-run window's project: the only active one.
        add_project(project(URL, "3", false), true).await;
        let active: Vec<bool> = load_projects_toml()
            .await
            .projects
            .iter()
            .map(|p| p.active)
            .collect();
        assert_eq!(active, [false, false, true]);
    }

    #[tokio::test]
    async fn project_settings_are_saved_into_the_current_entry_only() {
        let dir = TempConfigDir::new("project-settings");
        write_projects(vec![project(URL, "1", true), project(URL, "2", false)]).await;

        save_branches_async(&["main".into(), "prod".into()], URL, "2").await;
        save_hide_merged_async(true, URL, "2").await;
        let mut cols = crate::config::VisibleColumns::default();
        cols.set_visible("milestone", true);
        save_visible_columns_async(&cols, URL, "2").await;
        // Not in projects.toml (e.g. selected through env vars): nothing is written.
        save_hide_merged_async(true, "https://env-only.example", "9").await;

        let cfg = load_projects_toml().await;
        assert_eq!(cfg.projects.len(), 2);
        let (first, second) = (&cfg.projects[0], &cfg.projects[1]);
        assert_eq!(first.tracked_branches, None);
        assert_eq!(first.hide_merged, None);
        assert_eq!(
            second.tracked_branches.as_deref(),
            Some(&["main".to_string(), "prod".to_string()][..])
        );
        assert_eq!(second.hide_merged, Some(true));
        assert!(second.visible_columns.is_some());

        // An empty branch list removes the key instead of storing `[]`.
        save_branches_async(&[], URL, "2").await;
        assert_eq!(
            load_projects_toml().await.projects[1].tracked_branches,
            None
        );

        set_active_project_async(URL, "2").await;
        let active: Vec<bool> = load_projects_toml()
            .await
            .projects
            .iter()
            .map(|p| p.active)
            .collect();
        assert_eq!(active, [false, true]);
        assert!(!dir.path("projects.toml.tmp").exists());
    }

    #[tokio::test]
    async fn settings_table_is_validated_before_replacing_the_entry() {
        let _dir = TempConfigDir::new("settings-table");
        write_projects(vec![project(URL, "1", true)]).await;

        let valid: toml::Table = toml::from_str(
            "gitlab_url = \"https://gitlab.example\"\nproject_id = \"1\"\n\
             activity_stale_days = 30\ngitlab_username = \"jdoe\"\n",
        )
        .unwrap();
        let saved = save_project_settings_async(&valid, URL, "1").await.unwrap();
        assert_eq!(saved.activity_stale_days, Some(30));
        let entry = load_projects_toml().await.projects.remove(0);
        assert_eq!(entry.activity_stale_days, Some(30));
        assert_eq!(entry.gitlab_username.as_deref(), Some("jdoe"));

        // A wrong type is rejected and leaves the file as it was.
        let invalid: toml::Table = toml::from_str(
            "gitlab_url = \"u\"\nproject_id = \"1\"\nactivity_stale_days = \"soon\"\n",
        )
        .unwrap();
        assert!(save_project_settings_async(&invalid, URL, "1")
            .await
            .is_none());
        assert_eq!(
            load_projects_toml().await.projects[0].activity_stale_days,
            Some(30)
        );
        // Valid table, but the project is not listed: not saved.
        assert!(save_project_settings_async(&valid, URL, "404")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn tracker_state_round_trips() {
        let dir = TempConfigDir::new("state-round-trip");
        let mut merged = TrackedMr::placeholder("7".into(), "Fix login".into(), None);
        merged.status = MrStatus::MergedIn(["main".to_string()].into());
        merged.flagged = true;
        let loading = TrackedMr::placeholder("8".into(), "Add stats".into(), Some("v1".into()));
        let known = HashMap::from([("main".to_string(), HashSet::from(["7".to_string()]))]);
        let dismissed = HashSet::from(["3".to_string()]);

        save_state_async(
            &[merged, loading],
            &known,
            Some("2026-01-02"),
            &dismissed,
            URL,
            "1",
        )
        .await;
        assert!(dir.path(crate::state_db::DB_FILE).exists());
        assert!(!dir.path(&tracker_state_file_name(URL, "1")).exists());

        let (mrs, branches, loaded_known, discovery, loaded_dismissed) =
            load_state_async(URL, "1").await;
        assert_eq!(mrs.len(), 2);
        assert_eq!(
            (mrs[0].id.as_str(), mrs[0].data.title.as_str()),
            ("7", "Fix login")
        );
        assert!(mrs[0].flagged && mrs[0].found_branches.contains("main"));
        // Only a `MergedIn` status carries branches.
        assert!(mrs[1].found_branches.is_empty());
        assert_eq!(mrs[1].data.milestone.as_deref(), Some("v1"));
        assert!(branches.is_empty(), "branches live in projects.toml");
        assert_eq!(loaded_known, known);
        assert_eq!(discovery.as_deref(), Some("2026-01-02"));
        assert_eq!(loaded_dismissed, dismissed);
        // Another project has its own (absent) file.
        assert!(load_state_async(URL, "2").await.0.is_empty());
    }

    #[tokio::test]
    async fn legacy_state_file_is_renamed_and_its_branches_backfilled() {
        let dir = TempConfigDir::new("state-migration");
        write_projects(vec![project(URL, "1", true), project(URL, "2", false)]).await;
        let legacy = r#"{"mrs": [], "branches": ["main", "staging"]}"#;
        std::fs::write(dir.path("tracker_state.json"), legacy).unwrap();

        // Loading project 2 claims the legacy file and backfills *its* entry.
        let (_, branches, ..) = load_state_async(URL, "2").await;
        assert_eq!(branches, ["main", "staging"]);
        assert!(!dir.path("tracker_state.json").exists());
        // Imported into tracker.db, the file is kept aside.
        let file = tracker_state_file_name(URL, "2");
        assert!(!dir.path(&file).exists());
        assert!(dir.path(&format!("{file}.imported")).exists());
        let cfg = load_projects_toml().await;
        assert_eq!(cfg.projects[0].tracked_branches, None);
        assert_eq!(
            cfg.projects[1].tracked_branches.as_deref(),
            Some(&["main".to_string(), "staging".to_string()][..])
        );

        // Branches already in projects.toml are never overwritten by the state file.
        save_branches_async(&["prod".into()], URL, "2").await;
        load_state_async(URL, "2").await;
        assert_eq!(
            load_projects_toml().await.projects[1]
                .tracked_branches
                .as_deref(),
            Some(&["prod".to_string()][..])
        );

        // A project that already has its file leaves a legacy file alone.
        std::fs::write(dir.path("tracker_state.json"), legacy).unwrap();
        load_state_async(URL, "2").await;
        assert!(dir.path("tracker_state.json").exists());
    }

    #[tokio::test]
    async fn a_state_file_is_imported_once_and_reloaded_from_the_database() {
        let dir = TempConfigDir::new("state-import");
        let json = r#"{
            "mrs": [
                {"id": "12", "found_branches": ["main"], "flagged": true,
                 "title": "Fix login", "author": "ana", "web_url": "https://g/12",
                 "state": "merged", "merged_by": "bob",
                 "merged_at": "2026-11-17T10:00:00Z", "approved_by": ["bob"],
                 "source_branch": "fix", "target_branch": "main"},
                {"id": "13", "found_branches": [], "title": "Add stats",
                 "author": "ana", "web_url": "https://g/13",
                 "source_branch": "stats", "target_branch": "main"}
            ],
            "last_known_branches": {"main": ["12"]},
            "discovery_started_at": "2026-01-02",
            "dismissed_mr_ids": ["3"]
        }"#;
        let file = tracker_state_file_name(URL, "1");
        std::fs::write(dir.path(&file), json).unwrap();

        let (mrs, _, known, discovery, dismissed) = load_state_async(URL, "1").await;
        let ids: Vec<&str> = mrs.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["12", "13"]);
        assert!(!dir.path(&file).exists());
        assert!(dir.path(&format!("{file}.imported")).exists());

        // Read again: from the database, the same.
        let (again, _, known2, discovery2, dismissed2) = load_state_async(URL, "1").await;
        let ids2: Vec<&str> = again.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids2, ["12", "13"]);
        assert!(again[0].flagged && again[0].found_branches.contains("main"));
        assert_eq!(again[0].data.approved_by, ["bob"]);
        assert_eq!(
            (known2, discovery2, dismissed2),
            (known, discovery, dismissed)
        );

        // By hand: already there.
        assert!(import_state_command(URL, "1").await.is_err());
        // Its history, dated by GitLab.
        let db = crate::state_db::open_in(&dir.0).await.unwrap();
        let kinds: Vec<&str> = db
            .events_of(URL, "1", "12")
            .await
            .unwrap()
            .iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                "tracked",
                "opened",
                "approved",
                "merged",
                "branch_reached",
                "flagged"
            ]
        );
    }

    #[cfg(feature = "gui")]
    #[tokio::test]
    async fn the_caches_and_window_prefs_are_imported_then_kept_in_the_database() {
        use crate::gitlab_detail::MergeHistory;
        use crate::owners::{History, Merge};
        let dir = TempConfigDir::new("caches");
        let hash = tracker_state_file_name(URL, "1");
        let merges_file = hash.replacen("tracker_", "merges_", 1);
        let owners_file = hash.replacen("tracker_", "owners_", 1);
        let recent = chrono::Utc::now().to_rfc3339();
        let merges = MergeHistory {
            merged: [(4, recent.clone()), (1, "2020-01-01T00:00:00Z".to_string())].into(),
            read_at: Some(recent.clone()),
        };
        std::fs::write(
            dir.path(&merges_file),
            serde_json::to_string(&merges).unwrap(),
        )
        .unwrap();
        let merge = Merge {
            merged_at: recent.clone(),
            author: "ana".into(),
            reviewers: vec!["zoe".into(), "bob".into()],
            approvers: vec!["bob".into()],
            paths: vec!["b.rs".into(), "a.rs".into()],
        };
        let owners = History {
            merges: [(4, merge.clone())].into(),
            people: [("ana".to_string(), "Ana".to_string())].into(),
            read_at: Some(recent.clone()),
        };
        std::fs::write(
            dir.path(&owners_file),
            serde_json::to_string(&owners).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.path("gui.json"),
            r#"{"page": "delivery", "light": true}"#,
        )
        .unwrap();

        // The caches first (a page opened before the MRs were saved): the
        // project's state file is still imported after.
        let store = project_store(URL, "1").unwrap();
        assert_eq!(store.load_merges().await, Some(merges.clone()));
        assert_eq!(store.load_owners().await, Some(owners.clone()));
        assert!(!dir.path(&merges_file).exists() && !dir.path(&owners_file).exists());
        std::fs::write(
            dir.path(&hash),
            r#"{"mrs": [{"id": "4", "found_branches": [], "title": "T", "web_url": "https://g/4"}]}"#,
        )
        .unwrap();
        assert_eq!(load_state_async(URL, "1").await.0.len(), 1);

        // Read back from the database, orders kept; a new read adds to them.
        assert_eq!(store.load_owners().await, Some(owners.clone()));
        let mut more = owners.clone();
        more.merges.insert(
            5,
            Merge {
                author: "bob".into(),
                ..merge
            },
        );
        store.save_owners(&more, &owners).await;
        assert_eq!(store.load_owners().await.unwrap().merges.len(), 2);
        // Out of the period, still in the database: the history.
        assert!(store.load_merges().await.unwrap().merged.contains_key(&1));

        let prefs: serde_json::Value = load_setting("gui", "gui.json").await.unwrap();
        assert_eq!(prefs["page"], "delivery");
        assert!(!dir.path("gui.json").exists());
        save_setting("gui", &serde_json::json!({"page": "today"})).await;
        let prefs: serde_json::Value = load_setting("gui", "gui.json").await.unwrap();
        assert_eq!(prefs["page"], "today");
    }

    /// The window saves from its UI thread, blocking inside the runtime.
    #[test]
    fn a_save_blocking_inside_the_runtime_completes() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let _dir = TempConfigDir::new("blocking-save");
            let mr = TrackedMr::placeholder("7".into(), "Fix".into(), None);
            let file =
                StateFile::new(&[mr], &HashMap::new(), None, &HashSet::new(), URL, "1").unwrap();
            tokio::task::block_in_place(|| futures::executor::block_on(file.write()));
            assert_eq!(load_state_async(URL, "1").await.0.len(), 1);
        });
    }

    #[tokio::test]
    async fn unparseable_files_are_moved_aside_not_overwritten() {
        let dir = TempConfigDir::new("corrupt");
        std::fs::write(dir.path("projects.toml"), "[[project]\nbroken").unwrap();
        assert!(load_projects_toml().await.projects.is_empty());
        assert!(!dir.path("projects.toml").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path("projects.toml.bak")).unwrap(),
            "[[project]\nbroken"
        );

        let state = tracker_state_file_name(URL, "1");
        std::fs::write(dir.path(&state), "{not json").unwrap();
        assert!(load_state_async(URL, "1").await.0.is_empty());
        assert!(!dir.path(&state).exists());
        assert!(dir.path(&format!("{state}.bak")).exists());
    }

    #[tokio::test]
    async fn legacy_config_json_migrates_once_and_backfills_missing_fields() {
        let dir = TempConfigDir::new("config-json");
        let config_json = r#"{
            "gitlab_url": "https://gitlab.example/", "project_id": "1",
            "default_branches": ["main"], "table_label_prefixes": [],
            "refresh_interval_secs": 300, "activity_stale_days": 9
        }"#;
        std::fs::write(dir.path("config.json"), config_json).unwrap();

        // Backfill: only the fields the entry does not set yet.
        let mut entry = ProjectEntry {
            activity_stale_days: Some(3),
            ..project(URL, "1", true)
        };
        assert!(try_enrich_from_config_json(&mut entry).await);
        assert_eq!(entry.activity_stale_days, Some(3));
        assert_eq!(entry.refresh_interval_secs, Some(300));
        assert_eq!(
            entry.default_branches.as_deref(),
            Some(&["main".to_string()][..])
        );
        assert_eq!(
            entry.table_label_prefixes, None,
            "empty lists are not migrated"
        );
        assert!(
            !try_enrich_from_config_json(&mut entry).await,
            "nothing left to backfill"
        );

        // Migration: creates projects.toml from config.json, once.
        let migrated = try_migrate_from_config_json().await.unwrap();
        assert_eq!(migrated.gitlab_url, URL);
        assert!(migrated.active);
        assert_eq!(migrated.activity_stale_days, Some(9));
        let cfg = load_projects_toml().await;
        assert_eq!(cfg.projects.len(), 1);
        assert_eq!(cfg.projects[0].project_id, "1");
        assert!(try_migrate_from_config_json().await.is_none());

        // Without the project fields there is nothing to migrate.
        std::fs::remove_file(dir.path("projects.toml")).unwrap();
        std::fs::write(
            dir.path("config.json"),
            r#"{"gitlab_url": " ", "project_id": "1"}"#,
        )
        .unwrap();
        assert!(try_migrate_from_config_json().await.is_none());
        assert!(!dir.path("projects.toml").exists());
    }

    #[tokio::test]
    async fn config_json_is_created_on_first_run_and_survives_bad_content() {
        let dir = TempConfigDir::new("config-load");
        // Whatever GITLAB_TRACKER_* the shell exports applies to both sides.
        let defaults = apply_env_overrides(AppConfig::default());

        let first = load_or_create_config_async().await;
        assert!(
            dir.path("config.json").exists(),
            "template written on first run"
        );
        assert_eq!(first.activity_stale_days, defaults.activity_stale_days);

        std::fs::write(dir.path("config.json"), r#"{"show_cockpit": "maybe"}"#).unwrap();
        let fallback = load_or_create_config_async().await;
        assert_eq!(fallback.show_cockpit, defaults.show_cockpit);
        assert_eq!(fallback.default_branches, defaults.default_branches);
    }

    #[tokio::test]
    async fn state_writer_lands_the_newest_state_last() {
        let _dir = TempConfigDir::new("state-writer");
        let writer = StateWriter::default();
        for id in ["1", "2", "3"] {
            let dismissed = HashSet::from([id.to_string()]);
            let file = StateFile::new(&[], &HashMap::new(), None, &dismissed, URL, "1").unwrap();
            writer.save(file);
        }
        writer.flush().await;
        let (.., dismissed) = load_state_async(URL, "1").await;
        assert_eq!(dismissed, HashSet::from(["3".to_string()]));
    }
}
