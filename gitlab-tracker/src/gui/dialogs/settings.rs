//! The settings dialog: a tab per section, the search across them, each setting's control.

use super::*;

/// The settings, a tab per section on the left: the window's own first, then
/// the project's. A search goes through every section at once.
pub(super) fn settings(g: &mut GuiApp, ui: &mut Ui) {
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
pub(super) fn settings_nav(g: &mut GuiApp, ui: &mut Ui, sections: &[Section], query: &str) {
    let theme = g.theme;
    let active = !g.settings_search.is_empty();
    let search = super::super::widgets::field(
        ui,
        &theme,
        &mut g.settings_search,
        "settings_search",
        super::super::widgets::FieldLook {
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
pub(super) fn settings_page(g: &mut GuiApp, ui: &mut Ui, sections: &[Section]) {
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
pub(super) fn settings_found(g: &mut GuiApp, ui: &mut Ui, sections: &[Section], query: &str) {
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
pub(super) fn setting_rows(g: &mut GuiApp, ui: &mut Ui, rows: &[Row]) {
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
pub(super) fn setting(g: &mut GuiApp, ui: &mut Ui, row: Row) {
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
pub(super) fn control_width(g: &GuiApp, row: Row) -> f32 {
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

pub(super) fn control(g: &mut GuiApp, ui: &mut Ui, row: Row) {
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
                    if let Some(i) = super::super::board::segmented(
                        ui,
                        &theme,
                        "settings_theme",
                        &labels,
                        selected,
                    ) {
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
pub(super) fn language_control(ui: &mut Ui, language: &mut Option<String>) {
    let system = tf(
        "System ({language})",
        &[("language", super::super::i18n::system_language_name())],
    );
    let languages = super::super::i18n::languages();
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
pub(super) fn unit(id: &str) -> Option<&'static str> {
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
