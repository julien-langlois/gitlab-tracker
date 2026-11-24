//! The merge requests table and its toolbar.
//!
//! Rows are drawn by hand rather than with a table widget: each one is a rounded
//! card (selection ring, highlight of the MRs updated by the last refresh), and
//! only the rows in view are laid out.

use bevel_theme::{fonts, Chip, Theme, Tone};
use bevel_widgets::buttons::IconButton;
use egui::{
    Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    Vec2,
};

use super::icons::{self, Icon};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{
    activity, avatar, badge, button, button_width_for, field, icon_button, icon_toggle, label_tone,
    mono, muted, semibold, split_person, toned_label_chip, FieldLook,
};
use super::{tones, GuiApp, Page};
use crate::actions::Action;
use crate::app::{App, InputMode, SortColumn, SortOrder};
use crate::i18n::{tf, tl, tn};
use crate::models::{Effort, GitlabMrState, MrStatus};

mod body;
mod cells;
mod columns;
mod export;
mod filters;
mod row_model;
mod status;
mod toolbar;

use body::*;
use cells::*;
pub use columns::*;
pub(in crate::gui) use export::*;
use filters::*;
pub use row_model::*;
pub use status::*;
use toolbar::*;

pub fn view(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let shown = g.app.visible_mrs().count();
    toolbar(g, ui, shown, sizes);
    super::shell::fetch_errors(g, ui);
    super::merged_list::banner(g, ui);
    status_bar(g, ui);
    next_chips(g, ui);
    active_filters(g, ui);
    bulk_bar(g, ui);

    if shown == 0 {
        empty_state(g, ui);
        return;
    }

    let inspector_shown = sizes.inspector.is_some();
    egui::Frame::new()
        .inner_margin(Margin::symmetric(8, 0))
        .show(ui, |ui| table(g, ui, shown, inspector_shown));
}

/// What the bulk bar asked for, applied once the frame is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bulk {
    Flag(bool),
    CopyChecklist,
    StopTracking,
}

/// A quick pick of the bulk bar, among the MRs shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickPick {
    All,
    State(GitlabMrState),
    None,
}

/// The ids of the MRs shown that `pick` takes.
fn quick_pick(app: &App, pick: QuickPick) -> Vec<String> {
    app.visible_mrs()
        .filter(|mr| match pick {
            QuickPick::All => true,
            QuickPick::State(state) => mr.state == state,
            QuickPick::None => false,
        })
        .map(|mr| mr.id.clone())
        .collect()
}

/// The MRs picked (in bulk mode, or with Ctrl / Shift + click), and what to do
/// with them all.
fn bulk_bar(g: &mut GuiApp, ui: &mut Ui) {
    // Gone from the list: no longer picked.
    let ids: std::collections::HashSet<String> = g.app.mrs.iter().map(|m| m.id.clone()).collect();
    g.marked.retain(|id| ids.contains(id));
    if g.marked.is_empty() && !g.bulk_mode {
        return;
    }
    let theme = g.theme;
    let n = g.marked.len();
    let closed = quick_pick(&g.app, QuickPick::State(GitlabMrState::Closed)).len();
    let merged = quick_pick(&g.app, QuickPick::State(GitlabMrState::Merged)).len();
    let shown = g.app.visible_mrs().count();
    let mut picked = None;
    let mut quick = None;
    let mut clear = false;
    let read_only = g.app.read_only;
    let bulk_mode = g.bulk_mode;
    egui::Frame::new()
        .fill(theme.palette.selection)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 6))
        .outer_margin(Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(semibold(
                    tn(n as u64, "{n} MR selected", "{n} MRs selected"),
                    theme.text_size,
                ))
                .tip(tl("Ctrl+click adds one · Shift+click a range"));
                if bulk_mode {
                    ui.add_space(4.0);
                    ui.label(muted(&theme, tl("Pick:")));
                    let picks = [
                        (
                            QuickPick::All,
                            tf("All shown ({n})", &[("n", &shown.to_string())]),
                            shown,
                        ),
                        (
                            QuickPick::State(GitlabMrState::Closed),
                            tf("Closed ({n})", &[("n", &closed.to_string())]),
                            closed,
                        ),
                        (
                            QuickPick::State(GitlabMrState::Merged),
                            tf("Merged ({n})", &[("n", &merged.to_string())]),
                            merged,
                        ),
                    ];
                    for (pick, label, count) in picks {
                        let r = ui.add_enabled_ui(count > 0, |ui| {
                            button(ui, &theme, None, &label, false)
                        });
                        if r.inner
                            .tip(tl("Adds them to the MRs selected, among the MRs shown"))
                            .clicked()
                        {
                            quick = Some(pick);
                        }
                    }
                    let none =
                        ui.add_enabled_ui(n > 0, |ui| button(ui, &theme, None, tl("None"), false));
                    if none.inner.clicked() {
                        quick = Some(QuickPick::None);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = tl(if bulk_mode { "Done" } else { "Clear" });
                    if button(ui, &theme, Some(Icon::Close), label, false).clicked() {
                        clear = true;
                    }
                    if !bulk_mode {
                        bulk_actions(ui, &theme, n, read_only, &mut picked);
                    }
                });
            });
            // Bulk mode: the actions on a line of their own, under the picks.
            if bulk_mode {
                ui.add_space(4.0);
                let line = Vec2::new(ui.available_width(), theme.spacing.control_height + 2.0);
                ui.allocate_ui_with_layout(line, Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    bulk_actions(ui, &theme, n, read_only, &mut picked);
                });
            }
        });
    match quick {
        Some(QuickPick::None) => g.marked.clear(),
        Some(pick) => g.marked.extend(quick_pick(&g.app, pick)),
        None => {}
    }
    if clear {
        g.marked.clear();
        g.bulk_mode = false;
    }
    g.bulk = picked.or(g.bulk);
}

