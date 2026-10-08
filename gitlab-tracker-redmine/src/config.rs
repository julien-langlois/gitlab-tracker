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
/// under `[project.tracker]` (with `provider = "redmine"`).
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
/// [project.tracker]
/// provider = "redmine"
/// url = "https://redmine-a.example.com"
///
/// [project.tracker.tracker_type_colors]
/// "Bug" = { bg = "red", fg = "white" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedmineConfig {
    /// Base URL of the Redmine instance, e.g. <https://redmine.example.com>.
    /// Must not have a trailing slash.
    ///
    /// Optional when deserialising: the `[project.tracker]` fields reach this crate
    /// without `url` (the binary reads it itself and passes it on separately).
    #[serde(default)]
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
    /// [project.tracker.tracker_type_colors]
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
    /// [project.tracker.priority_colors]
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
    /// Builds the config from a `[project.tracker]` section: its `url` and the
    /// provider-specific `extra` fields (everything but `provider` and `url`).
    ///
    /// Invalid `extra` fields are logged and replaced by their defaults rather than
    /// failing: a typo in `status_transitions` must not disable ticket display.
    pub fn from_tracker_section(url: &str, extra: toml::Table) -> Self {
        let config: Self = extra.try_into().unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                "Invalid [project.tracker] settings — using defaults \
                 (ticket_patterns and status_transitions are ignored)"
            );
            Self::default()
        });
        Self {
            url: url.to_string(),
            ..config
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn section(toml: &str) -> RedmineConfig {
        RedmineConfig::from_tracker_section(
            "https://redmine.example",
            toml::from_str(toml).unwrap(),
        )
    }

    #[test]
    fn tracker_section_fields_are_read_without_a_url_key() {
        // `extra` is what is left of `[project.tracker]` once the binary has taken
        // `provider` and `url`: it used to fail on the missing `url` and silently
        // fall back to the defaults.
        let config = section(
            r##"
            ticket_patterns = ['RM-(\d+)']

            [tracker_type_colors]
            "Bug" = { bg = "red", fg = "white" }

            [priority_colors]
            "*" = { bg = "dark_gray", fg = "white" }

            [status_transitions.gitlab_state]
            merged = "5"
            "##,
        );
        assert_eq!(config.url, "https://redmine.example");
        assert_eq!(config.ticket_patterns, [r"RM-(\d+)"]);
        assert_eq!(config.tracker_type_colors["Bug"].bg, "red");
        assert_eq!(config.priority_colors["*"].fg, "white");
        assert_eq!(
            config.status_transitions.gitlab_state.merged.as_deref(),
            Some("5")
        );
        assert_eq!(config.status_transitions.gitlab_state.closed, None);
    }

    #[test]
    fn empty_or_invalid_section_falls_back_to_the_defaults() {
        let empty = section("");
        assert_eq!(empty.url, "https://redmine.example");
        assert_eq!(empty.ticket_patterns, default_patterns());
        assert!(empty.tracker_type_colors.is_empty() && empty.priority_colors.is_empty());
        assert!(empty.status_transitions.gitlab_state.merged.is_none());

        // A wrong type invalidates the section, not the integration: the URL is kept.
        let invalid = section("ticket_patterns = \"#(\\\\d+)\"\n");
        assert_eq!(invalid.url, "https://redmine.example");
        assert_eq!(invalid.ticket_patterns, default_patterns());
        // Unknown keys are ignored (forward compatibility).
        assert_eq!(
            section("future_option = true\n").ticket_patterns,
            default_patterns()
        );
    }

    #[test]
    fn integration_is_active_only_with_a_url() {
        assert!(!RedmineConfig::default().is_active());
        let blank = RedmineConfig {
            url: "   ".into(),
            ..RedmineConfig::default()
        };
        assert!(!blank.is_active());
        assert!(section("").is_active());
    }

    #[test]
    fn default_patterns_capture_the_ticket_number() {
        let capture = |pattern: &str, text: &str| {
            regex::Regex::new(pattern)
                .unwrap()
                .captures(text)
                .map(|c| c[1].to_string())
        };
        let patterns = default_patterns();
        assert_eq!(
            capture(&patterns[0], "Fix login #1234").as_deref(),
            Some("1234")
        );
        assert_eq!(capture(&patterns[1], "FIXES  #77").as_deref(), Some("77"));
        assert_eq!(
            capture(&patterns[2], "see https://redmine.example/issues/4271").as_deref(),
            Some("4271")
        );
        assert_eq!(capture(&patterns[0], "no reference"), None);
    }
}
