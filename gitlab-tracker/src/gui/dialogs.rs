//! Dialogs, shown for the app's popup modes like the TUI's popups: settings,
//! columns, project picker, log time and keyboard shortcuts.

use bevel_theme::Theme;
use egui::{Align, CornerRadius, Layout, Margin, RichText, Stroke, Ui, Vec2};
use gitlab_tracker_core::{ProjectSettingKind, ProjectSettingValue};

use super::widgets::{button, muted, primary_button, rule, section_title, semibold, switch};
use super::GuiApp;
use crate::actions::Action;
use crate::app::{InputMode, LogTimeForm};
use crate::i18n::{tf, tl};
use crate::settings::SettingsEditorItem;

pub fn show(g: &mut GuiApp, ctx: &egui::Context) {
    let (title, width): (&str, f32) = match g.app.input_mode {
        InputMode::Settings => (tl("Settings"), 520.0),
        InputMode::ColumnPicker => (tl("Columns"), 320.0),
        InputMode::ProjectPicker => (tl("Switch project"), 460.0),
        InputMode::LogTime => (tl("Log time"), 420.0),
        InputMode::Help => (tl("Keyboard shortcuts"), 520.0),
        _ => return,
    };
    let theme = g.theme;
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(egui::CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(20));
    let response = egui::Modal::new(egui::Id::unique("dialog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new(title).strong().size(theme.text_size + 3.0));
            ui.add_space(12.0);
            match g.app.input_mode {
                InputMode::Settings => settings(g, ui),
                InputMode::ColumnPicker => columns(g, ui),
                InputMode::ProjectPicker => projects(g, ui),
                InputMode::LogTime => log_time(g, ui),
                InputMode::Help => help(g, ui),
                _ => {}
            }
        });
    // Escape or a click outside: cancel, as Esc does in the TUI.
    if response.should_close() {
        close(g);
    }
}

/// Closes the dialog without applying it (the columns are saved as they change).
fn close(g: &mut GuiApp) {
    match g.app.input_mode {
        InputMode::Settings => {
            g.app.settings_editor.cancel();
            g.language_draft = g.language.clone();
        }
        InputMode::ColumnPicker => save_columns(g),
        InputMode::LogTime => g.app.log_time_form = LogTimeForm::default(),
        _ => {}
    }
    g.app.input_mode = InputMode::Normal;
}

fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(16.0);
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// Settings the GUI leaves out: the cockpit has its own page in the icon rail,
/// where the TUI shows it as a pane. Their saved values stay as they are.
const GUI_HIDDEN_SETTINGS: &[&str] = &["core.show_cockpit"];

/// The settings by section, each section a card of rows: name and help on the
/// left, the control on the right. The window's own settings come first, then
/// the project's.
fn settings(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    ui.label(muted(
        &theme,
        tf(
            "{project} · saved to projects.toml",
            &[("project", &super::shell::project_name(g))],
        ),
    ));
    ui.add_space(14.0);
    egui::ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.6)
        .show(ui, |ui| {
            settings_section(ui, &theme, tl("Interface"), |ui| {
                setting_row(
                    ui,
                    &theme,
                    tl("Language"),
                    tl("Language of this window. The terminal UI stays in English."),
                    |ui| language_control(ui, &mut g.language_draft),
                );
            });
            ui.add_space(16.0);
            let items: Vec<_> = g
                .app
                .settings_editor
                .items
                .iter_mut()
                .filter(|item| !GUI_HIDDEN_SETTINGS.contains(&item.def.id))
                .collect();
            let mut sections: Vec<(&str, Vec<_>)> = Vec::new();
            for item in items {
                match sections.iter_mut().find(|(s, _)| *s == item.def.section) {
                    Some((_, group)) => group.push(item),
                    None => sections.push((item.def.section, vec![item])),
                }
            }
            for (i, (section, group)) in sections.into_iter().enumerate() {
                if i > 0 {
                    ui.add_space(16.0);
                }
                let title = if section == "Core" {
                    "General"
                } else {
                    section
                };
                settings_section(ui, &theme, tl(title), |ui| {
                    let last = group.len() - 1;
                    for (j, item) in group.into_iter().enumerate() {
                        project_setting_row(ui, &theme, item);
                        if j < last {
                            rule(ui, &theme);
                        }
                    }
                });
            }
        });
    ui.add_space(16.0);
    rule(ui, &theme);
    ui.add_space(14.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if button(ui, &theme, None, tl("Save"), true).clicked() {
            g.pending.push(Action::SaveSettings);
            g.save_language();
            g.app.input_mode = InputMode::Normal;
        }
        if button(ui, &theme, None, tl("Cancel"), false).clicked() {
            close(g);
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(muted(&theme, tl("Esc to cancel")).size(theme.text_size - 1.5));
        });
    });
}

