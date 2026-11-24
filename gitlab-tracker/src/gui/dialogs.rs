//! Dialogs, shown for the app's popup modes like the TUI's popups: settings,
//! columns, project picker, log time and keyboard shortcuts.
//!
//! They share one shell: a header (the dialog's icon on a tile, its title and
//! a line about it, a close button), the body, then a footer holding the
//! buttons on the panel's color.

use bevel_theme::Theme;
use egui::{
    Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2,
};
use gitlab_tracker_core::{ProjectSettingKind, ProjectSettingValue};

use super::board::one_line;
use super::icons::{self, Icon};
use super::table;
use super::tip::TipExt;
use super::widgets::{button, focus_ring, muted, primary_button, rule, semibold, switch};
use super::GuiApp;
use crate::actions::Action;
use crate::app::{InputMode, LogTimeForm};
use crate::i18n::{tf, tl, tn};

pub fn show(g: &mut GuiApp, ctx: &egui::Context) {
    if g.wizard.is_some() {
        wizard(g, ctx);
        return;
    }
    // Wide where the window allows: the settings and the shortcuts read in
    // columns, the column picker as a grid of cards.
    let room = ctx.content_rect().width() - 96.0;
    let (icon, title, width): (Icon, &str, f32) = match g.app.input_mode {
        InputMode::Settings => (Icon::Settings, tl("Settings"), room.clamp(560.0, 940.0)),
        InputMode::ColumnPicker => (Icon::Columns, tl("Columns"), room.clamp(480.0, 700.0)),
        InputMode::ProjectPicker => (Icon::Home, tl("Projects"), room.clamp(440.0, 560.0)),
        InputMode::LogTime => (Icon::Clock, tl("Log time"), room.clamp(400.0, 480.0)),
        InputMode::Help => (
            Icon::Help,
            tl("Keyboard shortcuts"),
            room.clamp(520.0, 1040.0),
        ),
        _ => return,
    };
    let subtitle = subtitle(g);
    let theme = g.theme;
    // A pixel of margin keeps the frame's border clear of the footer's fill.
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(1));
    let mut closed = false;
    let response = egui::Modal::new(egui::Id::unique("dialog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            closed = header(ui, &theme, icon, title, subtitle.as_deref());
            rule(ui, &theme);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing.item.y;
                match g.app.input_mode {
                    InputMode::Settings => settings(g, ui),
                    InputMode::ColumnPicker => columns(g, ui),
                    InputMode::ProjectPicker => projects(g, ui),
                    InputMode::LogTime => log_time(g, ui),
                    InputMode::Help => help(g, ui),
                    _ => {}
                }
            });
        });
    // The close button, Escape or a click outside: cancel, as Esc does in the TUI.
    if closed || response.should_close() {
        close(g);
    }
}

/// The line under the dialog's title.
fn subtitle(g: &GuiApp) -> Option<String> {
    match g.app.input_mode {
        InputMode::Settings => Some(tf(
            "{project} · saved to projects.toml",
            &[("project", &super::shell::project_name(g))],
        )),
        InputMode::ColumnPicker => Some(tl("What the merge request table shows").to_string()),
        InputMode::ProjectPicker => Some(
            tl("Each project opens in its own tab, and keeps refreshing while another one is shown.")
                .to_string(),
        ),
        InputMode::LogTime => g
            .app
            .selected_mr()
            .and_then(|mr| mr.linked_ticket.as_ref())
            .map(|t| format!("#{} — {}", t.id, t.subject)),
        _ => None,
    }
}

/// Closes the dialog without applying it (the columns are saved as they change).
fn close(g: &mut GuiApp) {
    match g.app.input_mode {
        InputMode::Settings => {
            g.app.settings_editor.cancel();
            g.language_draft = g.language.clone();
            g.settings_search.clear();
        }
        InputMode::ColumnPicker => save_columns(g),
        InputMode::LogTime => g.app.log_time_form = LogTimeForm::default(),
        _ => {}
    }
    g.app.input_mode = InputMode::Normal;
}

/// The dialog's header: its icon on an accent tile, the title over `subtitle`,
/// and the close button. Whether the close button was clicked.
pub(super) fn header(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    title: &str,
    subtitle: Option<&str>,
) -> bool {
    let mut close = false;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 20,
            right: 14,
            top: 16,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                let radius = CornerRadius::same(theme.radii.large);
                ui.painter()
                    .rect_filled(tile, radius, theme.palette.accent.gamma_multiply(0.14));
                ui.painter().rect_stroke(
                    tile,
                    radius,
                    Stroke::new(1.0, theme.palette.accent.gamma_multiply(0.3)),
                    StrokeKind::Inside,
                );
                icons::paint(ui.painter(), tile.shrink(10.0), icon, theme.palette.accent);
                let text_width = ui.available_width() - 44.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(text_width, 40.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(text_width);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        // Centered on the tile: one line or two.
                        let lines = if subtitle.is_some() { 38.0 } else { 20.0 };
                        ui.add_space(((40.0 - lines) / 2.0_f32).max(0.0));
                        ui.label(semibold(title, theme.text_size + 4.0));
                        if let Some(subtitle) = subtitle {
                            ui.add(egui::Label::new(muted(theme, subtitle)).wrap());
                        }
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = format!("{} (Esc)", tl("Close"));
                    close = super::widgets::icon_button(
                        ui,
                        &label,
                        Icon::Close,
                        &theme.icon_button(),
                        30.0,
                    )
                    .clicked();
                });
            });
        });
    close
}

