//! The views panel: the project, the views with their counts, the filters that
//! take a value (label with suggestions, review effort, absent from branch…), the
//! tracked branches and the "hide merged" switch.

use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use gitlab_tracker_core::FilterDef;

use super::icons::Icon;
use super::widgets::{
    avatar, button, dot, dot_label, field, icon_button, label_tone, mono, muted, pill_toggle, rule,
    section_title, semibold, split_person, switch, FieldLook,
};
use super::{tones, GuiApp};
use crate::actions::Action;
use crate::branch_flow::{BranchFlow, BranchRole, BranchSetting, FlowBranch};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, Effort, TrackedMr};
use crate::utils::format_relative_date;
use bevel_theme::{fonts, Chip};

/// Views listed first, with their glyph; the other one-click filters are under
/// "More views".
const MAIN_VIEWS: &[(&str, &str)] = &[
    ("all", "≡"),
    ("flagged", "★"),
    ("reviewer_me", "◉"),
    ("assigned_to_me", "@"),
    ("has_notes", "✎"),
    ("ci_failing", "✕"),
];

/// Filters with a value shown directly; the others are under "More filters".
const MAIN_FILTERS: &[&str] = &["label", "absent_from_branch"];

/// The effort filters, shown as pills rather than views.
const EFFORT_FILTERS: [(&str, Effort); 3] = [
    ("effort_easy", Effort::Easy),
    ("effort_medium", Effort::Medium),
    ("effort_complex", Effort::Complex),
];

