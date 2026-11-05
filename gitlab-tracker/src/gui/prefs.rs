//! The window's own preferences, kept between sessions in `gui.json` next to
//! `projects.toml`: the page, the theme, the table's grouping and density, the
//! tracker's board, the cockpit's zeros, the inspector's folded sections, the
//! window's size and its open tabs. The demo neither reads nor writes them.

use serde::{Deserialize, Serialize};

use super::{table::GroupBy, GuiApp, Page};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub page: Option<String>,
    pub light: bool,
    pub group_by: Option<String>,
    pub compact: bool,
    pub tracker_board: bool,
    pub hide_zeros: bool,
    /// The table's Next step column is hidden.
    pub hide_next_column: bool,
    /// The table's branches in a column each (default: one flow cell).
    pub branch_columns: bool,
    pub folded_sections: Vec<String>,
    /// Inner size in points.
    pub window: Option<[f32; 2]>,
    /// The projects open in tabs, in order.
    pub tabs: Vec<SavedTab>,
}

/// An open tab: its project's GitLab URL and id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    pub id: String,
}

fn path() -> Option<std::path::PathBuf> {
    crate::storage::get_save_dir().map(|dir| dir.join("gui.json"))
}

/// The saved preferences; the defaults when there are none or they do not parse.
pub fn load() -> Prefs {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(prefs: &Prefs) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(prefs) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text) {
                tracing::warn!(error = %e, path = %path.display(), "Could not save the window preferences");
            }
        }
        Err(e) => tracing::warn!(error = %e, "Could not serialise the window preferences"),
    }
}

fn page_name(page: Page) -> &'static str {
    match page {
        Page::Today => "today",
        Page::Promote => "promote",
        Page::MergeRequests | Page::Mr => "mrs",
        Page::Cockpit => "cockpit",
        Page::Tracker => "tracker",
        #[cfg(feature = "stats")]
        Page::Stats => "stats",
        Page::Team => "team",
        Page::Journal => "journal",
        Page::Environments => "environments",
        Page::Ci => "ci",
        Page::Milestones => "milestones",
        Page::Overlaps => "overlaps",
    }
}

fn group_name(group: GroupBy) -> &'static str {
    match group {
        GroupBy::None => "none",
        GroupBy::Status => "status",
        GroupBy::Milestone => "milestone",
        GroupBy::Author => "author",
        GroupBy::NextStep => "next_step",
    }
}

/// Applies `prefs` to the window just opened.
pub fn apply(g: &mut GuiApp, prefs: &Prefs) {
    g.page = match prefs.page.as_deref() {
        Some("cockpit") => Page::Cockpit,
        Some("tracker") => Page::Tracker,
        Some("today") => Page::Today,
        Some("promote") => Page::Promote,
        Some("team") => Page::Team,
        Some("journal") => Page::Journal,
        Some("environments") => Page::Environments,
        Some("ci") => Page::Ci,
        Some("milestones") => Page::Milestones,
        Some("overlaps") => Page::Overlaps,
        // The stats open through their action once the window runs.
        _ => Page::MergeRequests,
    };
    #[cfg(feature = "stats")]
    if prefs.page.as_deref() == Some("stats") {
        g.open_page(Page::Stats);
    }
    if prefs.light {
        g.set_theme(bevel_theme::Mode::Light);
    }
    g.group_by = match prefs.group_by.as_deref() {
        Some("status") => GroupBy::Status,
        Some("milestone") => GroupBy::Milestone,
        Some("author") => GroupBy::Author,
        Some("next_step") => GroupBy::NextStep,
        _ => GroupBy::None,
    };
    g.compact = prefs.compact;
    g.tracker_board = prefs.tracker_board;
    g.hide_zeros = prefs.hide_zeros;
    g.hide_next_column = prefs.hide_next_column;
    g.flow_cells = !prefs.branch_columns;
    *super::widgets::folded_sections() = prefs.folded_sections.iter().cloned().collect();
}

/// The window's preferences as they are now.
pub fn current(g: &GuiApp) -> Prefs {
    Prefs {
        page: Some(page_name(g.page()).to_string()),
        light: g.theme.mode == bevel_theme::Mode::Light,
        group_by: Some(group_name(g.group_by).to_string()),
        compact: g.compact,
        tracker_board: g.tracker_board,
        hide_zeros: g.hide_zeros,
        hide_next_column: g.hide_next_column,
        branch_columns: !g.flow_cells,
        folded_sections: super::widgets::folded_sections().iter().cloned().collect(),
        window: g.window_size.map(|s| [s.x, s.y]),
        tabs: g
            .tabs
            .iter()
            .filter(|t| !matches!(t.state, super::SlotState::Closed))
            .map(|t| SavedTab {
                url: t.url.clone(),
                id: t.id.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_tolerate_missing_fields() {
        let prefs = Prefs {
            page: Some("cockpit".into()),
            light: true,
            group_by: Some("status".into()),
            compact: true,
            tracker_board: true,
            hide_zeros: true,
            hide_next_column: false,
            branch_columns: false,
            folded_sections: vec!["History".into()],
            window: Some([1600.0, 1000.0]),
            tabs: vec![SavedTab {
                url: "https://g.io".into(),
                id: "42".into(),
            }],
        };
        let text = serde_json::to_string(&prefs).unwrap();
        let back: Prefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back.page.as_deref(), Some("cockpit"));
        assert_eq!(back.window, Some([1600.0, 1000.0]));
        assert_eq!(back.tabs, prefs.tabs);
        // An older or hand-edited file: the rest takes its default.
        let partial: Prefs = serde_json::from_str(r#"{"compact": true}"#).unwrap();
        assert!(partial.compact && !partial.light && partial.page.is_none());
    }
}
