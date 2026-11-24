//! The MR page: the selected MR on a page of its own (double click or Enter on
//! the table, Esc back, J / K to the next or previous MR).
//!
//! Left, the work: why it cannot merge, its description (folded when long), then
//! its discussions, changes, commits and pipelines. Right, the context: a
//! timeline, approvals, related MRs, how it compares, and its planning.
//! What the list does not keep (merge checks, approval rules, the threads in full,
//! deployments) is read when the page opens (`gitlab_detail`).

use bevel_theme::{fonts, Chip, Theme, Tone};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Ui, Vec2};

use super::details::Folder;
use super::icons::Icon;
use super::inspector::{self, Step};
use super::tip::{Tip, TipExt};
use super::widgets::{
    avatar, badge, button, label_chip, mono, muted, semibold, spinner, split_person,
};
use super::{details, tones, GuiApp, Page};
use crate::actions::Action;
use crate::app::App;
use crate::gitlab_detail::{ConflictFile, Overview, PageData, Rule, Thread};
use crate::i18n::{tc, tf, tl, tn};
use crate::models::{
    BranchPresence, Effort, GitlabMrState, MilestoneEvent, MrStatus, PipelineState, TrackedMr,
};
use crate::utils::{format_relative_date, matches_gitlab_username};
use gitlab_tracker_core::MergeabilityStatus;

mod checks;
mod commits;
mod discussion;
mod header;
mod side;
mod timeline;

pub use checks::*;
use commits::*;
use discussion::*;
pub use header::*;
pub(in crate::gui) use side::*;
pub use timeline::*;

/// The left column's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Discussions,
    Changes,
    Commits,
    Pipelines,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Discussions, Tab::Changes, Tab::Commits, Tab::Pipelines];

    /// Its name as a section of the page's bar.
    fn id(self) -> &'static str {
        match self {
            Tab::Discussions => "discussions",
            Tab::Changes => "changes",
            Tab::Commits => "commits",
            Tab::Pipelines => "pipelines",
        }
    }

    fn from_id(id: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// A tab: its name, its count, and a dot with what it says.
type TabLine = (
    Tab,
    &'static str,
    usize,
    Option<egui::Color32>,
    Option<String>,
);

/// The section the tabs' card is, in the page's bar.
const TABS: &str = "tabs";

/// The right column's width, and the page width below which it goes under.
const SIDE_WIDTH: f32 = 420.0;
/// The context tiles' height, and the page width below which they stack.
const TILE_HEIGHT: f32 = 158.0;
/// A branch line in the "Where it is" tile.
const BRANCH_LINE: f32 = 26.0;
/// Branch lines shown before "+N more branches".
const MAX_BRANCH_LINES: usize = 8;
const TILES_IN_A_ROW: f32 = 760.0;
const TWO_COLUMNS: f32 = 1060.0;

pub fn page(g: &mut GuiApp, ui: &mut Ui) {
    let Some(mr) = g.app.selected_mr().cloned() else {
        return;
    };
    let data = g.details.page(&g.app, &mr, ui.ctx()).cloned();
    crumbs(g, ui, &mr);
    ui.add_space(10.0);
    header(g, ui, &mr);
    ui.add_space(14.0);
    let mut jumps = std::mem::take(&mut g.mr_jumps);
    let (open, fact) = facts(g, ui, &mr, data.as_ref());
    if let Some(id) = open {
        g.open_mr(&id);
    }
    if let Some(id) = fact {
        jump_to(g, &mut jumps, id);
    }
    ui.add_space(14.0);

    let width = ui.available_width();
    let entries = jump_entries(g, &mr, data.as_ref(), width < TWO_COLUMNS);
    if let Some(id) = super::jump::bar(ui, &g.theme, &mut jumps, &entries) {
        if let Some(tab) = Tab::from_id(id) {
            g.mr_tab = tab;
        }
    }
    if jumps.shown() {
        ui.add_space(12.0);
    }
    let output = egui::ScrollArea::vertical()
        .id_salt(("mr_page", &mr.id))
        .auto_shrink(false)
        .show(ui, |ui| {
            jumps.begin(ui);
            next_banner(g, ui, &mr);
            // On the merge train, or in the queue of the MRs set to auto-merge.
            let theme = g.theme;
            if super::merge_queue::card(ui, &theme, &g.queues, &mr, chrono::Utc::now()) {
                ui.add_space(12.0);
            }
            tiles(g, ui, &mr, data.as_ref(), width);
            ui.add_space(12.0);
            lifecycle(g, ui, &mr);
            ui.add_space(16.0);
            if width >= TWO_COLUMNS {
                let left = width - SIDE_WIDTH - 16.0;
                // Each column in a rect of its own, clipped to it: content that
                // does not wrap (a long path, a wide table) can never spill into
                // the other column.
                let top = ui.cursor().min;
                let viewport = ui.clip_rect();
                let column = |x: f32, w: f32| {
                    egui::Rect::from_min_size(egui::pos2(x, top.y), Vec2::new(w, f32::INFINITY))
                };
                let left_rect = column(top.x, left);
                let right_rect = column(top.x + left + 16.0, SIDE_WIDTH);
                let mut left_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                left_ui.set_clip_rect(left_rect.intersect(viewport));
                work(g, &mut left_ui, &mut jumps, &mr, data.as_ref());
                let mut right_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(Layout::top_down(Align::Min)),
                );
                right_ui.set_clip_rect(right_rect.intersect(viewport));
                context(g, &mut right_ui, &mut jumps, &mr, data.as_ref());
                let height = left_ui
                    .min_rect()
                    .height()
                    .max(right_ui.min_rect().height());
                ui.allocate_space(Vec2::new(width, height));
            } else {
                work(g, ui, &mut jumps, &mr, data.as_ref());
                ui.add_space(16.0);
                context(g, ui, &mut jumps, &mr, data.as_ref());
            }
            ui.add_space(24.0);
        });
    jumps.end(&output);
    g.mr_jumps = jumps;
}

