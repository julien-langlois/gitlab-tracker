use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_branches_val() -> Vec<String> {
    vec!["main".to_string()]
}

/// Visibility state for optional MR table columns.
///
/// Replaces the old fixed-field struct — columns are now registered via
/// `inventory::submit!(ColumnDef { … })` in each crate, so this map is
/// populated dynamically from `collect_all_columns()` at startup.
///
/// Keys are the stable `ColumnDef::id` values (e.g. `"activity"`, `"tracker_ticket"`).
/// Serialised as a flat TOML/JSON map for backward-compatibility with `projects.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VisibleColumns(pub std::collections::HashMap<String, bool>);

impl VisibleColumns {
    /// Returns `true` when the column with the given id is visible.
    /// Defaults to `false` (hidden) when the key is absent.
    pub fn is_visible(&self, id: &str) -> bool {
        self.0.get(id).copied().unwrap_or(false)
    }

    /// Sets the visibility of the column with the given id.
    pub fn set_visible(&mut self, id: &str, visible: bool) {
        self.0.insert(id.to_string(), visible);
    }

    /// Toggles the visibility of the column with the given id.
    pub fn toggle(&mut self, id: &str) {
        let current = self.is_visible(id);
        self.set_visible(id, !current);
    }

    /// Initialises missing entries from `ColumnDef::default_visible` so that
    /// newly registered columns are visible/hidden according to their default
    /// without requiring the user to update their config file.
    pub fn apply_defaults(&mut self, cols: &[&'static gitlab_tracker_core::ColumnDef]) {
        for col in cols {
            self.0
                .entry(col.id.to_string())
                .or_insert(col.default_visible);
        }
    }
}

/// Default threshold (in days) above which an MR is considered "stale" (red badge).
fn default_activity_stale_days() -> u64 {
    7
}

/// Default threshold (in days) below which an MR is considered "recent" (green badge).
fn default_activity_recent_days() -> u64 {
    2
}

/// Default cockpit visibility for projects that do not override it.
fn default_show_cockpit() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockpitThresholds {
    /// Number of days after which an open MR is counted as stale in the cockpit.
    pub stale_days: i64,
    /// Number of days after which the oldest open MR metric turns yellow.
    pub old_open_warning_days: i64,
    /// Number of days after which the oldest open MR metric turns red.
    pub old_open_alert_days: i64,
    /// Number of days before a milestone due date is counted as due soon.
    pub due_soon_days: i64,
    /// Difficulty score from which an MR is counted as complex.
    pub complex_score: f64,
    /// Commit count from which an MR is counted as having many commits.
    pub many_commits: u32,
    /// Changed file count from which an MR is counted as touching many files.
    pub many_files: u32,
    /// User note count from which an MR is counted as a hot thread.
    pub hot_threads: u32,
    /// Days left threshold for urgent release risk detection.
    pub release_urgent_days: i64,
    /// Remaining MR count threshold for urgent release risk detection.
    pub release_urgent_remaining: usize,
    /// Days left threshold for soon release risk detection.
    pub release_soon_days: i64,
    /// Remaining MR count threshold for soon release risk detection.
    pub release_soon_remaining: usize,
    /// Maximum number of release summaries displayed in the cockpit.
    pub max_release_summaries: usize,
}

impl Default for CockpitThresholds {
    fn default() -> Self {
        Self {
            stale_days: 7,
            old_open_warning_days: 7,
            old_open_alert_days: 14,
            due_soon_days: 7,
            complex_score: 0.66,
            many_commits: 10,
            many_files: 20,
            hot_threads: 10,
            release_urgent_days: 3,
            release_urgent_remaining: 2,
            release_soon_days: 7,
            release_soon_remaining: 5,
            max_release_summaries: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelColorConfig {
    pub bg: String,
    pub fg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
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
    /// Controls whether the operational cockpit pane is displayed when enough vertical space is available.
    #[serde(default = "default_show_cockpit")]
    pub show_cockpit: bool,
    /// Thresholds used by the operational cockpit metrics and release risk detection.
    #[serde(default)]
    pub cockpit_thresholds: CockpitThresholds,
    /// Controls which optional columns are visible in the MR table.
    /// All columns are hidden by default — enable them individually in config.json.
    #[serde(default)]
    pub visible_columns: VisibleColumns,
    /// Tech-stack calibration for the review-difficulty score.
    ///
    /// Preset examples (set `complexity_profile` in config.json):
    /// - Drupal:  easy_threshold: 300, hard_threshold: 2000  (lots of YAML/config)
    /// - Java:    easy_threshold: 100, hard_threshold:  600  (logic-dense lines)
    /// - Generic: easy_threshold: 200, hard_threshold: 1000  (default)
    ///
    /// `diff_difficulty_profile` is accepted as a legacy alias for seamless migration.
    #[serde(default, alias = "diff_difficulty_profile")]
    pub complexity_profile: crate::models::DifficultyProfile,
    /// GitLab-side label colours fetched at startup — maps lowercase label name → hex colour.
    /// Populated at runtime via `AppEvent::GitlabLabelsLoaded`; never read from config.json.
    /// Not serialised — always re-fetched from the GitLab API on startup.
    #[serde(skip)]
    pub gitlab_label_colors: HashMap<String, String>,
    /// GitLab username of the currently logged-in user for this project
    /// (e.g. `"jdoe"` — without the `@` prefix).
    ///
    /// Populated at startup from `projects.toml` via `apply_project_overrides`.
    /// When `Some`, the **"Assigned to me"** and **"Reviewer: me"** filter entries
    /// are shown in the filter picker popup. When `None`, those entries are hidden.
    ///
    /// Not serialised — always sourced from `projects.toml` at runtime.
    #[serde(skip)]
    pub gitlab_username: Option<String>,
    /// `label_colors` parsed once on first use, instead of re-parsing every colour
    /// on every frame. `label_colors` is only set during startup (`main.rs`),
    /// before the first render.
    #[serde(skip)]
    pub(crate) resolved_label_colors: std::sync::OnceLock<ResolvedLabelColors>,
}

/// Pre-parsed `label_colors`: exact keys plus wildcard prefixes, longest prefix
/// first so the most specific wildcard wins deterministically.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedLabelColors {
    exact: HashMap<String, (Color, Color)>,
    prefixes: Vec<(String, (Color, Color))>,
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
            refresh_interval_secs: Some(900),
            default_branches: default_branches_val(),
            table_label_prefixes: vec!["deploy::".to_string(), "review::".to_string()],
            label_colors,
            activity_stale_days: default_activity_stale_days(),
            activity_recent_days: default_activity_recent_days(),
            show_cockpit: default_show_cockpit(),
            cockpit_thresholds: CockpitThresholds::default(),
            visible_columns: VisibleColumns::default(),
            complexity_profile: crate::models::DifficultyProfile::default(),
            // Populated at runtime by AppEvent::GitlabLabelsLoaded — always starts empty.
            gitlab_label_colors: HashMap::new(),
            // Populated at runtime from projects.toml via apply_project_overrides — None by default.
            gitlab_username: None,
            resolved_label_colors: std::sync::OnceLock::new(),
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

    /// Resolves the background and foreground colours for a label chip.
    ///
    /// Priority:
    /// 1. Exact match in `config.json` `label_colors`
    /// 2. Wildcard prefix match in `config.json` (e.g. `"deploy::*"`)
    /// 3. GitLab-side colour passed as `gitlab_color` (hex, e.g. `"#6699cc"`) with a
    ///    computed foreground (black or white depending on luminance)
    /// 4. Default dark-gray background with white foreground
    pub fn get_label_style(&self, label: &str, gitlab_color: Option<&str>) -> (Color, Color) {
        let label_lower = label.to_lowercase();
        let resolved = self.resolved_label_colors.get_or_init(|| {
            let mut resolved = ResolvedLabelColors::default();
            for (key, cfg) in &self.label_colors {
                let style = (parse_color(&cfg.bg), parse_color(&cfg.fg));
                match key.strip_suffix('*') {
                    Some(prefix) => resolved.prefixes.push((prefix.to_lowercase(), style)),
                    None => {
                        resolved.exact.insert(key.clone(), style);
                    }
                }
            }
            resolved
                .prefixes
                .sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
            resolved
        });

        // 1. Exact match override from config.json
        if let Some(style) = resolved.exact.get(&label_lower) {
            return *style;
        }

        // 2. Wildcard prefix override from config.json
        if let Some((_, style)) = resolved
            .prefixes
            .iter()
            .find(|(prefix, _)| label_lower.starts_with(prefix.as_str()))
        {
            return *style;
        }

        // 3. GitLab-provided colour (hex) — compute a legible foreground automatically
        if let Some(hex) = gitlab_color {
            let bg = parse_color(hex);
            let fg = auto_foreground(&bg);
            return (bg, fg);
        }

        // 4. Generic fallback
        (Color::DarkGray, Color::White)
    }
}

/// Picks black or white as foreground based on the perceived luminance of `bg`.
///
/// Uses the W3C relative luminance formula (sRGB linearisation) so the chip
/// text remains legible on any GitLab badge colour without manual override.
fn auto_foreground(bg: &Color) -> Color {
    let (r, g, b) = match bg {
        Color::Rgb(r, g, b) => (*r, *g, *b),
        Color::Black => (0, 0, 0),
        Color::White => (255, 255, 255),
        Color::Red => (128, 0, 0),
        Color::Green => (0, 128, 0),
        Color::Blue => (0, 0, 128),
        Color::Yellow => (128, 128, 0),
        Color::Cyan => (0, 128, 128),
        Color::Magenta => (128, 0, 128),
        _ => return Color::White,
    };
    // sRGB linearisation then luminance (ITU-R BT.709)
    let linearise = |c: u8| {
        let f = c as f32 / 255.0;
        if f <= 0.04045 {
            f / 12.92
        } else {
            ((f + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linearise(r) + 0.7152 * linearise(g) + 0.0722 * linearise(b);
    if luminance > 0.179 {
        Color::Black
    } else {
        Color::White
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
            if s.is_ascii() && s.starts_with('#') && s.len() == 7 {
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