/// A section of the settings: its title, then a card of rows.
fn settings_section(ui: &mut Ui, theme: &Theme, title: &str, rows: impl FnOnce(&mut Ui)) {
    ui.label(semibold(title.to_uppercase(), theme.text_size - 2.5).color(theme.palette.text_muted));
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            rows(ui);
        });
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

/// Width kept on the right of a setting row for its control.
const CONTROL_WIDTH: f32 = 130.0;

/// One project setting, with its name and help in the window's language.
fn project_setting_row(ui: &mut Ui, theme: &Theme, item: &mut SettingsEditorItem) {
    let (label, help) = (tl(item.def.label), tl(item.def.help));
    setting_row(ui, theme, label, help, |ui| {
        setting_control(ui, theme, item)
    });
}

/// One setting: its name and help, then its control centered on the row.
fn setting_row(ui: &mut Ui, theme: &Theme, label: &str, help: &str, control: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                let text_width = ui.available_width() - CONTROL_WIDTH - 16.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(text_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(text_width);
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(
                            RichText::new(label).font(bevel_theme::fonts::medium(theme.text_size)),
                        );
                        ui.label(muted(theme, help).size(theme.text_size - 1.5));
                    },
                );
                // Last, once the text set the row's height: centered on it.
                let height = ui.min_rect().height();
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), height),
                    Layout::right_to_left(Align::Center),
                    control,
                );
            });
        });
}

fn setting_control(ui: &mut Ui, theme: &Theme, item: &mut SettingsEditorItem) {
    match (&item.def.kind, &mut item.value) {
        (ProjectSettingKind::Bool, ProjectSettingValue::Bool(on)) => {
            if switch(ui, theme, *on, "").clicked() {
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
            ui.add_sized([96.0, theme.spacing.control_height], value);
        }
        (ProjectSettingKind::Text, ProjectSettingValue::Text(text)) => {
            ui.add(egui::TextEdit::singleline(text).desired_width(CONTROL_WIDTH));
        }
        _ => {
            ui.label(muted(theme, "—"));
        }
    }
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

fn columns(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    ui.label(muted(
        &theme,
        tl("Shared with the terminal UI. Columns that do not fit the window are hidden until it is wider."),
    ));
    ui.add_space(8.0);
    for def in g.app.visible_column_defs() {
        let mut visible = g.app.config.visible_columns.is_visible(def.id);
        if ui.checkbox(&mut visible, tl(def.label)).changed() {
            g.app.config.visible_columns.toggle(def.id);
            // From now on the table follows this choice (see `table::wanted_columns`).
            g.app.project_settings.visible_columns = Some(g.app.config.visible_columns.clone());
        }
    }
    buttons(ui, |ui| {
        if primary_button(ui, &theme, tl("Done")).clicked() {
            close(g);
        }
    });
}

fn save_columns(g: &mut GuiApp) {
    if g.app.project_settings.visible_columns.is_some() {
        g.pending.push(Action::SaveVisibleColumns);
    }
}

// ── Project picker ───────────────────────────────────────────────────────────

fn projects(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let mut picked = None;
    ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
        for (i, project) in g.app.project_picker.iter().enumerate() {
            let current =
                crate::storage::is_same_project(project, &g.app.base_url, &g.app.project_id);
            let name = project.name.as_deref().unwrap_or(&project.project_id);
            let text = RichText::new(format!("{name}   ·   {}", project.gitlab_url));
            let response = ui.selectable_label(current, text);
            if response.clicked() && !current {
                picked = Some(i);
            }
        }
    });
    if g.app.project_picker.is_empty() {
        ui.label(muted(&theme, tl("No project in projects.toml.")));
    }
    if let Some(entry) = picked.and_then(|i| g.app.project_picker.get(i).cloned()) {
        g.pending.push(Action::SwitchProject(Box::new(entry)));
        g.app.input_mode = InputMode::Normal;
    }
    buttons(ui, |ui| {
        if ui.button(tl("Cancel")).clicked() {
            close(g);
        }
    });
}

