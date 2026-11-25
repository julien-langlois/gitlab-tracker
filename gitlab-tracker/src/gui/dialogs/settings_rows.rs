//! The settings as rows: the window's first, then the project's by section; their text, search match and state.

use super::*;

/// Settings the GUI leaves out: the cockpit has its own page in the icon rail,
/// where the TUI shows it as a pane. Their saved values stay as they are.
pub(super) const GUI_HIDDEN_SETTINGS: &[&str] = &["core.show_cockpit"];

/// Width of the settings' side tabs.
pub(super) const NAV_WIDTH: f32 = 216.0;

/// Width kept on the right of a setting row for its control.
pub(super) const CONTROL_WIDTH: f32 = 180.0;

/// A tab of the settings: a section, and its rows.
pub(super) struct Section {
    /// The section's name in the settings' definitions (`Core`, `Stats`…);
    /// `Interface` for the window's own settings.
    pub(super) key: &'static str,
    pub(super) rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Row {
    /// The window's language.
    Language,
    /// The window's theme.
    Theme,
    /// Look for newer versions.
    UpdateCheck,
    /// A project setting, by its index in the settings editor.
    Item(usize),
}

/// The window's settings first, then the project's by section, in the order
/// of their definitions.
pub(super) fn sections(g: &GuiApp) -> Vec<Section> {
    let mut sections = vec![Section {
        key: "Interface",
        rows: vec![Row::Language, Row::Theme, Row::UpdateCheck],
    }];
    for (i, item) in g.app.settings_editor.items.iter().enumerate() {
        if GUI_HIDDEN_SETTINGS.contains(&item.def.id) {
            continue;
        }
        match sections.iter_mut().find(|s| s.key == item.def.section) {
            Some(section) => section.rows.push(Row::Item(i)),
            None => sections.push(Section {
                key: item.def.section,
                rows: vec![Row::Item(i)],
            }),
        }
    }
    // The project's own first; the plug-ins' (statistics…) last.
    const ORDER: &[&str] = &[
        "Interface",
        "Core",
        "Signed commits",
        "Delivery",
        "Releases",
        "Redmine",
        "Stats",
    ];
    sections.sort_by_key(|s| {
        ORDER
            .iter()
            .position(|k| *k == s.key)
            .unwrap_or(ORDER.len() - 1)
    });
    sections
}

pub(super) fn section_title(key: &str) -> &str {
    tl(if key == "Core" { "General" } else { key })
}

pub(super) fn section_icon(key: &str) -> Icon {
    match key {
        "Interface" => Icon::PanelLeft,
        "Core" => Icon::Home,
        "Signed commits" => Icon::Shield,
        "Delivery" => Icon::Gauge,
        "Releases" => Icon::Tag,
        "Redmine" => Icon::Ticket,
        #[cfg(feature = "stats")]
        "Stats" => Icon::Stats,
        _ => Icon::Settings,
    }
}

/// The line under a section's title.
pub(super) fn section_about(key: &str) -> Option<&str> {
    let about = match key {
        "Interface" => "This window's look. Not part of the project's settings.",
        "Core" => "What the project tracks, and how often it refreshes.",
        "Signed commits" => "Warn about the commits of the open MRs that are not signed.",
        "Delivery" => "Hotfixes and deploy freezes.",
        "Releases" => "The draft release notes.",
        "Stats" => "The local statistics: how long they are kept, how sprints are cut.",
        _ => return None,
    };
    Some(tl(about))
}

pub(super) const UPDATE_CHECK_HELP: &str =
    "Once a day, look on GitHub for a newer release and say so in \
     the title bar. Applied at once; nothing is downloaded.";

/// A row's name and help, in the window's language.
pub(super) fn row_text(g: &GuiApp, row: Row) -> (&'static str, &'static str) {
    match row {
        Row::Language => (
            tl("Language"),
            tl("Language of this window. The terminal UI stays in English."),
        ),
        Row::Theme => (
            tl("Theme"),
            tl("Applied at once, as with the title bar's button."),
        ),
        Row::UpdateCheck => (tl("New versions"), tl(UPDATE_CHECK_HELP)),
        Row::Item(i) => {
            let def = &g.app.settings_editor.items[i].def;
            (tl(def.label), tl(def.help))
        }
    }
}

/// Whether a row answers the search `query` (lowercase): in its name or help,
/// translated or in English.
pub(super) fn row_matches(g: &GuiApp, row: Row, query: &str) -> bool {
    let (label, help) = row_text(g, row);
    let english = match row {
        Row::Item(i) => {
            let def = &g.app.settings_editor.items[i].def;
            [def.label, def.help]
        }
        Row::Language => ["Language", ""],
        Row::Theme => ["Theme", ""],
        Row::UpdateCheck => ["New versions", UPDATE_CHECK_HELP],
    };
    [label, help, english[0], english[1]]
        .iter()
        .any(|text| text.to_lowercase().contains(query))
}

/// Whether a row differs from what the dialog opened with.
pub(super) fn row_modified(g: &GuiApp, row: Row) -> bool {
    match row {
        Row::Language => g.language_draft != g.language,
        Row::Theme | Row::UpdateCheck => false,
        Row::Item(i) => g.app.settings_editor.is_modified(i),
    }
}

/// A switch's value, by setting id.
pub(super) fn bool_setting(g: &GuiApp, id: &str) -> Option<bool> {
    g.app
        .settings_editor
        .items
        .iter()
        .find(|item| item.def.id == id)
        .and_then(|item| match item.value {
            ProjectSettingValue::Bool(on) => Some(on),
            _ => None,
        })
}

/// Whether a row can be changed now, and whether it hangs under the row above:
/// the signatures' options wait on "Enforce signed commits", "…only with the
/// same changes" also on the rebases' tolerance.
pub(super) fn row_state(g: &GuiApp, row: Row) -> (bool, bool) {
    let Row::Item(i) = row else {
        return (true, false);
    };
    let id = g.app.settings_editor.items[i].def.id;
    if !id.starts_with("signed_commits.") || id == "signed_commits.enforce" {
        return (true, false);
    }
    let enforced = bool_setting(g, "signed_commits.enforce").unwrap_or(true);
    if id == "signed_commits.require_same_patch" {
        let tolerated = bool_setting(g, "signed_commits.tolerate_gitlab_rebase").unwrap_or(true);
        (enforced && tolerated, true)
    } else {
        (enforced, false)
    }
}