/// A filter and its index in the full list.
type Entry = (usize, &'static FilterDef);

/// Where each of the visible filters goes in the panel. Every filter lands in
/// exactly one group, so one a plugin registers shows up without changes here.
#[derive(Default)]
struct Groups {
    /// [`MAIN_VIEWS`], in that order, with their glyph.
    views: Vec<(Entry, &'static str)>,
    /// The other one-click filters, under "More views".
    more_views: Vec<Entry>,
    /// The [`EFFORT_FILTERS`], as pills.
    effort: Vec<(usize, Effort)>,
    /// [`MAIN_FILTERS`]: a field each.
    filters: Vec<Entry>,
    /// The other filters with a value, under "More filters".
    more_filters: Vec<Entry>,
}

impl Groups {
    fn of(visible: &[Entry]) -> Self {
        let mut groups = Groups::default();
        for &(id, glyph) in MAIN_VIEWS {
            if let Some(&entry) = visible
                .iter()
                .find(|(_, d)| d.id == id && !d.needs_text_input)
            {
                groups.views.push((entry, glyph));
            }
        }
        for &(id, effort) in &EFFORT_FILTERS {
            if let Some(&(index, _)) = visible.iter().find(|(_, d)| d.id == id) {
                groups.effort.push((index, effort));
            }
        }
        for &entry in visible {
            let (index, def) = entry;
            let placed = groups.views.iter().any(|((i, _), _)| *i == index)
                || groups.effort.iter().any(|(i, _)| *i == index);
            if placed {
                continue;
            }
            match (def.needs_text_input, MAIN_FILTERS.contains(&def.id)) {
                (false, _) => groups.more_views.push(entry),
                (true, true) => groups.filters.push(entry),
                (true, false) => groups.more_filters.push(entry),
            }
        }
        groups
    }
}

pub fn panel(g: &mut GuiApp, ui: &mut Ui, width: f32) {
    let theme = g.theme;
    egui::Panel::left("views")
        .exact_size(width)
        .resizable(false)
        .frame(theme.panel_frame())
        .show(ui, |ui| {
            let edge = ui.max_rect();
            ui.painter().vline(
                edge.right() - 0.5,
                edge.y_range(),
                Stroke::new(1.0, theme.palette.border),
            );
            // Content wider than the panel would push its neighbours: wrap instead.
            ui.set_max_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            if g.app.tracker.is_some() {
                egui::Panel::bottom("views_footer")
                    .frame(egui::Frame::new().inner_margin(Margin::symmetric(14, 10)))
                    .show(ui, |ui| footer(g, ui));
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| content(g, ui));
        });
}

fn content(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(
                egui::Label::new(semibold(
                    super::shell::project_name(g),
                    theme.text_size + 1.0,
                ))
                .truncate(),
            );
            let host = g
                .app
                .base_url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            ui.label(
                mono(
                    format!("{host} · #{}", g.app.project_id),
                    theme.text_size - 1.5,
                )
                .color(theme.palette.text_muted),
            );
        });
    rule(ui, &theme);

    let visible = g.app.visible_filter_defs();
    let groups = Groups::of(&visible);
    egui::Frame::new()
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 12,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 6,
                    right: 6,
                    top: 0,
                    bottom: 6,
                })
                .show(ui, |ui| section_title(ui, &theme, tl("Views")));
            for &((index, def), glyph) in &groups.views {
                view_item(g, ui, index, def, glyph);
            }
            let more = &groups.more_views;
            if !more.is_empty() {
                ui.add_space(4.0);
                let active_inside = more.iter().any(|(i, _)| *i == g.app.active_filter.index);
                egui::CollapsingHeader::new(muted(&theme, tl("More views")))
                    .id_salt("more_views")
                    .default_open(active_inside)
                    .show_unindented(ui, |ui| {
                        for &(index, def) in more {
                            view_item(g, ui, index, def, "•");
                        }
                    });
            }
        });
    rule(ui, &theme);

    egui::Frame::new()
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            section_title(ui, &theme, tl("Filters"));
            ui.add_space(6.0);
            for &(index, def) in &groups.filters {
                text_filter(g, ui, index, def);
                if def.id == "label" {
                    effort_pills(g, ui, &groups.effort);
                }
            }
            if !groups.filters.iter().any(|(_, d)| d.id == "label") {
                effort_pills(g, ui, &groups.effort);
            }
            let more = &groups.more_filters;
            if !more.is_empty() {
                ui.add_space(2.0);
                let active_inside = more.iter().any(|(i, _)| *i == g.app.active_filter.index);
                egui::CollapsingHeader::new(muted(&theme, tl("More filters")))
                    .id_salt("more_filters")
                    .default_open(active_inside)
                    .show_unindented(ui, |ui| {
                        for &(index, def) in more {
                            text_filter(g, ui, index, def);
                        }
                    });
            }
            ui.add_space(4.0);
            if switch(ui, &theme, g.app.config.hide_merged, tl("Hide merged MRs"))
                .on_hover_text(tl("They stay tracked and refreshed (M)"))
                .clicked()
            {
                g.pending.push(Action::ToggleHideMerged);
            }
        });
    rule(ui, &theme);

    egui::Frame::new()
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            branches(g, ui);
        });
}

fn footer(g: &GuiApp, ui: &mut Ui) {
    let theme = &g.theme;
    let name = g.app.tracker.as_ref().map_or("Tracker", |t| t.name());
    dot_label(
        ui,
        theme,
        theme.tones.green.fg,
        tf("{name} connected", &[("name", name)]),
    );
}

/// The name of a filter without the TUI's emoji and "… (type below)" hint, in
/// the window's language.
fn clean_label(def: &FilterDef) -> String {
    let label = def.label.split(" (").next().unwrap_or(def.label);
    let label: String = label.chars().filter(|c| (*c as u32) < 0x2000).collect();
    tl(label.trim().trim_end_matches('…').trim()).to_string()
}

