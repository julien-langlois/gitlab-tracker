use crate::config::{AppConfig, CockpitThresholds};
use crate::models::{MrStatus, SavedMr, SavedState, TrackedMr};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

mod config_dir;
#[cfg(feature = "gui")]
mod project_store;
mod projects;
mod projects_file;
mod state;
mod tokens;

pub use config_dir::*;
#[cfg(feature = "gui")]
pub use project_store::*;
pub use projects::*;
pub use projects_file::*;
pub use state::*;
pub use tokens::*;

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
    /// Seconds between two sentinel checks (`0`: off), see `crate::sentinel`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentinel_interval_secs: Option<u64>,
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

#[cfg(test)]
mod tests;