/// The dialog's body, with its margins.
pub(super) fn body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(20, 18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The dialog's footer, on the panel's color under a rule: `add` lays out its
/// line from the left; the buttons go in a right-to-left layout at its end.
pub(super) fn footer(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui)) {
    rule(ui, theme);
    let radius = theme.radii.large.saturating_sub(1);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .corner_radius(CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius,
            se: radius,
        })
        .inner_margin(Margin::symmetric(20, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(theme.spacing.control_height + 2.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                add(ui);
            });
        });
}

/// The buttons at the footer's right end, the first added rightmost.
fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui);
    });
}

/// A group's caption: small capitals, muted.
fn caption(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(semibold(text.to_uppercase(), theme.text_size - 2.5).color(theme.palette.text_muted));
    ui.add_space(4.0);
}

/// A card of rows: the panel's fill, a border, rounded.
fn card<R>(ui: &mut Ui, theme: &Theme, rows: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            rows(ui)
        })
        .inner
}

/// A small dot in `color`, `size` across, with `tip` on hover.
fn tip_dot(ui: &mut Ui, color: Color32, size: f32, tip: &str) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size + 4.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    response.tip(tip);
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// Settings the GUI leaves out: the cockpit has its own page in the icon rail,
/// where the TUI shows it as a pane. Their saved values stay as they are.
const GUI_HIDDEN_SETTINGS: &[&str] = &["core.show_cockpit"];

/// Width of the settings' side tabs.
const NAV_WIDTH: f32 = 216.0;

/// Width kept on the right of a setting row for its control.
const CONTROL_WIDTH: f32 = 180.0;

/// A tab of the settings: a section, and its rows.
struct Section {
    /// The section's name in the settings' definitions (`Core`, `Stats`…);
    /// `Interface` for the window's own settings.
    key: &'static str,
    rows: Vec<Row>,
}

