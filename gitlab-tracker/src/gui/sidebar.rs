//! The views panel: the project, the views with their counts, the filters that
//! take a value (label with suggestions, review effort, absent from branch…), the
//! tracked branches and the "hide merged" switch.

use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use gitlab_tracker_core::FilterDef;

use super::icons::Icon;
use super::tip::{Tip, TipExt};
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

mod branch_list;
mod filters;
mod search_box;
mod views;

pub(in crate::gui) use branch_list::*;
use filters::*;
use search_box::*;
use views::*;

/// Views listed first, with their glyph; the other one-click filters are under
/// "More views".
const MAIN_VIEWS: &[(&str, &str)] = &[
    ("all", "≡"),
    ("flagged", "★"),
    ("reviewer_me", "◉"),
    ("assigned_to_me", "@"),
    ("merged_by_me", "✓"),
    ("has_notes", "✎"),
    ("ci_failing", "✕"),
];

/// Filters with a value shown directly; the others are under "More filters".
const MAIN_FILTERS: &[&str] = &["label", "absent_from_branch", "merged_between"];

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
            super::views::section(g, ui);
            ui.add_space(12.0);
            egui::Frame::new()
                .inner_margin(Margin {
                    left: 6,
                    right: 6,
                    top: 0,
                    bottom: 6,
                })
                .show(ui, |ui| section_title(ui, &theme, tl("Views")));
            for (i, &((index, def), glyph)) in groups.views.iter().enumerate() {
                view_item(g, ui, index, def, glyph);
                // Who the MRs wait on, under "All".
                if i == 0 {
                    next_views(g, ui);
                }
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
                let active_inside = more.iter().any(|(i, _)| filter_on(g, *i).is_some());
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
                .tip(tl("They stay tracked and refreshed (M)"))
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

#[cfg(test)]
mod tests;