/// Scrolls to the section `id` of the page's bar; a tab's opens it too.
pub(super) fn jump_to(g: &mut GuiApp, jumps: &mut super::jump::Jumps, id: &'static str) {
    match Tab::from_id(id) {
        Some(tab) => {
            g.mr_tab = tab;
            jumps.ask(TABS);
        }
        None => jumps.ask(id),
    }
}

/// The page's sections, for its bar: what blocks, the description, the tabs
/// (with their counts and dots), and, when the context goes under the work
/// (a narrow window), its cards. Side by side, they are in view with the work.
fn jump_entries(
    g: &mut GuiApp,
    mr: &TrackedMr,
    data: Option<&PageData>,
    narrow: bool,
) -> Vec<super::jump::Entry> {
    use super::jump::Entry;
    let mut entries = Vec::new();
    if mr.state == GitlabMrState::Opened {
        let failing = checks(
            mr,
            data.and_then(|d| d.overview.as_ref()),
            data.and_then(|d| d.rules.as_deref()),
            data.and_then(|d| d.threads.as_deref().ok()),
            data.and_then(|d| d.conflicts.as_deref()),
        )
        .iter()
        .filter(|c| c.verdict == Verdict::Blocks)
        .count();
        let entry = Entry::new("blockers", tl("Blockers"));
        entries.push(if failing > 0 {
            entry.count(failing)
        } else {
            entry
        });
    }
    if mr.state == GitlabMrState::Opened {
        entries.push(Entry::new("reviewers", tl("Reviewers")));
    }
    if !mr.description.trim().is_empty() {
        entries.push(Entry::new("description", tl("Description")));
    }
    let current = g.mr_tab;
    for (i, (tab, label, n, alert, tip)) in tabs(g, mr, data).into_iter().enumerate() {
        let entry = Entry::new(tab.id(), label)
            .anchor(TABS)
            .count(n)
            .dot(alert, tip)
            .lit(current == tab);
        entries.push(if i == 0 { entry.group() } else { entry });
    }
    if narrow {
        entries.push(Entry::new("timeline", tl("Timeline")).group());
        if data.and_then(|d| d.rules.as_ref()).is_some() {
            entries.push(Entry::new("approvals", tl("Approvals")));
        }
        if !related_mrs(&g.app, mr).is_empty() {
            entries.push(Entry::new("related", tl("Related MRs")));
        }
        entries.push(Entry::new("planning", tl("Planning")));
    }
    entries
}

