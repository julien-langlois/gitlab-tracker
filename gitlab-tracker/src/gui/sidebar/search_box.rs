//! The text filter: its field, its steps and its suggestions (labels among them).

use super::*;

/// A filter that takes a value: its caption, a field (Enter applies it, ✕ clears
/// it) and the values met on the tracked MRs as suggestions: ↑ and ↓ highlight
/// one, Enter or a click applies it.
pub(super) fn text_filter(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef) {
    if def.id == "absent_from_branch" && !g.app.branches.is_empty() {
        branch_pills(g, ui, index, def);
        return;
    }
    if def.id == "merged_between" {
        merged_between(g, ui, index, def);
        return;
    }
    let theme = g.theme;
    let current = filter_on(g, index).map(|f| f.query.clone());
    let active = current.is_some();
    ui.add_space(4.0);
    ui.label(muted(&theme, clean_label(def)));

    let id = ui.make_persistent_id(("text_filter", def.id));
    let mut draft: String = ui
        .data_mut(|d| d.get_temp(id))
        .unwrap_or_else(|| current.unwrap_or_default());
    let mut submit = None;
    let mut clear = false;

    // The arrows go to the suggestions, taken before the field would move its
    // text cursor with them; the field's id is the one of the last frame.
    let field_key = id.with("field");
    let highlight_key = id.with("highlight");
    let focused = ui
        .data(|d| d.get_temp::<egui::Id>(field_key))
        .is_some_and(|field| ui.memory(|m| m.has_focus(field)));
    let mut highlight: Option<usize> = ui.data(|d| d.get_temp(highlight_key)).flatten();
    let before = if focused {
        suggestions(g, def.id, &draft)
    } else {
        Vec::new()
    };
    if before.is_empty() {
        highlight = None;
    } else {
        let (down, up) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            )
        });
        highlight = step(highlight, before.len(), down, up);
    }
    let clear_room = if active { 26.0 } else { 0.0 };
    let response = field(
        ui,
        &theme,
        &mut draft,
        ("text_filter_edit", def.id),
        FieldLook {
            hint: &hint(def.id, &g.app.branches),
            icon: None,
            width: ui.available_width() - clear_room,
            active,
        },
    );
    ui.data_mut(|d| d.insert_temp(field_key, response.id));
    if response.changed() {
        highlight = None;
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let picked = highlight
            .and_then(|i| before.get(i))
            .map(|(name, ..)| name.clone());
        submit = Some(picked.unwrap_or_else(|| draft.trim().to_string()));
        highlight = None;
    }
    if active {
        let clear_rect = egui::Rect::from_min_size(
            egui::pos2(response.rect.right() + 4.0, response.rect.top()),
            Vec2::new(22.0, response.rect.height()),
        );
        let style = theme.icon_button();
        let button = bevel_widgets::buttons::IconButton::new(
            tl("Clear the filter"),
            Vec2::splat(22.0),
            &style,
        );
        let clicked = ui
            .put(clear_rect, |ui: &mut Ui| {
                button.show(ui, |ui, rect, tint| {
                    super::super::icons::paint(
                        ui.painter(),
                        rect.shrink(5.0),
                        super::super::icons::Icon::Close,
                        tint,
                    )
                })
            })
            .tip(tl("Clear the filter"))
            .clicked();
        clear |= clicked;
    }

    // Suggestions while the field has the keyboard, and while the pointer is on
    // them (clicking one takes the focus away from the field).
    let list_id = id.with("list_hovered");
    let over_list = ui.data(|d| d.get_temp::<bool>(list_id)).unwrap_or(false);
    let open = response.has_focus() || over_list;
    let suggestions = if open {
        suggestions(g, def.id, &draft)
    } else {
        Vec::new()
    };
    let highlight = highlight.filter(|i| *i < suggestions.len());
    ui.data_mut(|d| d.insert_temp(highlight_key, highlight));
    let mut hovered = false;
    if !suggestions.is_empty() {
        let (picked, list_hovered) = suggestion_list(ui, &theme, &suggestions, highlight);
        hovered = list_hovered;
        if picked.is_some() {
            submit = picked;
        }
    }
    ui.data_mut(|d| d.insert_temp(list_id, hovered && submit.is_none()));

    if clear {
        draft.clear();
        g.pending
            .push(Action::SetFilters(with_filter(g, index, None, false, &[])));
    } else if let Some(query) = submit.clone().filter(|q| !q.is_empty()) {
        draft = query.clone();
        g.pending.push(Action::SetFilters(with_filter(
            g,
            index,
            Some(query),
            false,
            &[],
        )));
    }
    if !active && !response.has_focus() && submit.is_none() && draft.is_empty() {
        ui.data_mut(|d| d.remove::<String>(id));
    } else {
        ui.data_mut(|d| d.insert_temp(id, draft));
    }
}