#[derive(Clone, Copy, PartialEq)]
enum Row {
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
fn sections(g: &GuiApp) -> Vec<Section> {
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

fn section_title(key: &str) -> &str {
    tl(if key == "Core" { "General" } else { key })
}

fn section_icon(key: &str) -> Icon {
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
fn section_about(key: &str) -> Option<&str> {
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

const UPDATE_CHECK_HELP: &str = "Once a day, look on GitHub for a newer release and say so in \
     the title bar. Applied at once; nothing is downloaded.";

/// A row's name and help, in the window's language.
fn row_text(g: &GuiApp, row: Row) -> (&'static str, &'static str) {
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
fn row_matches(g: &GuiApp, row: Row, query: &str) -> bool {
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
fn row_modified(g: &GuiApp, row: Row) -> bool {
    match row {
        Row::Language => g.language_draft != g.language,
        Row::Theme | Row::UpdateCheck => false,
        Row::Item(i) => g.app.settings_editor.is_modified(i),
    }
}

/// A switch's value, by setting id.
fn bool_setting(g: &GuiApp, id: &str) -> Option<bool> {
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
fn row_state(g: &GuiApp, row: Row) -> (bool, bool) {
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

/// The settings, a tab per section on the left: the window's own first, then
/// the project's. A search goes through every section at once.
fn settings(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    ui.spacing_mut().item_spacing.y = 0.0;
    let sections = sections(g);
    if !sections.iter().any(|s| s.key == g.settings_tab) {
        g.settings_tab = sections[0].key.to_string();
    }
    let query = g.settings_search.trim().to_lowercase();
    // One height for every tab: the dialog stays put while they change.
    let height = (ui.ctx().content_rect().height() - 260.0).clamp(300.0, 560.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let nav_rect = Rect::from_min_size(rect.min, Vec2::new(NAV_WIDTH, height));
    ui.painter().rect_filled(nav_rect, 0.0, theme.palette.panel);
    ui.painter().vline(
        nav_rect.right() - 0.5,
        nav_rect.y_range(),
        Stroke::new(1.0, theme.palette.border_soft),
    );
    let mut nav_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(nav_rect.shrink2(Vec2::new(10.0, 12.0)))
            .layout(Layout::top_down(Align::Min)),
    );
    settings_nav(g, &mut nav_ui, &sections, &query);
    let content_rect = Rect::from_min_max(egui::pos2(nav_rect.right(), rect.top()), rect.max);
    let mut content_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(content_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt(("settings_page", g.settings_tab.clone(), query.is_empty()))
        .auto_shrink(false)
        .max_height(height)
        .show(&mut content_ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 26,
                    right: 26,
                    top: 20,
                    bottom: 22,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if query.is_empty() {
                        settings_page(g, ui, &sections);
                    } else {
                        settings_found(g, ui, &sections, &query);
                    }
                });
        });

    let changes = sections
        .iter()
        .flat_map(|s| &s.rows)
        .filter(|row| row_modified(g, **row))
        .count();
    footer(ui, &theme, |ui| {
        if changes > 0 {
            tip_dot(ui, theme.tones.amber.fg, 7.0, tl("Changed, not saved yet"));
            ui.label(
                RichText::new(tn(
                    changes as u64,
                    "{n} unsaved change",
                    "{n} unsaved changes",
                ))
                .color(theme.palette.text_secondary),
            );
        } else {
            ui.label(muted(&theme, tl("Esc to cancel")));
        }
        buttons(ui, |ui| {
            if button(ui, &theme, None, tl("Save"), true).clicked() {
                g.pending.push(Action::SaveSettings);
                g.save_language();
                g.settings_search.clear();
                g.app.input_mode = InputMode::Normal;
            }
            if button(ui, &theme, None, tl("Cancel"), false).clicked() {
                close(g);
            }
        });
    });
}

/// The settings' side: the search, then a tab per section. A section with
/// unsaved changes carries a dot; while searching, each tab counts its matches.
fn settings_nav(g: &mut GuiApp, ui: &mut Ui, sections: &[Section], query: &str) {
    let theme = g.theme;
    let active = !g.settings_search.is_empty();
    let search = super::widgets::field(
        ui,
        &theme,
        &mut g.settings_search,
        "settings_search",
        super::widgets::FieldLook {
            hint: tl("Search settings"),
            icon: Some(Icon::Search),
            width: ui.available_width(),
            active,
        },
    );
    // Escape in the field empties it before it closes the dialog.
    if search.has_focus()
        && active
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        g.settings_search.clear();
    }
    ui.add_space(12.0);
    let mut picked = None;
    for section in sections {
        let found = if query.is_empty() {
            section.rows.len()
        } else {
            section
                .rows
                .iter()
                .filter(|row| row_matches(g, **row, query))
                .count()
        };
        let modified = section.rows.iter().any(|row| row_modified(g, *row));
        let selected = query.is_empty() && section.key == g.settings_tab;
        let title = section_title(section.key);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let response = bevel_widgets::tabs::Tab::new(
            ui.scope_id().with(("settings_tab", section.key)),
            title,
            selected,
        )
        .focus_stroke(Stroke::new(2.0, theme.palette.accent))
        .show_at(ui, rect, |ui, response| {
            let p = &theme.palette;
            let painter = ui.painter();
            let radius = CornerRadius::same(theme.radii.medium);
            if selected {
                painter.rect_filled(rect, radius, p.selection);
                painter.rect_filled(
                    Rect::from_min_size(
                        rect.left_top() + Vec2::new(0.0, 9.0),
                        Vec2::new(3.0, rect.height() - 18.0),
                    ),
                    CornerRadius::same(2),
                    p.accent,
                );
            } else if response.hovered() {
                painter.rect_filled(rect, radius, p.raised);
            }
            let dim = !query.is_empty() && found == 0;
            let color = if selected || (response.hovered() && !dim) {
                p.text
            } else if dim {
                p.text_muted.gamma_multiply(0.6)
            } else {
                p.text_secondary
            };
            let icon_color = if selected { p.accent } else { color };
            let icon_rect = Rect::from_center_size(
                egui::pos2(rect.left() + 21.0, rect.center().y),
                Vec2::splat(16.0),
            );
            icons::paint(painter, icon_rect, section_icon(section.key), icon_color);
            // On the right: the matches while searching, a dot for unsaved changes.
            let mut right = rect.right() - 10.0;
            if !query.is_empty() && found > 0 {
                let galley = painter.layout_no_wrap(
                    found.to_string(),
                    bevel_theme::fonts::medium(theme.text_size - 2.0),
                    p.text_muted,
                );
                right -= galley.size().x;
                painter.galley(
                    egui::pos2(right, rect.center().y - galley.size().y / 2.0),
                    galley,
                    p.text_muted,
                );
                right -= 8.0;
            }
            if modified {
                painter.circle_filled(
                    egui::pos2(right - 3.5, rect.center().y),
                    3.5,
                    theme.tones.amber.fg,
                );
                right -= 15.0;
            }
            let font = if selected {
                bevel_theme::fonts::semibold(theme.text_size - 0.5)
            } else {
                bevel_theme::fonts::medium(theme.text_size - 0.5)
            };
            let left = rect.left() + 38.0;
            let galley = one_line(ui, title, font, color, right - left);
            painter.galley(
                egui::pos2(left, rect.center().y - galley.size().y / 2.0),
                galley,
                color,
            );
        });
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if modified {
            response.clone().tip(tl("Changed, not saved yet"));
        }
        if response.clicked() {
            picked = Some(section.key);
        }
        ui.add_space(2.0);
    }
    if let Some(key) = picked {
        g.settings_tab = key.to_string();
        g.settings_search.clear();
    }
}

/// The tab picked: its title, what it is about, then its rows.
fn settings_page(g: &mut GuiApp, ui: &mut Ui, sections: &[Section]) {
    let theme = g.theme;
    let Some(section) = sections.iter().find(|s| s.key == g.settings_tab) else {
        return;
    };
    ui.label(semibold(section_title(section.key), theme.text_size + 5.0));
    if let Some(about) = section_about(section.key) {
        ui.add_space(4.0);
        ui.label(muted(&theme, about));
    }
    ui.add_space(16.0);
    let rows = section.rows.clone();
    card(ui, &theme, |ui| setting_rows(g, ui, &rows));
}

/// The rows answering the search, by section.
fn settings_found(g: &mut GuiApp, ui: &mut Ui, sections: &[Section], query: &str) {
    let theme = g.theme;
    let mut any = false;
    for section in sections {
        let rows: Vec<Row> = section
            .rows
            .iter()
            .copied()
            .filter(|row| row_matches(g, *row, query))
            .collect();
        if rows.is_empty() {
            continue;
        }
        if any {
            ui.add_space(18.0);
        }
        any = true;
        caption(ui, &theme, section_title(section.key));
        ui.add_space(2.0);
        card(ui, &theme, |ui| setting_rows(g, ui, &rows));
    }
    if !any {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(semibold(tl("No setting found"), theme.text_size + 1.0));
            ui.add_space(4.0);
            ui.label(muted(
                &theme,
                tf(
                    "Nothing matches “{query}” in the settings' names and help.",
                    &[("query", g.settings_search.trim())],
                ),
            ));
        });
    }
}

/// `rows`, a rule between two.
fn setting_rows(g: &mut GuiApp, ui: &mut Ui, rows: &[Row]) {
    let theme = g.theme;
    for (j, row) in rows.iter().enumerate() {
        if j > 0 {
            rule(ui, &theme);
        }
        setting(g, ui, *row);
    }
}

/// One setting: its name and help, then its control on the right, or below
/// for a text (a list of branches, of crons…).
fn setting(g: &mut GuiApp, ui: &mut Ui, row: Row) {
    let theme = g.theme;
    let (label, help) = row_text(g, row);
    let modified = row_modified(g, row);
    let (enabled, nested) = row_state(g, row);
    let stacked = matches!(row, Row::Item(i) if g.app.settings_editor.items[i].def.kind == ProjectSettingKind::Text);
    let indent: i8 = if nested { 20 } else { 0 };
    egui::Frame::new()
        .inner_margin(Margin {
            left: 18 + indent,
            right: 16,
            top: 14,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_enabled_ui(enabled, |ui| {
                let text = |ui: &mut Ui, width: f32| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                ui.label(
                                    RichText::new(label)
                                        .font(bevel_theme::fonts::medium(theme.text_size)),
                                );
                                if modified {
                                    tip_dot(
                                        ui,
                                        theme.tones.amber.fg,
                                        6.0,
                                        tl("Changed, not saved yet"),
                                    );
                                }
                            });
                            ui.label(muted(&theme, help).size(theme.text_size - 1.5));
                        },
                    );
                };
                if stacked {
                    text(ui, ui.available_width());
                    ui.add_space(10.0);
                    control(g, ui, row);
                    return;
                }
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 20.0;
                    text(ui, ui.available_width() - control_width(g, row) - 20.0);
                    // Last, once the text set the row's height: centered on it.
                    let height = ui.min_rect().height();
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), height),
                        Layout::right_to_left(Align::Center),
                        |ui| control(g, ui, row),
                    );
                });
            });
        });
}

