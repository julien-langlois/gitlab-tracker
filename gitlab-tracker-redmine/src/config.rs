use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Regex patterns applied to MR titles and descriptions to detect Redmine ticket IDs.
///
/// Each pattern must contain exactly one capture group `(\d+)` that matches the numeric ID.
fn default_patterns() -> Vec<String> {
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

/// Redmine integration configuration, persisted as `redmine.yaml`
/// in the same config directory as the main application
/// (`~/.config/gitlab-tracker/gitlab-tracker/redmine.yaml`).
///
/// The file is created with sensible defaults on first run so the user
/// only needs to fill in `redmine_url`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedmineConfig {
    /// Base URL of the Redmine instance, e.g. "https://redmine.example.com".
    /// Must not have a trailing slash.
    pub redmine_url: String,

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
    /// ```yaml
    /// tracker_type_colors:
    ///   "Bug":       { bg: "red",    fg: "white" }
    ///   "Evolution": { bg: "cyan",   fg: "black" }
    ///   "Support":   { bg: "yellow", fg: "black" }
    ///   "*":         { bg: "dark_gray", fg: "white" }
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
    /// ```yaml
    /// priority_colors:
    ///   "Low":    { bg: "dark_gray", fg: "white" }
    ///   "Regular":  { bg: "dark_gray", fg: "white" }
    ///   "High":    { bg: "yellow",    fg: "black" }
    ///   "Urgent":  { bg: "red",       fg: "white" }
    ///   "*":        { bg: "dark_gray", fg: "white" }
    /// ```
    #[serde(default)]
    pub priority_colors: HashMap<String, LabelColorConfig>,
}

impl Default for RedmineConfig {
    fn default() -> Self {
        Self {
            redmine_url: String::new(),
            ticket_patterns: default_patterns(),
            tracker_type_colors: HashMap::new(),
            priority_colors: HashMap::new(),
        }
    }
}

/// Resolves the path to `redmine.yaml` in the application config directory.
pub fn get_config_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "gitlab-tracker", "gitlab-tracker")?;
    Some(dirs.config_dir().join("redmine.yaml"))
}

/// Ensures `redmine_url` is present in the config, prompting the user interactively
/// if it is missing. Persists any value entered so the user is only asked once.
///
/// The prompt is skipped (integration stays inactive) when the user leaves it empty.
pub async fn ensure_redmine_config(config: &mut RedmineConfig) {
    use std::io::Write;

    let url_from_env = std::env::var("REDMINE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());

    if url_from_env.is_none() && config.redmine_url.trim().is_empty() {
        println!("🌐 No Redmine URL found in config or environment.");
        println!("   Leave empty to disable Redmine integration.");
        print!("Redmine URL: ");
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() {
            let value = input.trim().to_string();
            if !value.is_empty() {
                config.redmine_url = value;
                save_config(config).await;
            }
        }
    } else if let Some(url) = url_from_env {
        config.redmine_url = url;
    }
}

/// Loads `redmine.yaml` from disk, or writes a default file and returns it.
///
/// On first run the generated file acts as documentation: the user can open it,
/// fill in `redmine_url`, and restart the app.
pub async fn load_or_create_config() -> RedmineConfig {
    if let Some(path) = get_config_path() {
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            match serde_yaml::from_str::<RedmineConfig>(&content) {
                Ok(cfg) => return cfg,
                Err(e) => {
                    tracing::warn!(error = %e, path = ?path, "Failed to parse redmine.yaml — using defaults");
                }
            }
        }

        // Write defaults so the user has a template to edit.
        let default = RedmineConfig::default();
        if let Ok(yaml) = serde_yaml::to_string(&default) {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if let Err(e) = tokio::fs::write(&path, yaml).await {
                tracing::error!(error = %e, path = ?path, "Failed to write default redmine.yaml");
            }
        }
        default
    } else {
        RedmineConfig::default()
    }
}

/// Persists the current config to `redmine.yaml`.
pub async fn save_config(config: &RedmineConfig) {
    if let Some(path) = get_config_path() {
        match serde_yaml::to_string(config) {
            Ok(yaml) => {
                if let Err(e) = tokio::fs::write(&path, yaml).await {
                    tracing::error!(error = %e, path = ?path, "Failed to save redmine.yaml");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize RedmineConfig");
            }
        }
    }
}
