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
    if g.wizard.is_some() {
        wizard(g, ctx);
        return;
    }
    let (title, width): (&str, f32) = match g.app.input_mode {
        InputMode::Settings => (tl("Settings"), 520.0),
        InputMode::ColumnPicker => (tl("Columns"), 320.0),
        InputMode::ProjectPicker => (tl("Projects"), 480.0),
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
            let mut edit = egui::TextEdit::singleline(text).desired_width(CONTROL_WIDTH);
            if item.def.id == "core.name" {
                edit = edit
                    .char_limit(crate::settings_core::MAX_PROJECT_NAME_CHARS)
                    .hint_text(tl("Project id"));
            }
            ui.add(edit);
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

/// The projects of `projects.toml`: a click shows its tab, opening it when
/// needed. "New project…" opens the project form.
fn projects(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let mut picked = None;
    ui.label(muted(
        &theme,
        tl("Each project opens in its own tab, and keeps refreshing while another one is shown."),
    ));
    ui.add_space(10.0);
    egui::ScrollArea::vertical()
        .max_height(360.0)
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
                            theme.palette.content
                        } else {
                            theme.palette.raised
                        })
                        .stroke(Stroke::new(1.0, theme.palette.border))
                        .corner_radius(CornerRadius::same(theme.radii.medium))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(semibold(name, theme.text_size));
                                    ui.label(muted(
                                        &theme,
                                        format!("{} · {}", project.gitlab_url, project.project_id),
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
                                        super::widgets::badge(ui, &theme, state, theme.tones.green);
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
                    ui.add_space(6.0);
                }
            });
        });
    if g.app.project_picker.is_empty() {
        ui.label(muted(&theme, tl("No project in projects.toml.")));
    }
    if let Some(entry) = picked.and_then(|i| g.app.project_picker.get(i).cloned()) {
        g.pending.push(Action::SwitchProject(Box::new(entry)));
        g.app.input_mode = InputMode::Normal;
    }
    buttons(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if button(
            ui,
            &theme,
            Some(super::icons::Icon::Plus),
            tl("New project…"),
            true,
        )
        .clicked()
        {
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
}

/// The project form, as a dialog: a new project, or the tokens of one.
fn wizard(g: &mut GuiApp, ctx: &egui::Context) {
    let theme = g.theme;
    let Some(form) = g.wizard.as_mut() else {
        return;
    };
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(20));
    let mut outcome = super::setup::Outcome::Open;
    let response = egui::Modal::new(egui::Id::unique("project_form"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(
                RichText::new(form.title())
                    .strong()
                    .size(theme.text_size + 3.0),
            );
            ui.add_space(12.0);
            // The new project's form is long: it scrolls in a low window. The
            // tokens' fits as it is.
            if form.is_tokens() {
                outcome = form.show(ui, &theme);
            } else {
                let height = (ctx.content_rect().height() - 160.0).clamp(240.0, 640.0);
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .min_scrolled_height(height)
                    .show(ui, |ui| outcome = form.show(ui, &theme));
            }
        });
    let tokens = form.is_tokens();
    if response.should_close() && !form.is_busy() {
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
const KEYS_WIDTH: f32 = 200.0;

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

fn help(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    egui::ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.7)
        .show(ui, |ui| {
            for (section, entries) in SHORTCUTS {
                section_title(ui, &theme, tl(section));
                ui.add_space(4.0);
                for (key, what) in *entries {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(KEYS_WIDTH, 24.0),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_width(KEYS_WIDTH);
                                keys(ui, &theme, key);
                            },
                        );
                        ui.label(tl(what));
                    });
                }
                ui.add_space(10.0);
            }
        });
    buttons(ui, |ui| {
        if primary_button(ui, &theme, tl("Close")).clicked() {
            close(g);
        }
    });
}