/// The room a row's control takes on its right: the help runs up to it.
fn control_width(g: &GuiApp, row: Row) -> f32 {
    match row {
        Row::Item(i) => match g.app.settings_editor.items[i].def.kind {
            ProjectSettingKind::Bool => 40.0,
            ProjectSettingKind::U64 { .. } => 110.0,
            ProjectSettingKind::Text => CONTROL_WIDTH,
        },
        Row::Language | Row::Theme => CONTROL_WIDTH,
        Row::UpdateCheck => 40.0,
    }
}

fn control(g: &mut GuiApp, ui: &mut Ui, row: Row) {
    let theme = g.theme;
    match row {
        Row::UpdateCheck => {
            if switch(ui, &theme, g.update_check, "").clicked() {
                g.set_update_check(!g.update_check);
            }
        }
        Row::Language => language_control(ui, &mut g.language_draft),
        Row::Theme => {
            ui.allocate_ui_with_layout(
                Vec2::new(CONTROL_WIDTH, 28.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    let selected = usize::from(theme.mode == bevel_theme::Mode::Light);
                    let labels = [tl("Dark"), tl("Light")];
                    if let Some(i) =
                        super::board::segmented(ui, &theme, "settings_theme", &labels, selected)
                    {
                        g.set_theme(if i == 1 {
                            bevel_theme::Mode::Light
                        } else {
                            bevel_theme::Mode::Dark
                        });
                    }
                },
            );
        }
        Row::Item(i) => {
            let item = &mut g.app.settings_editor.items[i];
            match (&item.def.kind, &mut item.value) {
                (ProjectSettingKind::Bool, ProjectSettingValue::Bool(on)) => {
                    if switch(ui, &theme, *on, "").clicked() {
                        *on = !*on;
                    }
                }
                (ProjectSettingKind::U64 { min, step }, ProjectSettingValue::U64(n)) => {
                    let mut value = egui::DragValue::new(n)
                        .range(*min..=u64::MAX)
                        .speed(*step as f64);
                    if let Some(unit) = unit(item.def.id) {
                        value = value.suffix(format!(" {}", tl(unit)));
                    }
                    ui.add_sized([110.0, theme.spacing.control_height], value);
                }
                (ProjectSettingKind::Text, ProjectSettingValue::Text(text)) => {
                    let mut edit = egui::TextEdit::singleline(text).margin(Margin::symmetric(8, 6));
                    if item.def.id == "core.name" {
                        edit = edit
                            .desired_width(320.0)
                            .char_limit(crate::settings_core::MAX_PROJECT_NAME_CHARS)
                            .hint_text(tl("Project id"));
                    } else {
                        // Branch patterns, crons, accounts: code, in full width.
                        edit = edit
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace);
                    }
                    ui.add(edit);
                }
                _ => {
                    ui.label(muted(&theme, "—"));
                }
            }
        }
    }
}

/// The language dropdown: the system's language first, then each catalog's.
fn language_control(ui: &mut Ui, language: &mut Option<String>) {
    let system = tf(
        "System ({language})",
        &[("language", super::i18n::system_language_name())],
    );
    let languages = super::i18n::languages();
    let selected = match language.as_deref() {
        None => system.clone(),
        Some(code) => languages
            .iter()
            .find(|(c, _)| *c == code)
            .map_or_else(|| code.to_string(), |(_, name)| name.to_string()),
    };
    egui::ComboBox::from_id_salt("language")
        .selected_text(selected)
        .width(CONTROL_WIDTH)
        .show_ui(ui, |ui| {
            ui.selectable_value(language, None, system);
            for (code, name) in languages {
                ui.selectable_value(language, Some(code.to_string()), name);
            }
        });
}

