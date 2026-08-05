use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_branches_val() -> Vec<String> {
    vec!["main".to_string()]
}

/// Controls which optional columns are rendered in the MR table.
///
/// All columns default to `false` (hidden) so the table stays compact
/// out of the box. Each field can be toggled individually in `config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VisibleColumns {
    /// Show the "Activity" column (Stale / Slowing / Active badge).
    #[serde(default)]
    pub activity: bool,
    /// Show the "Target" column (target branch the MR merges into).
    #[serde(default)]
    pub target_branch: bool,
    /// Show the "Labels" column (filtered label chips).
    #[serde(default)]
    pub labels: bool,
    /// Show the "Milestone" column.
    #[serde(default)]
    pub milestone: bool,
    /// Show the "Notes" column (total number of comments and discussion threads).
    #[serde(default)]
    pub notes: bool,
    /// Show the "Ticket" column with the linked tracker ticket ID + status.
    /// Only visible when a tracker provider is configured at runtime.
    #[serde(default)]
    pub tracker_ticket: bool,
}

/// Default threshold (in days) above which an MR is considered "stale" (red badge).
fn default_activity_stale_days() -> u64 {
    7
}

/// Default threshold (in days) below which an MR is considered "recent" (green badge).
fn default_activity_recent_days() -> u64 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelColorConfig {
    pub bg: String,
    pub fg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub project_id: Option<String>,
    pub gitlab_url: Option<String>,
    pub refresh_interval_secs: Option<u64>,
    #[serde(default = "default_branches_val")]
    pub default_branches: Vec<String>,
    #[serde(default)]
    pub table_label_prefixes: Vec<String>,
    #[serde(default)]
    pub label_colors: HashMap<String, LabelColorConfig>,
    /// Number of days of inactivity above which an MR badge turns red (stale).
    #[serde(default = "default_activity_stale_days")]
    pub activity_stale_days: u64,
    /// Number of days of activity below which an MR badge turns green (recent).
    #[serde(default = "default_activity_recent_days")]
    pub activity_recent_days: u64,
    /// Controls which optional columns are visible in the MR table.
    /// All columns are hidden by default — enable them individually in config.json.
    #[serde(default)]
    pub visible_columns: VisibleColumns,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut label_colors = HashMap::new();
        label_colors.insert(
            "deploy::*".to_string(),
            LabelColorConfig {
                bg: "green".into(),
                fg: "black".into(),
            },
        );
        label_colors.insert(
            "review::approved".to_string(),
            LabelColorConfig {
                bg: "magenta".into(),
                fg: "white".into(),
            },
        );
        label_colors.insert(
            "review::*".to_string(),
            LabelColorConfig {
                bg: "cyan".into(),
                fg: "black".into(),
            },
        );
        label_colors.insert(
            "size::*".to_string(),
            LabelColorConfig {
                bg: "dark_gray".into(),
                fg: "white".into(),
            },
        );
        label_colors.insert(
            "bug".to_string(),
            LabelColorConfig {
                bg: "red".into(),
                fg: "white".into(),
            },
        );
        label_colors.insert(
            "fix".to_string(),
            LabelColorConfig {
                bg: "red".into(),
                fg: "white".into(),
            },
        );

        Self {
            project_id: None,
            gitlab_url: None,
            refresh_interval_secs: Some(900),
            default_branches: default_branches_val(),
            table_label_prefixes: vec!["deploy::".to_string(), "review::".to_string()],
            label_colors,
            activity_stale_days: default_activity_stale_days(),
            activity_recent_days: default_activity_recent_days(),
            visible_columns: VisibleColumns::default(),
        }
    }
}

impl AppConfig {
    pub fn is_table_label(&self, label: &str) -> bool {
        if self.table_label_prefixes.is_empty() {
            return true;
        }
        let label_lower = label.to_lowercase();
        self.table_label_prefixes
            .iter()
            .any(|prefix| label_lower.starts_with(&prefix.to_lowercase()))
    }

    /// Computes the activity badge for a given ISO 8601 `updated_at` timestamp.
    ///
    /// Returns a `(icon, Color)` tuple based on the configured thresholds:
    /// - Green  🟢 : updated within `activity_recent_days`
    /// - Yellow 🟡 : updated between `activity_recent_days` and `activity_stale_days`
    /// - Red    🔴 : not updated for more than `activity_stale_days`
    /// - Gray      : timestamp unavailable or unparseable
    pub fn activity_badge(&self, updated_at: Option<&str>) -> (&'static str, Color) {
        use chrono::{DateTime, Utc};

        let Some(ts) = updated_at else {
            return ("⬛ Unknown", Color::DarkGray);
        };

        let Ok(parsed) = DateTime::parse_from_rfc3339(ts) else {
            return ("⬛ Unknown", Color::DarkGray);
        };

        let elapsed_days = (Utc::now() - parsed.to_utc()).num_days();

        if elapsed_days < self.activity_recent_days as i64 {
            ("🟢 Active", Color::Green)
        } else if elapsed_days < self.activity_stale_days as i64 {
            ("🟡 Slowing", Color::Yellow)
        } else {
            ("🔴 Stale", Color::Red)
        }
    }

    pub fn get_label_style(&self, label: &str) -> (Color, Color) {
        let label_lower = label.to_lowercase();

        if let Some(cfg) = self.label_colors.get(&label_lower) {
            return (parse_color(&cfg.bg), parse_color(&cfg.fg));
        }

        for (key, cfg) in &self.label_colors {
            if let Some(prefix) = key.strip_suffix('*') {
                if label_lower.starts_with(&prefix.to_lowercase()) {
                    return (parse_color(&cfg.bg), parse_color(&cfg.fg));
                }
            }
        }

        (Color::DarkGray, Color::White)
    }
}

pub fn parse_color(s: &str) -> Color {
    let s_lower = s.to_lowercase();
    match s_lower.as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "black" => Color::Black,
        // Named ANSI greys are remapped to absolute RGB values so that badges and
        // muted text render consistently across all terminal palettes (Ambiance,
        // Dracula, Nord, Gruvbox Dark, Solarized Dark, …).
        // Color::DarkGray maps to ANSI slot #8 which many palettes render too dark
        // (~#333–#555) to remain legible as a badge background or muted foreground.
        "dark_gray" | "dark_grey" => Color::Rgb(88, 88, 100),
        "gray" | "grey" | "light_gray" | "light_grey" => Color::Rgb(160, 160, 175),
        _ => {
            if s.starts_with('#') && s.len() == 7 {
                let r = u8::from_str_radix(&s[1..3], 16).unwrap_or(128);
                let g = u8::from_str_radix(&s[3..5], 16).unwrap_or(128);
                let b = u8::from_str_radix(&s[5..7], 16).unwrap_or(128);
                Color::Rgb(r, g, b)
            } else {
                // Unknown token — fall back to a visible mid-grey rather than the
                // opaque ANSI DarkGray that may be invisible on dark palettes.
                Color::Rgb(88, 88, 100)
            }
        }
    }
}