/// The left column's tabs: name, count, and a dot when something waits there
/// (red, or amber for a signature not verified), with what it says.
fn tabs(g: &mut GuiApp, mr: &TrackedMr, data: Option<&PageData>) -> [TabLine; 4] {
    let theme = g.theme;
    let threads = data.and_then(|d| d.threads.as_ref().ok());
    let changed_files = g.details.diff(&mr.id).map(<[_]>::len);
    // A dot on Commits when a signature is missing (red) or not verified (amber).
    let commits_alert = g.app.signature_report(mr).and_then(|r| {
        let s = r.summary();
        if !s.unsigned.is_empty() || !s.system.is_empty() {
            Some((theme.tones.red.fg, tl("A commit is not signed")))
        } else if !s.unverified.is_empty() {
            Some((theme.tones.amber.fg, tl("A signature is not verified")))
        } else {
            None
        }
    });
    // A red dot on Discussions while a thread is unresolved.
    let open = unresolved(mr, data);
    let discussions_alert = (open > 0).then(|| {
        (
            theme.tones.red.fg,
            tn(
                open as u64,
                "{n} unresolved thread",
                "{n} unresolved threads",
            ),
        )
    });
    // A red dot on Pipelines when the latest pipeline failed.
    let pipelines_alert = mr
        .pipelines
        .first()
        .is_some_and(|p| p.status == PipelineState::Failed)
        .then_some((theme.tones.red.fg, tl("The latest pipeline failed")));
    [
        (
            Tab::Discussions,
            tl("Discussions"),
            threads.map_or(mr.user_notes_count as usize, |t| t.len()),
            discussions_alert.as_ref().map(|a| a.0),
            discussions_alert.map(|a| a.1),
        ),
        (
            Tab::Changes,
            tl("Changes"),
            // The diff read, else GitLab's count (the demo has none).
            changed_files.unwrap_or_else(|| {
                mr.diff_stats
                    .as_ref()
                    .map_or(0, |s| s.files_changed as usize)
            }),
            None,
            None,
        ),
        (
            Tab::Commits,
            tl("Commits"),
            mr.commits.len(),
            commits_alert.map(|a| a.0),
            commits_alert.map(|a| a.1.to_string()),
        ),
        (
            Tab::Pipelines,
            tl("Pipelines"),
            mr.pipelines.len(),
            pipelines_alert.map(|a| a.0),
            pipelines_alert.map(|a| a.1.to_string()),
        ),
    ]
}

/// Who the MR waits on, what they have to do, and what comes after.
fn next_banner(g: &mut GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
    if step.actor == crate::next_step::Actor::Unknown {
        return;
    }
    let action = super::next::card(ui, &theme, mr, &step, true);
    ui.add_space(12.0);
    if let Some(action) = action {
        super::next::apply(g, action, true);
    }
}

/// The MR's stages in a line, with the time each hop took against the usual.
fn lifecycle(g: &GuiApp, ui: &mut Ui, mr: &TrackedMr) {
    let theme = g.theme;
    let step = crate::next_step::of(&g.app, mr, g.app.config.gitlab_username.as_deref());
    let stages = super::next::lifecycle(&g.app, mr, &step);
    let tone = super::next::tone(&step, &theme);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(egui::Stroke::new(1.0, theme.palette.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                tile_title(ui, &theme, tl("Lifecycle"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        muted(
                            &theme,
                            tl("Time between steps · the median of the tracked MRs in grey"),
                        )
                        .size(theme.text_size - 2.5),
                    );
                });
            });
            super::next::lifecycle_strip(ui, &theme, &stages, tone);
        });
}

// ── Top ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
