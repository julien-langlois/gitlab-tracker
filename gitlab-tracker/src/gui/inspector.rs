//! The inspector: everything about the selected MR in one scrolling column —
//! review, effort, branches, labels, linked ticket, threads, pipelines, people,
//! planning and history.

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use super::mr_page::display_of;
use super::tip::{Tip, TipExt};
use super::widgets::{
    activity, avatar, badge, bar, button, dot, icon_button, key_values, label_chip, mono, muted,
    section, semibold, spinner, split_person,
};
use super::{tones, GuiApp, InspectorSection, Page};
use crate::actions::Action;
use crate::app::{TimeLogState, TrackerView};
use crate::i18n::{tf, tl, tn};
use crate::models::{BranchPresence, Effort, MrStatus, PipelineState, TrackedMr};
use crate::utils::{
    awaiting_review_by, format_relative_date, matches_gitlab_username, review_waiting_since,
};

mod branch_chips;
mod review_card;
mod sections;
mod steps;
mod ticket_card;
mod top;

pub(in crate::gui) use branch_chips::*;
use review_card::*;
pub(in crate::gui) use sections::*;
pub(in crate::gui) use steps::*;
pub use ticket_card::*;
use top::*;

pub fn panel(g: &mut GuiApp, ui: &mut Ui, width: f32) {
    let theme = g.theme;
    egui::Panel::right("inspector")
        .exact_size(width)
        .resizable(false)
        .frame(theme.panel_frame())
        .show(ui, |ui| {
            let edge = ui.max_rect();
            ui.painter().vline(
                edge.left() + 0.5,
                edge.y_range(),
                Stroke::new(1.0, theme.palette.border),
            );
            // Content wider than the panel would push the panel off the window:
            // keep it to the panel's width (long text wraps).
            ui.set_max_width(ui.available_width());
            let Some(mr) = g.app.selected_mr().cloned() else {
                ui.add_space(48.0);
                ui.vertical_centered(|ui| ui.label(muted(&theme, tl("Select a merge request"))));
                return;
            };
            header(g, ui, &mr);
            next_card(g, ui, &mr);
            anchors(g, ui, &mr);
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    body(g, ui, &mr);
                });
        });
}

/// Who the MR waits on, under the header (which stays in place).
fn next_card(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
    let mut action = None;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 0,
            bottom: 12,
        })
        .show(ui, |ui| {
            action = super::next::card(ui, &theme, mr, &step, false);
        });
    if let Some(action) = action {
        super::next::apply(g, action, false);
    }
}

/// The sections as tabs under the header (which stays in place): a click
/// scrolls the body to that section.
fn anchors(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let mut links = vec![
        (InspectorSection::Review, tl("Review").to_string()),
        (InspectorSection::Changes, tl("Changes").to_string()),
        (InspectorSection::Branches, tl("Branches").to_string()),
    ];
    let open_threads = mr.threads.iter().filter(|t| !t.resolved).count();
    if open_threads > 0 {
        links.push((
            InspectorSection::Threads,
            format!("{} {open_threads}", tl("Threads")),
        ));
    }
    if g.app.tracker.is_some() || mr.linked_ticket.is_some() {
        links.push((InspectorSection::Ticket, tl("Ticket").to_string()));
    }
    links.push((InspectorSection::Pipelines, tl("Pipelines").to_string()));
    if !mr.description.trim().is_empty() {
        links.push((InspectorSection::Description, tl("Description").to_string()));
    }
    let current = g.inspector_tab;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 0,
            bottom: 12,
        })
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(theme.palette.content)
                .stroke(Stroke::new(1.0, theme.palette.border_soft))
                .corner_radius(egui::CornerRadius::same(theme.radii.large))
                .inner_margin(Margin::same(3))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                    let gap = 2.0;
                    let width = ui.available_width();
                    let items: Vec<_> = links
                        .into_iter()
                        .map(|(section, name)| {
                            let galley = ui.painter().layout_no_wrap(
                                name,
                                fonts::medium(theme.text_size - 1.5),
                                theme.palette.text_secondary,
                            );
                            let w = galley.size().x + 18.0;
                            (section, galley, w)
                        })
                        .collect();
                    // Into lines that fit, each stretched to the full width: no
                    // gap left at the end of the bar.
                    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
                    let mut used = 0.0;
                    for (i, (_, _, w)) in items.iter().enumerate() {
                        let line = lines.last_mut().expect("one line at least");
                        let need = if line.is_empty() { *w } else { used + gap + w };
                        if !line.is_empty() && need > width {
                            lines.push(vec![i]);
                            used = *w;
                        } else {
                            line.push(i);
                            used = need;
                        }
                    }
                    // As many tabs on each line when they fit that way: 3 and 3
                    // rather than 4 and 2 stretched wide.
                    if lines.len() > 1 {
                        let per = items.len().div_ceil(lines.len());
                        let even: Vec<Vec<usize>> = (0..items.len())
                            .collect::<Vec<_>>()
                            .chunks(per)
                            .map(<[usize]>::to_vec)
                            .collect();
                        let fits = |line: &Vec<usize>| {
                            line.iter().map(|&i| items[i].2).sum::<f32>()
                                + gap * (line.len() as f32 - 1.0)
                                <= width
                        };
                        if even.len() == lines.len() && even.iter().all(fits) {
                            lines = even;
                        }
                    }
                    for line in lines {
                        let natural: f32 = line.iter().map(|&i| items[i].2).sum::<f32>()
                            + gap * (line.len() as f32 - 1.0);
                        // Half a point short: rounded over, it would widen the panel.
                        let extra = ((width - natural - 0.5) / line.len() as f32).max(0.0);
                        ui.horizontal(|ui| {
                            for &i in &line {
                                let (section, galley, w) = &items[i];
                                let size = Vec2::new(w + extra, galley.size().y + 12.0);
                                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                                let response =
                                    response.on_hover_cursor(egui::CursorIcon::PointingHand);
                                let radius = egui::CornerRadius::same(theme.radii.medium);
                                let on = current == *section;
                                let color = if on {
                                    ui.painter()
                                        .rect_filled(rect, radius, theme.palette.selection);
                                    theme.palette.text
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, radius, theme.palette.raised);
                                    theme.palette.text
                                } else {
                                    theme.palette.text_secondary
                                };
                                ui.painter().galley(
                                    rect.center() - galley.size() / 2.0,
                                    galley.clone(),
                                    color,
                                );
                                if response.clicked() {
                                    g.scroll_to = Some(*section);
                                    g.inspector_tab = *section;
                                }
                            }
                        });
                    }
                });
        });
    super::widgets::rule(ui, &theme);
}

/// Scrolls the body to here when `section` was asked for.
fn anchor(g: &mut GuiApp, ui: &mut Ui, section: InspectorSection) {
    if g.scroll_to == Some(section) {
        ui.scroll_to_cursor(Some(Align::TOP));
        g.scroll_to = None;
    }
}
