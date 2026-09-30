use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Regex patterns applied to MR titles and descriptions to detect Redmine ticket IDs.
///
/// Each pattern must contain exactly one capture group `(\d+)` that matches the numeric ID.
pub fn default_patterns() -> Vec<String> {
    vec![
        // Plain "#1234" reference (most common convention)
        r"#(\d+)".to_string(),
        // "refs #1234" or "fixes #1234" keywords
        r"(?i)(?:refs|fixes|closes|resolves)\s+#(\d+)".to_string(),
        // Direct Redmine URL embedded in the description
        r"/issues/(\d+)".to_string(),
    ]
}

/// Colour pair (background + foreground) for a badge label in the Inspector panel.
///
/// Colour values are the same strings accepted by `AppConfig::parse_color`:
/// named colours (`"red"`, `"cyan"`, …) or hex codes (`"#ff6600"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelColorConfig {
    /// Background colour of the badge.
    pub bg: String,
    /// Foreground (text) colour of the badge.
    pub fg: String,
}

/// GitLab lifecycle-state to Redmine status-id mapping.
///
/// Values are Redmine `status_id` strings discovered via `gitlab-tracker tracker-statuses`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitlabStateTransitionConfig {
    /// Redmine status id applied when a GitLab MR transitions to `Merged`.
    pub merged: Option<String>,
    /// Redmine status id applied when a GitLab MR transitions to `Closed`.
    pub closed: Option<String>,
}

/// Redmine workflow automation mappings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusTransitionsConfig {
    /// Mapping from GitLab MR lifecycle states to Redmine status ids.
    #[serde(default)]
    pub gitlab_state: GitlabStateTransitionConfig,
}

/// Per-project Redmine integration configuration embedded in `projects.toml`
/// under `[project.redmine]`.
///
/// Each GitLab project can point to a **different** Redmine instance, enabling
/// multi-tenant setups where project A uses `redmine-a.example.com` and project B
/// uses `redmine-b.example.com`. The API token for each instance is stored
/// separately in the OS keyring, keyed by the Redmine URL.
///
/// Example in `projects.toml`:
/// ```toml
/// [[project]]
/// name = "Backend — Client A"
/// project_id = "12345678"
/// gitlab_url = "https://gitlab.com"
///
/// [project.redmine]
/// url = "https://redmine-a.example.com"
///
/// [project.redmine.tracker_type_colors]
/// "Bug" = { bg = "red", fg = "white" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedmineConfig {
    /// Base URL of the Redmine instance, e.g. <https://redmine.example.com>.
    /// Must not have a trailing slash.
    pub url: String,

    /// Regex patterns used to detect ticket IDs in the MR title and description.
    /// Each pattern must expose the numeric ticket ID in capture group 1.
    #[serde(default = "default_patterns")]
    pub ticket_patterns: Vec<String>,

    /// Colour overrides for Redmine tracker-type labels displayed as badges
    /// in the Inspector panel (e.g. "Evolution", "Bug", "Support").
    ///
    /// Keys are matched case-insensitively against the `tracker` field returned
    /// by the Redmine API. Use `"*"` as a catch-all fallback.
    ///
    /// Example:
    /// ```toml
    /// [project.redmine.tracker_type_colors]
    /// "Bug"       = { bg = "red",      fg = "white" }
    /// "Evolution" = { bg = "cyan",     fg = "black" }
    /// "Support"   = { bg = "yellow",   fg = "black" }
    /// "*"         = { bg = "dark_gray", fg = "white" }
    /// ```
    #[serde(default)]
    pub tracker_type_colors: HashMap<String, LabelColorConfig>,

    /// Colour overrides for Redmine priority labels displayed as badges
    /// in the Inspector panel (e.g. "Regular", "High", "Urgent").
    ///
    /// Keys are matched case-insensitively against the `priority` field returned
    /// by the Redmine API. Use `"*"` as a catch-all fallback.
    ///
    /// Example:
    /// ```toml
    /// [project.redmine.priority_colors]
    /// "Low"     = { bg = "dark_gray", fg = "white" }
    /// "Regular" = { bg = "dark_gray", fg = "white" }
    /// "High"    = { bg = "yellow",    fg = "black" }
    /// "Urgent"  = { bg = "red",       fg = "white" }
    /// "*"       = { bg = "dark_gray", fg = "white" }
    /// ```
    #[serde(default)]
    pub priority_colors: HashMap<String, LabelColorConfig>,

    /// Optional workflow automation mapping from GitLab MR states to Redmine status IDs.
    ///
    /// The mapping is guarded by optimistic concurrency in the orchestrator: the issue
    /// is transitioned only when Redmine still has the same status as the last locally
    /// cached ticket, preventing accidental overwrite of manual workflow changes.
    #[serde(default)]
    pub status_transitions: StatusTransitionsConfig,
}

impl Default for RedmineConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            ticket_patterns: default_patterns(),
            tracker_type_colors: HashMap::new(),
            priority_colors: HashMap::new(),
            status_transitions: StatusTransitionsConfig::default(),
        }
    }
}

impl RedmineConfig {
    /// Returns `true` when the URL is non-empty after trimming.
    /// Used to gate the integration without unwrapping an `Option<RedmineConfig>`.
    pub fn is_active(&self) -> bool {
        !self.url.trim().is_empty()
    }

    /// Applies the `REDMINE_URL` environment variable override when set,
    /// returning whether the value was changed.
    pub fn apply_env_override(&mut self) -> bool {
        if let Ok(url) = std::env::var("REDMINE_URL") {
            let url = url.trim().to_string();
            if !url.is_empty() {
                self.url = url;
                return true;
            }
        }
        false
    }
}