/// The unit of a number setting, from its id's suffix (`…_secs`, `…_days`…).
fn unit(id: &str) -> Option<&'static str> {
    [
        ("_secs", "s"),
        ("_days", "days"),
        ("_hours", "h"),
        ("_weeks", "weeks"),
    ]
    .into_iter()
    .find(|(suffix, _)| id.ends_with(suffix))
    .map(|(_, unit)| unit)
}

// ── Columns ──────────────────────────────────────────────────────────────────

/// What a shared column shows, by its id.
fn column_detail(id: &str) -> &'static str {
    tl(match id {
        "activity" => "Recent, slowing or stale, from its last update",
        "target_branch" => "The branch it merges into",
        "labels" => "Its GitLab labels",
        "milestone" => "The milestone it belongs to",
        "notes" => "How many comments it holds",
        "diff_stats" => "How hard it is to review, from its size",
        "commits_behind" => "The commits its target branch gained since",
        "tracker_ticket" => "Its linked ticket",
        _ => "",
    })
}

/// The column picker: a live preview of the table's head, then the columns as
/// cards to switch on and off, those shared with the terminal UI first.
fn columns(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    body(ui, |ui| {
        caption(ui, &theme, tl("Preview"));
        ui.add_space(2.0);
        preview(g, ui);
        ui.add_space(20.0);

        caption(ui, &theme, tl("Shared with the terminal UI"));
        ui.add_space(4.0);
        let defs = g.app.visible_column_defs();
        let cards: Vec<(&str, &str, bool)> = defs
            .iter()
            .map(|def| {
                (
                    tl(def.label),
                    column_detail(def.id),
                    table::shows_column(&g.app, def.id),
                )
            })
            .collect();
        if let Some(i) = toggle_cards(ui, &theme, "shared_columns", &cards) {
            toggle_column(g, defs[i].id);
        }
        ui.add_space(12.0);

        caption(ui, &theme, tl("This window only"));
        ui.add_space(4.0);
        let cards = [
            (
                tl("Next step"),
                tl("Who each MR waits on, and since when"),
                !g.hide_next_column,
            ),
            (
                tl("Branches in one cell"),
                tl("A dot per branch, joined, instead of a column each: room for the title"),
                g.flow_cells,
            ),
        ];
        match toggle_cards(ui, &theme, "window_columns", &cards) {
            Some(0) => g.hide_next_column = !g.hide_next_column,
            Some(_) => g.flow_cells = !g.flow_cells,
            None => {}
        }
    });
    footer(ui, &theme, |ui| {
        ui.label(muted(&theme, tl("Applied at once, kept for this project.")));
        buttons(ui, |ui| {
            if primary_button(ui, &theme, tl("Done")).clicked() {
                close(g);
            }
        });
    });
}

/// Shows or hides the shared column `id`. The first choice starts from the
/// columns the table shows, not from the terminal UI's defaults.
fn toggle_column(g: &mut GuiApp, id: &str) {
    if g.app.project_settings.visible_columns.is_none() {
        for def in g.app.visible_column_defs() {
            let shown = table::shows_column(&g.app, def.id);
            g.app.config.visible_columns.set_visible(def.id, shown);
        }
    }
    g.app.config.visible_columns.toggle(id);
    // From now on the table follows this choice (see `table::wanted_columns`).
    g.app.project_settings.visible_columns = Some(g.app.config.visible_columns.clone());
}

fn save_columns(g: &mut GuiApp) {
    if g.app.project_settings.visible_columns.is_some() {
        g.pending.push(Action::SaveVisibleColumns);
    }
}

/// How a column shows in the preview.
#[derive(Clone, Copy, PartialEq)]
enum Head {
    /// Always there: the MR, its title, the branches.
    Fixed,
    /// Picked, and shown.
    Shown,
    /// Picked, but the window is too narrow for it.
    Hidden,
}

/// The table's head as the table draws it now, the picked columns in the
/// accent; those the window has no room for, dashed.
fn preview(g: &GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let wanted = table::wanted_columns(&g.app, g.hide_next_column, !g.queues.is_empty());
    // The table fits its columns while drawn: elsewhere, nothing is dropped.
    let on_table = matches!(g.page(), super::Page::MergeRequests);
    let shown = |col: &table::Col| !on_table || g.shown_columns.contains(col);
    egui::Frame::new()
        .fill(theme.palette.content)
        .stroke(Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(12, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                head(ui, &theme, tl("MR"), Head::Fixed, 0.0);
                head(ui, &theme, tl("Title"), Head::Fixed, 130.0);
                for col in &wanted {
                    let kind = if shown(col) {
                        Head::Shown
                    } else {
                        Head::Hidden
                    };
                    head(ui, &theme, col.title(), kind, 0.0);
                }
                head(ui, &theme, tl("Branches"), Head::Fixed, 0.0);
            });
        });
    let hidden = wanted.iter().filter(|c| !shown(c)).count();
    if hidden > 0 {
        ui.add_space(6.0);
        ui.label(
            RichText::new(tn(
                hidden as u64,
                "{n} column hidden: the window is too narrow for it. Widen it to see it.",
                "{n} columns hidden: the window is too narrow for them. Widen it to see them.",
            ))
            .size(theme.text_size - 1.5)
            .color(theme.tones.amber.fg),
        );
    }
}

