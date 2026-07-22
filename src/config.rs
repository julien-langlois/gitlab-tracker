use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_branches_val() -> Vec<String> {
    vec!["main".to_string()]
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
        "gray" | "grey" | "dark_gray" => Color::DarkGray,
        "light_gray" => Color::Gray,
        _ => {
            if s.starts_with('#') && s.len() == 7 {
                let r = u8::from_str_radix(&s[1..3], 16).unwrap_or(128);
                let g = u8::from_str_radix(&s[3..5], 16).unwrap_or(128);
                let b = u8::from_str_radix(&s[5..7], 16).unwrap_or(128);
                Color::Rgb(r, g, b)
            } else {
                Color::DarkGray
            }
        }
    }
}