/// The bulk bar's actions, right to left; disabled while nothing is picked.
fn bulk_actions(ui: &mut Ui, theme: &Theme, n: usize, read_only: bool, picked: &mut Option<Bulk>) {
    ui.add_enabled_ui(n > 0 && !read_only, |ui| {
        let stop = button(ui, theme, None, tl("Stop tracking…"), false);
        egui::Popup::menu(&stop).show(|ui| {
            ui.label(tn(
                n as u64,
                "Stop tracking {n} MR?",
                "Stop tracking these {n} MRs?",
            ));
            if ui.button(tl("Stop tracking")).clicked() {
                *picked = Some(Bulk::StopTracking);
            }
        });
    });
    ui.add_enabled_ui(n > 0, |ui| {
        if button(
            ui,
            theme,
            Some(Icon::Clipboard),
            tl("Copy checklist"),
            false,
        )
        .clicked()
        {
            *picked = Some(Bulk::CopyChecklist);
        }
        if button(ui, theme, Some(Icon::StarOutline), tl("Unflag"), false).clicked() {
            *picked = Some(Bulk::Flag(false));
        }
        if button(ui, theme, Some(Icon::Star), tl("Flag"), false).clicked() {
            *picked = Some(Bulk::Flag(true));
        }
    });
}

// ── Toolbar ──────────────────────────────────────────────────────────────────

/// How the table groups its rows, under foldable headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    None,
    Status,
    Milestone,
    Author,
    /// Who each MR waits on (see `crate::next_step`).
    NextStep,
    /// The day it was merged, in local time: the day's count on the heading.
    MergedDay,
}

impl GroupBy {
    const ALL: [GroupBy; 6] = [
        GroupBy::None,
        GroupBy::Status,
        GroupBy::Milestone,
        GroupBy::Author,
        GroupBy::NextStep,
        GroupBy::MergedDay,
    ];

    pub(super) fn label(self) -> &'static str {
        tl(match self {
            GroupBy::None => "No grouping",
            GroupBy::Status => "By status",
            GroupBy::Milestone => "By milestone",
            GroupBy::Author => "By author",
            GroupBy::NextStep => "By next step",
            GroupBy::MergedDay => "By merge day",
        })
    }
}

fn sort_label(column: SortColumn) -> &'static str {
    tl(match column {
        SortColumn::UpdatedAt => "Updated",
        SortColumn::Id => "MR",
        SortColumn::Milestone => "Milestone",
        SortColumn::Title => "Title",
        SortColumn::MergedAt => "Merge date",
    })
}

#[cfg(test)]
mod tests;