fn head(ui: &mut Ui, theme: &Theme, text: &str, kind: Head, min_width: f32) {
    let p = &theme.palette;
    let color = match kind {
        Head::Fixed => p.text_secondary,
        Head::Shown => p.accent,
        Head::Hidden => p.text_muted,
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        bevel_theme::fonts::semibold(theme.text_size - 2.0),
        color,
    );
    let size = Vec2::new((galley.size().x + 18.0).max(min_width), 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let radius = CornerRadius::same(theme.radii.medium);
    match kind {
        Head::Fixed => {
            ui.painter().rect_filled(rect, radius, p.input);
        }
        Head::Shown => {
            ui.painter()
                .rect_filled(rect, radius, p.accent.gamma_multiply(0.14));
            ui.painter().rect_stroke(
                rect,
                radius,
                Stroke::new(1.0, p.accent.gamma_multiply(0.35)),
                StrokeKind::Inside,
            );
        }
        Head::Hidden => {
            let r = rect.shrink(0.5);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            ui.painter().extend(egui::Shape::dashed_line(
                &corners,
                Stroke::new(1.0, p.border),
                4.0,
                3.0,
            ));
        }
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 9.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    if kind == Head::Hidden {
        response.tip(tl("Hidden: the window is too narrow"));
    }
}

/// `cards` (title, detail, on) two by two; the one clicked, if any.
fn toggle_cards(
    ui: &mut Ui,
    theme: &Theme,
    id: &str,
    cards: &[(&str, &str, bool)],
) -> Option<usize> {
    const GAP: f32 = 10.0;
    let width = ((ui.available_width() - GAP) / 2.0).floor();
    let mut clicked = None;
    for (r, pair) in cards.chunks(2).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (c, (title, detail, on)) in pair.iter().enumerate() {
                let index = r * 2 + c;
                if toggle_card(ui, theme, (id, index), title, detail, *on, width).clicked() {
                    clicked = Some(index);
                }
            }
        });
        ui.add_space(GAP - ui.spacing().item_spacing.y);
    }
    clicked
}

/// A card to switch on and off: its title over what it shows, and a switch.
fn toggle_card(
    ui: &mut Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    detail: &str,
    on: bool,
    width: f32,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 62.0), Sense::hover());
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), on, title)
    });
    let p = &theme.palette;
    let mut cut = false;
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool(response.id, on);
        let radius = CornerRadius::same(theme.radii.large);
        let hovered = response.hovered();
        ui.painter()
            .rect_filled(rect, radius, if hovered { p.content } else { p.panel });
        if how_on > 0.0 {
            ui.painter()
                .rect_filled(rect, radius, p.accent.gamma_multiply(0.08 * how_on));
        }
        let stroke = if on {
            p.accent.gamma_multiply(0.6)
        } else if hovered {
            p.text_muted.gamma_multiply(0.7)
        } else {
            p.border
        };
        ui.painter()
            .rect_stroke(rect, radius, Stroke::new(1.0, stroke), StrokeKind::Inside);
        let text_width = rect.width() - 14.0 - 30.0 - 28.0;
        let title_color = if on { p.text } else { p.text_secondary };
        let galley = one_line(
            ui,
            title,
            bevel_theme::fonts::medium(theme.text_size),
            title_color,
            text_width,
        );
        ui.painter().galley(
            egui::pos2(rect.left() + 14.0, rect.top() + 12.0),
            galley,
            title_color,
        );
        let font = egui::FontId::proportional(theme.text_size - 1.5);
        cut = ui
            .painter()
            .layout_no_wrap(detail.to_owned(), font.clone(), p.text_muted)
            .size()
            .x
            > text_width;
        let galley = one_line(ui, detail, font, p.text_muted, text_width);
        ui.painter().galley(
            egui::pos2(rect.left() + 14.0, rect.top() + 34.0),
            galley,
            p.text_muted,
        );
        super::widgets::switch_track(
            ui,
            theme,
            egui::pos2(rect.right() - 14.0 - 30.0, rect.center().y),
            how_on,
        );
    }
    focus_ring(ui, theme, &response, theme.radii.large);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if cut {
        response.tip(detail)
    } else {
        response
    }
}

// ── Project picker ───────────────────────────────────────────────────────────

/// The projects of `projects.toml`: a click shows its tab, opening it when
/// needed. "New project…" opens the project form.
fn projects(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let mut picked = None;
    body(ui, |ui| {
        egui::ScrollArea::vertical()
            .max_height(380.0)
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                    for (i, project) in g.app.project_picker.iter().enumerate() {
                        let current = crate::storage::is_same_project(
                            project,
                            &g.app.base_url,
                            &g.app.project_id,
                        );
                        let open = g.tab_of(&project.gitlab_url, &project.project_id).is_some();
                        let name = project.name.as_deref().unwrap_or(&project.project_id);
                        let response = egui::Frame::new()
                            .fill(if current {
                                theme.palette.selection
                            } else {
                                theme.palette.panel
                            })
                            .stroke(Stroke::new(
                                1.0,
                                if current {
                                    theme.palette.accent.gamma_multiply(0.6)
                                } else {
                                    theme.palette.border
                                },
                            ))
                            .corner_radius(CornerRadius::same(theme.radii.large))
                            .inner_margin(Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.spacing_mut().item_spacing.y = 2.0;
                                        ui.label(semibold(name, theme.text_size));
                                        ui.label(muted(
                                            &theme,
                                            format!(
                                                "{} · {}",
                                                project.gitlab_url, project.project_id
                                            ),
                                        ));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let state = if current {
                                            Some(tl("Shown"))
                                        } else if open {
                                            Some(tl("In a tab"))
                                        } else {
                                            None
                                        };
                                        if let Some(state) = state {
                                            super::widgets::badge(
                                                ui,
                                                &theme,
                                                state,
                                                theme.tones.green,
                                            );
                                        }
                                    });
                                });
                            })
                            .response
                            .interact(egui::Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() && !current {
                            picked = Some(i);
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        if g.app.project_picker.is_empty() {
            ui.label(muted(&theme, tl("No project in projects.toml.")));
        }
    });
    if let Some(entry) = picked.and_then(|i| g.app.project_picker.get(i).cloned()) {
        g.pending.push(Action::SwitchProject(Box::new(entry)));
        g.app.input_mode = InputMode::Normal;
    }
    footer(ui, &theme, |ui| {
        ui.label(muted(&theme, tl("Esc to cancel")));
        buttons(ui, |ui| {
            if button(ui, &theme, Some(Icon::Plus), tl("New project…"), true).clicked() {
                g.app.input_mode = InputMode::Normal;
                g.wizard = Some(super::setup::ProjectForm::new(
                    &g.ctx,
                    super::setup::Mode::New,
                ));
            }
            if button(ui, &theme, None, tl("Close"), false).clicked() {
                close(g);
            }
        });
    });
}

