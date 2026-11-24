//! The window's own preferences, kept between sessions in `tracker.db` (the
//! `gui` setting; `gui.json` before, imported on first use): the page, the
//! theme, the table's grouping and density, the tracker's board, the cockpit's
//! zeros, the inspector's folded sections, the window's size and its open tabs.
//! The demo neither reads nor writes them.

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
    /// The export's format and options, for the next export and its shortcut.
    pub export: super::export::Settings,
    /// How the stand-up is written, and when it was last copied.
    pub standup: super::standup::Settings,
    /// Inner size in points.
    pub window: Option<[f32; 2]>,
    /// The projects open in tabs, in order.
    pub tabs: Vec<SavedTab>,
    /// No look for newer versions (Settings › Interface).
    pub no_update_check: bool,
}

/// An open tab: its project's GitLab URL and id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    pub id: String,
}

/// The saved preferences (in `tracker.db`, `gui.json` imported on first use);
/// the defaults when there are none or they do not parse.
pub fn load() -> Prefs {
    super::block_on(crate::storage::load_setting("gui", "gui.json")).unwrap_or_default()
}

pub fn save(prefs: &Prefs) {
    super::block_on(crate::storage::save_setting("gui", prefs));
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
        Page::Team | Page::Person => "team",
        Page::Journal => "journal",
        Page::Environments => "environments",
        Page::Ci => "ci",
        Page::Milestones => "milestones",
        Page::Overlaps => "overlaps",
        Page::Delivery => "delivery",
        Page::Releases => "releases",
        Page::Stacks => "stacks",
    }
}

fn group_name(group: GroupBy) -> &'static str {
    match group {
        GroupBy::None => "none",
        GroupBy::Status => "status",
        GroupBy::Milestone => "milestone",
        GroupBy::Author => "author",
        GroupBy::NextStep => "next_step",
        GroupBy::MergedDay => "merged_day",
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
        Some("delivery") => Page::Delivery,
        Some("releases") => Page::Releases,
        Some("stacks") => Page::Stacks,
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
    g.update_check = !prefs.no_update_check;
    g.group_by = match prefs.group_by.as_deref() {
        Some("status") => GroupBy::Status,
        Some("milestone") => GroupBy::Milestone,
        Some("author") => GroupBy::Author,
        Some("next_step") => GroupBy::NextStep,
        Some("merged_day") => GroupBy::MergedDay,
        _ => GroupBy::None,
    };
    g.compact = prefs.compact;
    g.tracker_board = prefs.tracker_board;
    g.hide_zeros = prefs.hide_zeros;
    g.hide_next_column = prefs.hide_next_column;
    g.flow_cells = !prefs.branch_columns;
    g.export.settings = prefs.export;
    g.standup = prefs.standup;
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
        export: g.export.settings,
        standup: g.standup,
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
        no_update_check: !g.update_check,
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
            export: super::super::export::Settings::default(),
            standup: super::super::standup::Settings {
                period: super::super::standup::Period::Week,
                hidden: 3,
                last_copied: Some(1_764_000_000),
                ..Default::default()
            },
            window: Some([1600.0, 1000.0]),
            tabs: vec![SavedTab {
                url: "https://g.io".into(),
                id: "42".into(),
            }],
            no_update_check: true,
        };
        let text = serde_json::to_string(&prefs).unwrap();
        let back: Prefs = serde_json::from_str(&text).unwrap();
        assert_eq!(back.page.as_deref(), Some("cockpit"));
        assert_eq!(back.window, Some([1600.0, 1000.0]));
        assert_eq!(back.tabs, prefs.tabs);
        assert_eq!(back.standup, prefs.standup);
        assert!(back.no_update_check);
        // An older or hand-edited file: the rest takes its default.
        let partial: Prefs = serde_json::from_str(r#"{"compact": true}"#).unwrap();
        assert!(partial.compact && !partial.light && partial.page.is_none());
        assert!(!partial.no_update_check, "new versions are looked for by default");
    }
}