/// One view: glyph, name and how many MRs it shows.
fn view_item(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef, glyph: &str) {
    let theme = g.theme;
    let p = &theme.palette;
    let selected = index == g.app.active_filter.index;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let fill = if selected {
        p.selection
    } else if response.hovered() {
        p.raised
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(theme.radii.medium), fill);
    let y = rect.center().y;
    let glyph_color = if selected { p.accent } else { p.text_muted };
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, y),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(theme.text_size - 1.0),
        glyph_color,
    );
    let count = g.app.filter_count(index).to_string();
    let count_rect = ui.painter().text(
        egui::pos2(rect.right() - 8.0, y),
        egui::Align2::RIGHT_CENTER,
        &count,
        egui::FontId::proportional(theme.text_size - 1.5),
        p.text_muted,
    );
    let label_color = if selected { p.text } else { p.text_secondary };
    let font = if selected {
        bevel_theme::fonts::medium(theme.text_size)
    } else {
        egui::FontId::proportional(theme.text_size)
    };
    let max_width = count_rect.left() - rect.left() - 40.0;
    let galley = ui
        .painter()
        .layout(clean_label(def), font, label_color, max_width.max(20.0));
    let pos = egui::pos2(rect.left() + 32.0, y - galley.size().y / 2.0);
    ui.painter()
        .with_clip_rect(rect.shrink(2.0))
        .galley(pos, galley, label_color);
    super::widgets::focus_ring(ui, &theme, &response, theme.radii.medium);
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
        && !selected
    {
        g.pending.push(Action::SetFilter {
            index,
            query: String::new(),
        });
    }
}

/// "Absent from branch" as a pill per tracked branch: the branches are known,
/// nothing to type. A second click on the active one clears it.
fn branch_pills(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef) {
    let theme = g.theme;
    ui.add_space(4.0);
    ui.label(muted(&theme, clean_label(def)));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        for branch in g.app.branches.clone() {
            let on = g.app.active_filter.index == index && g.app.active_filter.query == branch;
            let response = pill_toggle(ui, &theme, &branch, theme.tones.red, on)
                .on_hover_text(tf("The MRs not on {branch} yet", &[("branch", &branch)]));
            if response.clicked() {
                g.pending.push(Action::SetFilter {
                    index: if on { 0 } else { index },
                    query: if on { String::new() } else { branch },
                });
            }
        }
    });
    ui.add_space(2.0);
}

/// Review effort as three pills; a second click on the active one clears it.
fn effort_pills(g: &mut GuiApp, ui: &mut Ui, pills: &[(usize, Effort)]) {
    let theme = g.theme;
    if pills.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(muted(&theme, tl("Review effort")));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for &(index, effort) in pills {
            let (label, tone) = tones::effort(effort, &theme.tones);
            let on = g.app.active_filter.index == index;
            if pill_toggle(ui, &theme, label, tone, on).clicked() {
                g.pending.push(Action::SetFilter {
                    index: if on { 0 } else { index },
                    query: String::new(),
                });
            }
        }
    });
    ui.add_space(6.0);
}

/// A filter that takes a value: its caption, a field (Enter applies it, ✕ clears
/// it) and the values met on the tracked MRs as suggestions: ↑ and ↓ highlight
/// one, Enter or a click applies it.
fn text_filter(g: &mut GuiApp, ui: &mut Ui, index: usize, def: &FilterDef) {
    if def.id == "absent_from_branch" && !g.app.branches.is_empty() {
        branch_pills(g, ui, index, def);
        return;
    }
    let theme = g.theme;
    let active = g.app.active_filter.index == index;
    ui.add_space(4.0);
    ui.label(muted(&theme, clean_label(def)));

    let id = ui.make_persistent_id(("text_filter", def.id));
    let mut draft: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_else(|| {
        if active {
            g.app.active_filter.query.clone()
        } else {
            String::new()
        }
    });
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
                    super::icons::paint(
                        ui.painter(),
                        rect.shrink(5.0),
                        super::icons::Icon::Close,
                        tint,
                    )
                })
            })
            .on_hover_text(tl("Clear the filter"))
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
        g.pending.push(Action::SetFilter {
            index: 0,
            query: String::new(),
        });
    } else if let Some(query) = submit.clone().filter(|q| !q.is_empty()) {
        draft = query.clone();
        g.pending.push(Action::SetFilter { index, query });
    }
    if !active && !response.has_focus() && submit.is_none() && draft.is_empty() {
        ui.data_mut(|d| d.remove::<String>(id));
    } else {
        ui.data_mut(|d| d.insert_temp(id, draft));
    }
}