/// The project form, as a dialog in the shell of the others: a new project,
/// or the tokens of one.
fn wizard(g: &mut GuiApp, ctx: &egui::Context) {
    let theme = g.theme;
    let Some(form) = g.wizard.as_mut() else {
        return;
    };
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(1));
    let mut outcome = form.poll();
    let mut closed = false;
    let width = (ctx.content_rect().width() - 96.0).clamp(480.0, 620.0);
    let response = egui::Modal::new(egui::Id::unique("project_form"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            closed = header(ui, &theme, form.icon(), form.title(), Some(form.subtitle()));
            rule(ui, &theme);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing.item.y;
                // The new project's form is long: it scrolls in a low window.
                let height = (ctx.content_rect().height() - 220.0).clamp(220.0, 640.0);
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .show(ui, |ui| body(ui, |ui| form.body(ui, &theme)));
            });
            // The check's verdict stays in view, whatever the scroll.
            if form.has_verdict() {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 20,
                        right: 20,
                        top: 0,
                        bottom: 14,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        form.verdict(ui, &theme);
                    });
            }
            footer(ui, &theme, |ui| {
                if let super::setup::Outcome::Cancelled = form.footer(ui, &theme, true) {
                    outcome = super::setup::Outcome::Cancelled;
                }
            });
        });
    let tokens = form.is_tokens();
    if (closed || response.should_close()) && !form.is_busy() {
        outcome = super::setup::Outcome::Cancelled;
    }
    match outcome {
        super::setup::Outcome::Open => {}
        super::setup::Outcome::Cancelled => g.wizard = None,
        super::setup::Outcome::Saved(entry) => {
            g.wizard = None;
            if tokens {
                // Asked once: a key left empty opens the project without tickets.
                if let Some(t) = &entry.tracker {
                    g.skip_redmine
                        .insert(t.url.trim_end_matches('/').to_string());
                }
            }
            g.open_project(*entry);
        }
    }
}

// ── Log time ─────────────────────────────────────────────────────────────────

fn log_time(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let activities = g.app.activities.clone();
    let error = g.app.activities_error.clone();
    body(ui, |ui| {
        let form = &mut g.app.log_time_form;
        egui::Grid::new("log_time")
            .num_columns(2)
            .spacing([14.0, 12.0])
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tl("Duration")).font(bevel_theme::fonts::medium(theme.text_size)),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut form.duration_input)
                        .hint_text("1h30, 45m, 1.5"),
                );
                ui.end_row();

                ui.label(
                    RichText::new(tl("Activity")).font(bevel_theme::fonts::medium(theme.text_size)),
                );
                if activities.is_empty() {
                    match &error {
                        Some(e) => ui.label(RichText::new(e).color(theme.tones.red.fg)),
                        None => ui.label(muted(&theme, tl("Loading…"))),
                    };
                } else {
                    let selected = activities
                        .get(form.selected_activity_idx)
                        .map(|a| a.name.as_str())
                        .unwrap_or("—");
                    egui::ComboBox::from_id_salt("activity")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for (i, activity) in activities.iter().enumerate() {
                                ui.selectable_value(
                                    &mut form.selected_activity_idx,
                                    i,
                                    &activity.name,
                                );
                            }
                        });
                }
                ui.end_row();

                ui.label(
                    RichText::new(tl("Comment")).font(bevel_theme::fonts::medium(theme.text_size)),
                );
                ui.add(egui::TextEdit::multiline(&mut form.comment_input).desired_rows(3));
                ui.end_row();
            });
        if let Some(error) = &form.error {
            ui.add_space(6.0);
            ui.label(RichText::new(error).color(theme.tones.red.fg));
        }
    });
    let submitting = g.app.log_time_form.submitting;
    footer(ui, &theme, |ui| {
        ui.label(muted(&theme, tl("Esc to cancel")));
        buttons(ui, |ui| {
            let label = tl(if submitting { "Sending…" } else { "Log time" });
            if ui
                .add_enabled_ui(!submitting, |ui| primary_button(ui, &theme, label))
                .inner
                .clicked()
            {
                g.pending.push(Action::SubmitLogTime);
            }
            if button(ui, &theme, None, tl("Cancel"), false).clicked() {
                close(g);
            }
        });
    });
}

// ── Help ─────────────────────────────────────────────────────────────────────