/// The highlighted suggestion after ↓ (`down`) or ↑ (`up`) among `len`, wrapping
/// around; the first press highlights the first (or last) one, as in the TUI.
pub(super) fn step(highlight: Option<usize>, len: usize, down: bool, up: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match (highlight, down, up) {
        (None, true, _) => Some(0),
        (None, _, true) => Some(len - 1),
        (Some(i), true, _) => Some((i + 1) % len),
        (Some(i), _, true) => Some((i + len - 1) % len),
        (current, _, _) => current.filter(|i| *i < len),
    }
}

/// The values a filter with a value can take, from the tracked MRs, matching
/// `query`: name, swatch color and how many MRs carry it.
pub(super) fn suggestions(g: &mut GuiApp, id: &str, query: &str) -> Vec<(String, Color32, usize)> {
    let muted = g.theme.palette.text_muted;
    let values: Vec<String> = match id {
        "label" => return label_suggestions(g, query),
        "absent_from_branch" => g.app.branches.clone(),
        // The branches the MRs go to, then the tracked ones.
        "target_branch" => g
            .app
            .mrs
            .iter()
            .map(|mr| mr.target_branch.clone())
            .chain(g.app.branches.iter().cloned())
            .collect(),
        "milestone" => g
            .app
            .mrs
            .iter()
            .filter_map(|mr| mr.milestone.clone())
            .collect(),
        "assignee" => g
            .app
            .mrs
            .iter()
            .filter_map(|mr| mr.assignee.clone())
            .collect(),
        _ => return Vec::new(),
    };
    // Counted as the filter matches: the MRs whose value is this one.
    let count = |value: &str| -> usize {
        g.app
            .mrs
            .iter()
            .filter(|mr| match id {
                "target_branch" => mr.target_branch == value,
                "milestone" => mr.milestone.as_deref() == Some(value),
                "assignee" => mr.assignee.as_deref() == Some(value),
                _ => false,
            })
            .count()
    };
    let query = query.trim().to_lowercase();
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|v| !v.trim().is_empty() && v.to_lowercase().contains(&query))
        .filter(|v| seen.insert(v.to_lowercase()))
        .take(8)
        .map(|v| {
            let n = count(&v);
            (v, muted, n)
        })
        .collect()
}

pub(super) fn hint(id: &str, branches: &[String]) -> String {
    let example = match id {
        "label" => "review::",
        "absent_from_branch" => branches.last().map_or("", String::as_str),
        "target_branch" => branches.first().map_or("", String::as_str),
        "milestone" => "v2.4",
        _ => "",
    };
    if example.is_empty() {
        tl("Type, then Enter").into()
    } else {
        tf("e.g. {example}", &[("example", example)])
    }
}

/// The labels matching `query`, with their swatch color and how many tracked MRs
/// carry them.
pub(super) fn label_suggestions(g: &mut GuiApp, query: &str) -> Vec<(String, Color32, usize)> {
    let theme = g.theme;
    // `label_suggestions` reads the filter picker's state, as in the TUI.
    if let Some(position) = g
        .app
        .visible_filter_defs()
        .iter()
        .position(|(_, d)| d.id == "label")
    {
        g.app.filter_picker.cursor = position;
    }
    g.app.filter_picker.input = query.to_string();
    g.app
        .label_suggestions()
        .into_iter()
        .take(8)
        .map(|label| {
            let key = label.to_lowercase();
            let count = g
                .app
                .mrs
                .iter()
                .filter(|mr| {
                    mr.labels.iter().any(|l| {
                        let l = l.to_lowercase();
                        if key.ends_with("::") {
                            l.starts_with(&key)
                        } else {
                            l == key
                        }
                    })
                })
                .count();
            let color = label_tone(&g.app, &label, &theme).fg;
            (label, color, count)
        })
        .collect()
}

/// A raised list under a field, `highlight`ed from the keyboard; returns the
/// entry clicked.
pub(super) fn suggestion_list(
    ui: &mut Ui,
    theme: &bevel_theme::Theme,
    entries: &[(String, Color32, usize)],
    highlight: Option<usize>,
) -> (Option<String>, bool) {
    let mut picked = None;
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .shadow(egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(46),
        })
        .inner_margin(Margin::same(4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, (name, color, count)) in entries.iter().enumerate() {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                if response.hovered() || highlight == Some(i) {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(4), theme.palette.selection);
                }
                let swatch = egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 12.0, rect.center().y),
                    Vec2::splat(8.0),
                );
                ui.painter()
                    .rect_filled(swatch, CornerRadius::same(2), *color);
                ui.painter().text(
                    egui::pos2(rect.left() + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::monospace(theme.text_size - 1.5),
                    theme.palette.text,
                );
                if *count > 0 {
                    ui.painter().text(
                        egui::pos2(rect.right() - 8.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        count.to_string(),
                        egui::FontId::proportional(theme.text_size - 2.0),
                        theme.palette.text_muted,
                    );
                }
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    picked = Some(name.clone());
                }
            }
        });
    (picked, frame.response.contains_pointer())
}