/// The highlighted suggestion after ↓ (`down`) or ↑ (`up`) among `len`, wrapping
/// around; the first press highlights the first (or last) one, as in the TUI.
fn step(highlight: Option<usize>, len: usize, down: bool, up: bool) -> Option<usize> {
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
fn suggestions(g: &mut GuiApp, id: &str, query: &str) -> Vec<(String, Color32, usize)> {
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

fn hint(id: &str, branches: &[String]) -> String {
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
fn label_suggestions(g: &mut GuiApp, query: &str) -> Vec<(String, Color32, usize)> {
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
fn suggestion_list(
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

/// The tracked branches (one column each in the table): remove one, or add one.
fn branches(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    section_title(ui, &theme, tl("Tracked branches"));
    ui.add_space(4.0);
    let mut removed = None;
    let mut moved = None;
    let mut flow_change = None;
    let mut open = None;
    let count = g.app.branches.len();
    let flow = g.app.branch_flow();
    let project = project_url(g);
    for (i, branch) in g.app.branches.iter().enumerate() {
        let row = ui.horizontal(|ui| {
            ui.set_height(24.0);
            dot(ui, theme.tones.green.fg, 6.0);
            ui.label(mono(branch, theme.text_size - 1.0));
            if let Some(fb) = flow.get(branch) {
                flow_change = flow_change
                    .take()
                    .or_else(|| role_chip(g, ui, i, fb, &flow, project.as_deref()));
            }
        });
        // Under it, the merges it still has to take in, when there are some.
        open = open
            .take()
            .or_else(|| behind(g, ui, branch, project.as_deref()));
        // The controls show on hover: the order is the table's columns.
        if !ui.rect_contains_pointer(row.response.rect) {
            continue;
        }
        let mut controls = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row.response.rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        let small = |ui: &mut Ui, text: &str, hint: &str| {
            ui.add(egui::Button::new(muted(&theme, text)).frame(false))
                .on_hover_text(hint)
                .clicked()
        };
        if !g.app.read_only && small(&mut controls, "✕", tl("Stop tracking this branch")) {
            removed = Some(branch.clone());
        }
        if i + 1 < count && small(&mut controls, "↓", tl("Move right in the table")) {
            moved = Some((i, i + 1));
        }
        if i > 0 && small(&mut controls, "↑", tl("Move left in the table")) {
            moved = Some((i, i - 1));
        }
    }
    if let Some(branch) = removed {
        g.pending.push(Action::SubmitInput(format!("-{branch}")));
    }
    if let Some((from, to)) = moved {
        g.pending.push(Action::MoveBranch { from, to });
    }
    if let Some((branch, setting)) = flow_change {
        g.pending.push(Action::SetBranchFlow { branch, setting });
    }
    if let Some(id) = open {
        g.open_mr(&id);
    }
    if g.app.read_only {
        return;
    }
    ui.add_space(4.0);
    let id = ui.make_persistent_id("new_branch");
    let mut draft: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
    let response = ui.add(
        egui::TextEdit::singleline(&mut draft)
            .hint_text(RichText::new(tl("Add a branch, then Enter")))
            .margin(Margin::symmetric(8, 5))
            .desired_width(f32::INFINITY),
    );
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let name = draft.trim().trim_start_matches('-').to_string();
        // A number would be read as an MR to track.
        if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
            g.pending.push(Action::SubmitInput(name));
        }
        draft.clear();
    }
    ui.data_mut(|d| d.insert_temp(id, draft));
}

/// What a tracked branch is for, as a chip: a click sets its role and, for a
/// feature branch, the branch it is fed by. A guessed role shows muted.
/// Returns the new setting of the branch when one was picked.
fn role_chip(
    g: &GuiApp,
    ui: &mut Ui,
    index: usize,
    branch: &FlowBranch,
    flow: &BranchFlow,
    project: Option<&str>,
) -> Option<(String, BranchSetting)> {
    let theme = g.theme;
    let t = &theme.tones;
    let tone = match branch.role {
        BranchRole::Feature => t.teal,
        BranchRole::Integration => t.blue,
        BranchRole::Staging => t.amber,
        BranchRole::Production => t.purple,
    };
    let tone = if branch.guessed { t.gray } else { tone };
    let chip =
        ui.add(Chip::new(tl(branch.role.label()), tone).font(fonts::medium(theme.text_size - 3.0)));
    let response = ui
        .interact(
            chip.rect,
            ui.make_persistent_id(("branch_role", index)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let hint = match (&branch.parent, branch.guessed) {
        (Some(parent), true) => tf(
            "Guessed from the name, fed by {parent}: click to set",
            &[("parent", parent)],
        ),
        (Some(parent), false) => tf("Fed by {parent}: click to change", &[("parent", parent)]),
        (None, true) => tl("Guessed from the name: click to set").to_string(),
        (None, false) => tl("Click to change").to_string(),
    };
    let response = response.on_hover_text(hint);
    if g.app.read_only {
        return None;
    }
    let saved = g
        .app
        .project_settings
        .branch_flow
        .as_ref()
        .and_then(|s| s.get(&branch.name))
        .cloned()
        .unwrap_or_default();
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(180.0);
        ui.label(muted(&theme, tl("Role in the flow")));
        for role in BranchRole::ALL {
            let on = !branch.guessed && branch.role == role;
            if ui.selectable_label(on, tl(role.label())).clicked() {
                picked = Some(BranchSetting {
                    role: Some(role),
                    ..saved.clone()
                });
            }
        }
        if ui
            .selectable_label(branch.guessed, tl("Guess from the name"))
            .clicked()
        {
            picked = Some(BranchSetting {
                role: None,
                ..saved.clone()
            });
        }
        if let (Some(parent), Some(project)) = (&branch.parent, project) {
            ui.separator();
            compare_link(ui, project, &branch.name, parent);
        }
        if branch.role != BranchRole::Feature {
            return;
        }
        ui.separator();
        ui.label(muted(&theme, tl("Fed by")));
        for other in flow.branches.iter().filter(|b| b.name != branch.name) {
            let on = branch.parent.as_deref() == Some(other.name.as_str());
            if ui
                .selectable_label(on, mono(&other.name, theme.text_size - 1.0))
                .clicked()
            {
                picked = Some(BranchSetting {
                    parent: Some(other.name.clone()),
                    ..saved.clone()
                });
            }
        }
    });
    picked.map(|setting| (branch.name.clone(), setting))
}

/// "https://host/group/project", from the web URL of a tracked MR.
fn project_url(g: &GuiApp) -> Option<String> {
    g.app.mrs.iter().find_map(|mr| {
        mr.web_url
            .split_once("/-/merge_requests")
            .map(|(project, _)| project.to_string())
    })
}

/// GitLab's comparison of `branch` with `source`: the commits of `source`
/// not in `branch` yet, tracked MRs or not.
fn compare_url(project: &str, branch: &str, source: &str) -> String {
    format!("{project}/-/compare/{branch}...{source}")
}

fn compare_link(ui: &mut Ui, project: &str, branch: &str, source: &str) {
    let text = format!(
        "{} ↗",
        tf("Compare with {source} on GitLab", &[("source", source)])
    );
    if ui
        .button(text)
        .on_hover_text(compare_url(project, branch, source))
        .clicked()
    {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab(compare_url(project, branch, source)));
        ui.close();
    }
}

/// The tracked MRs merged upstream of `branch` and not in it yet, as a line
/// under it: a click lists them, each opening its page, with the comparison
/// on GitLab for what the tracking does not see. Returns the MR to open.
fn behind(g: &GuiApp, ui: &mut Ui, branch: &str, project: Option<&str>) -> Option<String> {
    let theme = g.theme;
    let mrs = g.app.behind_on(branch);
    if mrs.is_empty() {
        return None;
    }
    let text = tn(
        mrs.len() as u64,
        "↓ {n} MR to bring in",
        "↓ {n} MRs to bring in",
    );
    let line = ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .size(theme.text_size - 2.0)
                    .color(theme.tones.amber.fg),
            )
            .sense(Sense::click()),
        )
    });
    let response = line
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tf(
            "Merged upstream, not in {branch} yet: rebase it, or merge into it",
            &[("branch", branch)],
        ));
    let mut open = None;
    egui::Popup::menu(&response).show(|ui| {
        const WIDTH: f32 = 400.0;
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 2.0;
        // Header: how many, into which branch, and what to do.
        ui.horizontal(|ui| {
            ui.label(semibold(
                tn(
                    mrs.len() as u64,
                    "{n} MR to bring in",
                    "{n} MRs to bring in",
                ),
                theme.text_size,
            ));
            ui.add(
                Chip::new(branch, theme.tones.amber)
                    .font(egui::FontId::monospace(theme.text_size - 2.5)),
            );
        });
        ui.label(
            muted(
                &theme,
                tl("Merged upstream, not in this branch yet: rebase it, or merge into it."),
            )
            .size(theme.text_size - 1.5),
        );
        ui.add_space(6.0);
        for mr in &mrs {
            if let Some(id) = behind_row(g, ui, mr, branch, WIDTH) {
                open = Some(id);
                ui.close();
            }
        }
        let Some(project) = project else {
            return;
        };
        ui.add_space(6.0);
        rule(ui, &theme);
        ui.add_space(6.0);
        let mut sources: Vec<&str> = mrs.iter().map(|mr| mr.target_branch.as_str()).collect();
        sources.sort_unstable();
        sources.dedup();
        ui.horizontal_wrapped(|ui| {
            for source in sources {
                let text = tf("Compare with {source} on GitLab", &[("source", source)]);
                let url = compare_url(project, branch, source);
                if button(ui, &theme, Some(Icon::ExternalLink), &text, false)
                    .on_hover_text(&url)
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
        ui.label(
            muted(
                &theme,
                tl("Only the tracked MRs are listed; GitLab compares every commit."),
            )
            .size(theme.text_size - 2.5),
        );
    });
    open
}

/// One MR of the "to bring in" list: its author, `!iid` and title, where and
/// when it was merged, and how much of it `branch` lacks. A click opens its
/// page (returns its id); the arrow opens it on GitLab.
pub(super) fn behind_row(
    g: &GuiApp,
    ui: &mut Ui,
    mr: &TrackedMr,
    branch: &str,
    width: f32,
) -> Option<String> {
    let theme = g.theme;
    let p = &theme.palette;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tl("Open its page"));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme.radii.medium), p.raised);
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(8.0, 5.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    let name = split_person(&mr.author).0;
    avatar(ui, &theme, name, 26.0).on_hover_text(name);
    ui.add_space(4.0);
    // The arrow to GitLab on the right, the texts in what is left.
    let gitlab = ui
        .with_layout(Layout::right_to_left(Align::Center), |ui| {
            let clicked = icon_button(
                ui,
                tl("Open in GitLab"),
                Icon::ExternalLink,
                &theme.icon_button(),
                24.0,
            )
            .clicked();
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        mono(format!("!{}", mr.id), theme.text_size - 1.5).color(p.text_muted),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&mr.title)
                                .font(fonts::medium(theme.text_size - 1.0))
                                .color(p.text),
                        )
                        .truncate()
                        .selectable(false),
                    );
                });
                let when = mr
                    .merged_at
                    .as_deref()
                    .map(format_relative_date)
                    .unwrap_or_default();
                let mut detail = tf(
                    "merged into {target} {when}",
                    &[("target", &mr.target_branch), ("when", &when)],
                );
                if g.app.branch_presence(mr, branch) == Some(BranchPresence::Partial) {
                    detail = format!("{detail} · {}", tl("partly there"));
                }
                ui.add(
                    egui::Label::new(muted(&theme, detail).size(theme.text_size - 2.5))
                        .truncate()
                        .selectable(false),
                );
            });
            clicked
        })
        .inner;
    if gitlab {
        ui.ctx()
            .open_url(egui::OpenUrl::new_tab(mr.web_url.clone()));
        ui.close();
        return None;
    }
    response.clicked().then(|| mr.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registered filter: the app's, and those of the linked plugins.
    fn all() -> Vec<Entry> {
        gitlab_tracker_core::collect_all_filters()
            .into_iter()
            .enumerate()
            .collect()
    }

    #[test]
    fn every_filter_has_a_place_in_the_panel() {
        let visible = all();
        let groups = Groups::of(&visible);
        let mut placed: Vec<usize> = groups
            .views
            .iter()
            .map(|((i, _), _)| *i)
            .chain(groups.more_views.iter().map(|(i, _)| *i))
            .chain(groups.effort.iter().map(|(i, _)| *i))
            .chain(groups.filters.iter().map(|(i, _)| *i))
            .chain(groups.more_filters.iter().map(|(i, _)| *i))
            .collect();
        placed.sort_unstable();
        let expected: Vec<usize> = visible.iter().map(|(i, _)| *i).collect();
        assert_eq!(placed, expected, "each filter once, none left out");
        // The filters with a value get a field; the others a click.
        for (_, def) in groups.filters.iter().chain(&groups.more_filters) {
            assert!(def.needs_text_input, "{}", def.id);
        }
        for (_, def) in groups
            .views
            .iter()
            .map(|(e, _)| e)
            .chain(&groups.more_views)
        {
            assert!(!def.needs_text_input, "{}", def.id);
        }
    }

    #[test]
    fn the_arrows_cycle_through_the_suggestions() {
        assert_eq!(step(None, 3, true, false), Some(0), "↓ starts at the first");
        assert_eq!(step(None, 3, false, true), Some(2), "↑ starts at the last");
        assert_eq!(step(Some(2), 3, true, false), Some(0), "wraps forward");
        assert_eq!(step(Some(0), 3, false, true), Some(2), "wraps back");
        assert_eq!(step(Some(1), 3, false, false), Some(1), "no key: stays");
        assert_eq!(
            step(Some(4), 3, false, false),
            None,
            "fewer suggestions now"
        );
        assert_eq!(step(Some(0), 0, true, false), None);
    }

    #[test]
    fn the_tui_filters_are_all_registered() {
        let ids: Vec<&str> = all().iter().map(|(_, d)| d.id).collect();
        for id in [
            "all",
            "flagged",
            "state_opened",
            "state_merged",
            "state_closed",
            "mergeability_mergeable",
            "mergeability_conflict",
            "mergeability_rebase",
            "mergeability_not_approved",
            "mergeability_requested_changes",
            "mergeability_draft",
            "mergeability_discussions",
            "has_notes",
            "ci_failing",
            "assigned_to_me",
            "reviewer_me",
            "effort_easy",
            "effort_medium",
            "effort_complex",
            "milestone",
            "target_branch",
            "assignee",
            "absent_from_branch",
            "label",
        ] {
            assert!(ids.contains(&id), "{id}");
        }
    }
}