/// The window's shortcuts: the TUI's, minus those for moving the focus.
const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Merge requests",
        &[
            ("↑ / ↓  or  k / j", "Select the previous / next MR"),
            (
                "Enter  or  double click",
                "Open the MR's page (Esc: back to the list)",
            ),
            ("O", "Open the selected MR in the browser"),
            ("Space", "Flag / unflag the selected MR"),
            ("Y", "Copy the git clone command"),
            ("E", "Copy the visible MRs as a Markdown checklist"),
            (
                "Ctrl+Shift+C",
                "Copy the MRs shown in the export's last format",
            ),
            ("Del", "Stop tracking the selected MR"),
            ("R", "Refresh now"),
            (
                "Ctrl+click  or  Shift+click",
                "Pick several MRs, for the bulk actions",
            ),
        ],
    ),
    (
        "Views",
        &[
            ("/  or  I", "Search (Enter on a number tracks the MR)"),
            ("F", "Show / hide the views panel"),
            ("M", "Hide / show merged MRs"),
            ("S / Shift+S", "Next sort column / reverse the order"),
            ("C", "Columns"),
            ("P", "Inspector: details / pipelines"),
            ("T", "Inspector: linked ticket"),
            ("[ / ]", "Previous / next section of a long page"),
            ("Home", "Back to the top of a long page"),
        ],
    ),
    (
        "Projects",
        &[
            ("W", "Open or add a project"),
            ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab"),
            ("Ctrl+1 … 9", "Go to that tab (9: the last)"),
            ("Ctrl+W  or  middle click", "Close the tab"),
        ],
    ),
    (
        "Elsewhere",
        &[
            ("Ctrl+K", "Command palette: pages, views, MRs and commands"),
            ("L", "Log time on the linked ticket"),
            ("G", "Statistics (Tab: next tab, W: time window)"),
            (",", "Settings"),
            ("?", "This list"),
            (
                "Esc",
                "Close a dialog / leave the statistics / unpick the MRs",
            ),
        ],
    ),
];

/// Width of the keys' column, the same in every section.
const KEYS_WIDTH: f32 = 190.0;

/// The keys of a shortcut as keycaps: "↑ / ↓  or  k / j" gives four caps, the
/// "/" and "or" between them as plain text.
fn keys(ui: &mut Ui, theme: &Theme, keys: &str) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (i, token) in keys.split_whitespace().enumerate() {
        // Between keys, "/" and "or" are words; first, "/" is the search key.
        if i > 0 && (token == "/" || token == "or") {
            ui.label(muted(theme, tl(token)));
            continue;
        }
        let galley = ui.painter().layout_no_wrap(
            token.to_string(),
            egui::FontId::monospace(theme.text_size - 1.5),
            theme.palette.text,
        );
        let size = Vec2::new((galley.size().x + 12.0).max(22.0), 22.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let radius = CornerRadius::same(theme.radii.small);
        ui.painter().rect_filled(rect, radius, theme.palette.panel);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme.palette.border),
            egui::StrokeKind::Inside,
        );
        // The cap's lower edge, a little thicker.
        ui.painter().hline(
            rect.x_range().shrink(2.0),
            rect.bottom() - 1.0,
            Stroke::new(1.5, theme.palette.border),
        );
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            theme.palette.text,
        );
    }
}

/// The shortcuts, by section, each a card of rows: the keys, what they do.
/// Two columns once the dialog is wide enough.
fn help(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let height = (ui.ctx().content_rect().height() - 240.0).clamp(280.0, 640.0);
    egui::ScrollArea::vertical()
        .max_height(height)
        .show(ui, |ui| {
            body(ui, |ui| {
                let width = ui.available_width();
                if width < 740.0 {
                    for (i, (section, entries)) in SHORTCUTS.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(16.0);
                        }
                        shortcut_section(ui, &theme, section, entries);
                    }
                    return;
                }
                // Merge requests and projects on the left, the rest on the right:
                // the two columns about as long.
                let gap = 20.0;
                let column = (width - gap) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for half in [[0, 2], [1, 3]] {
                        ui.allocate_ui_with_layout(
                            Vec2::new(column, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(column);
                                for (j, i) in half.into_iter().enumerate() {
                                    if let Some((section, entries)) = SHORTCUTS.get(i) {
                                        if j > 0 {
                                            ui.add_space(16.0);
                                        }
                                        shortcut_section(ui, &theme, section, entries);
                                    }
                                }
                            },
                        );
                    }
                });
            });
        });
    footer(ui, &theme, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        keys(ui, &theme, "?");
        ui.label(muted(&theme, tl("This list")));
        buttons(ui, |ui| {
            if primary_button(ui, &theme, tl("Close")).clicked() {
                close(g);
            }
        });
    });
}

fn shortcut_section(ui: &mut Ui, theme: &Theme, section: &str, entries: &[(&str, &str)]) {
    caption(ui, theme, tl(section));
    ui.add_space(2.0);
    card(ui, theme, |ui| {
        for (i, (key, what)) in entries.iter().enumerate() {
            if i > 0 {
                rule(ui, theme);
            }
            egui::Frame::new()
                .inner_margin(Margin::symmetric(12, 7))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.allocate_ui_with_layout(
                            Vec2::new(KEYS_WIDTH, 24.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_width(KEYS_WIDTH);
                                keys(ui, theme, key);
                            },
                        );
                        let width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            Vec2::new(width, 24.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.set_width(width);
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new(tl(what)).color(theme.palette.text_secondary),
                                );
                            },
                        );
                    });
                });
        }
    });
}