// ── Log time ─────────────────────────────────────────────────────────────────

fn log_time(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let ticket = g
        .app
        .selected_mr()
        .and_then(|mr| mr.linked_ticket.as_ref())
        .map(|t| format!("#{} — {}", t.id, t.subject));
    if let Some(ticket) = ticket {
        ui.label(muted(&theme, ticket));
        ui.add_space(8.0);
    }
    let activities = g.app.activities.clone();
    let error = g.app.activities_error.clone();
    let form = &mut g.app.log_time_form;
    egui::Grid::new("log_time")
        .num_columns(2)
        .spacing([12.0, 10.0])
        .show(ui, |ui| {
            ui.label(tl("Duration"));
            ui.add(
                egui::TextEdit::singleline(&mut form.duration_input).hint_text("1h30, 45m, 1.5"),
            );
            ui.end_row();

            ui.label(tl("Activity"));
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
                            ui.selectable_value(&mut form.selected_activity_idx, i, &activity.name);
                        }
                    });
            }
            ui.end_row();

            ui.label(tl("Comment"));
            ui.add(egui::TextEdit::multiline(&mut form.comment_input).desired_rows(3));
            ui.end_row();
        });
    if let Some(error) = &form.error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).color(theme.tones.red.fg));
    }
    let submitting = form.submitting;
    buttons(ui, |ui| {
        let label = tl(if submitting { "Sending…" } else { "Log time" });
        if ui
            .add_enabled_ui(!submitting, |ui| primary_button(ui, &theme, label))
            .inner
            .clicked()
        {
            g.pending.push(Action::SubmitLogTime);
        }
        if ui.button(tl("Cancel")).clicked() {
            close(g);
        }
    });
}

// ── Help ─────────────────────────────────────────────────────────────────────

/// The window's shortcuts: the TUI's, minus those for moving the focus.
const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Merge requests",
        &[
            ("↑ / ↓  or  k / j", "Select the previous / next MR"),
            ("O", "Open the selected MR in the browser"),
            ("Space", "Flag / unflag the selected MR"),
            ("Y", "Copy the git clone command"),
            ("E", "Copy the visible MRs as a Markdown checklist"),
            ("Del", "Stop tracking the selected MR"),
            ("R", "Refresh now"),
        ],
    ),
    (
        "Views",
        &[
            ("/  or  I", "Search (Enter on a number tracks the MR)"),
            ("F", "Show the views panel"),
            ("M", "Hide / show merged MRs"),
            ("S / Shift+S", "Next sort column / reverse the order"),
            ("C", "Columns"),
            ("P", "Inspector: details / pipelines"),
            ("T", "Inspector: linked ticket"),
        ],
    ),
    (
        "Elsewhere",
        &[
            ("L", "Log time on the linked ticket"),
            ("G", "Statistics (Tab: next tab, W: time window)"),
            ("W", "Switch project"),
            (",", "Settings"),
            ("?", "This list"),
            ("Esc", "Close a dialog / leave the statistics"),
        ],
    ),
];

fn help(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    for (section, entries) in SHORTCUTS {
        section_title(ui, &theme, tl(section));
        egui::Grid::new(section)
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                for (key, what) in *entries {
                    ui.label(RichText::new(*key).monospace().color(theme.palette.accent));
                    ui.label(tl(what));
                    ui.end_row();
                }
            });
        ui.add_space(10.0);
    }
    buttons(ui, |ui| {
        if primary_button(ui, &theme, tl("Close")).clicked() {
            close(g);
        }
    });
}
