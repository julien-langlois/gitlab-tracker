//! The window's frame: title bar with the project tabs, icon rail, status bar, and
//! the responsive split between the side panels and the page.

use bevel_theme::{fonts, Theme};
use egui::{Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use super::tip::{Tip, TipExt};
use super::widgets::{dot, icon_toggle, muted, spinner};
use super::{cockpit, dialogs, inspector, sidebar, table, tracker, GuiApp, Page};
use crate::actions::Action;
use crate::app::InputMode;
use crate::i18n::{tf, tl, tn};

mod countdowns;
mod footer;
mod icon_rail;
mod tab_strip;
mod title_bar_view;

use countdowns::*;
use footer::*;
use icon_rail::*;
pub use tab_strip::*;
pub use title_bar_view::*;

pub fn draw(g: &mut GuiApp, ui: &mut Ui) {
    let width = ui.max_rect().width();
    let page = g.page();
    let mut sizes = Sizes::for_width(width, g.show_views, g.show_inspector);
    g.views_shown = page == Page::MergeRequests && sizes.views.is_some();
    if page == Page::Tracker {
        sizes.widen_inspector(width);
    }
    title_bar(g, ui, &sizes, page);
    status_bar(g, ui);
    // The MR page belongs to the merge requests, a person's page to the team.
    rail(
        g,
        ui,
        match page {
            Page::Mr => Page::MergeRequests,
            Page::Person => Page::Team,
            page => page,
        },
    );

    let content = g.theme.content_frame();
    match page {
        #[cfg(feature = "stats")]
        Page::Stats => super::stats::screen(g, ui, &sizes),
        Page::Cockpit => {
            egui::CentralPanel::default()
                .frame(content.inner_margin(Margin::symmetric(24, 20)))
                .show(ui, |ui| cockpit::page(g, ui));
        }
        Page::Promote => {
            egui::CentralPanel::default()
                .frame(content.inner_margin(Margin::symmetric(24, 20)))
                .show(ui, |ui| super::promote::page(g, ui));
        }
        Page::Mr => {
            egui::CentralPanel::default()
                .frame(content.inner_margin(Margin::symmetric(24, 16)))
                .show(ui, |ui| super::mr_page::page(g, ui));
        }
        Page::Today => {
            egui::CentralPanel::default()
                .frame(content.inner_margin(Margin::symmetric(24, 20)))
                .show(ui, |ui| super::today::page(g, ui));
        }
        Page::Team => super::team::screen(g, ui, &sizes),
        Page::Person => {
            egui::CentralPanel::default()
                .frame(content.inner_margin(Margin::symmetric(24, 16)))
                .show(ui, |ui| super::person::page(g, ui));
        }
        Page::Journal => super::journal::screen(g, ui, &sizes),
        Page::Environments => super::environments::screen(g, ui, &sizes),
        Page::Ci => super::ci::screen(g, ui, &sizes),
        Page::Milestones => super::milestones::screen(g, ui, &sizes),
        Page::Overlaps => super::overlaps::screen(g, ui, &sizes),
        Page::Delivery => super::delivery::screen(g, ui, &sizes),
        Page::Releases => super::releases::screen(g, ui, &sizes),
        Page::Stacks => super::stacks::screen(g, ui, &sizes),
        Page::MergeRequests | Page::Tracker => {
            if let (Page::MergeRequests, Some(width)) = (page, sizes.views) {
                sidebar::panel(g, ui, width);
            }
            if let Some(width) = sizes.inspector {
                inspector::panel(g, ui, width);
            }
            egui::CentralPanel::default()
                .frame(content)
                .show(ui, |ui| match page {
                    Page::Tracker => tracker::page(g, ui),
                    _ => table::view(g, ui, &sizes),
                });
        }
    }

    dialogs::show(g, ui.ctx());
    super::views::dialog(g, ui.ctx());
    super::changelog::show(g, ui.ctx());
    super::palette::show(g, ui.ctx());
    super::toasts::show(g, ui.ctx());
}

// ── Responsive layout ────────────────────────────────────────────────────────

/// Below this window width the views panel starts hidden.
const VIEWS_PANEL_BREAKPOINT: f32 = 1440.0;
/// Below this window width the inspector starts hidden.
const INSPECTOR_BREAKPOINT: f32 = 1100.0;
/// Width of the icon rail.
const RAIL_WIDTH: f32 = 52.0;
/// What the tracker page keeps for its list when the inspector widens.
const TRACKER_LIST_MIN_WIDTH: f32 = 640.0;

/// Which side panels show, and how wide, for the window's width in points.
///
/// Points, not pixels: egui already scales them to the screen's density, so a 4K
/// screen at 200 % lays out like 1080p, and an ultrawide at 100 % gets the extra room.
/// `show_*` are the user's choices from the toggle buttons; `None` follows the width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    pub views: Option<f32>,
    pub inspector: Option<f32>,
    /// Drop secondary text from the title bar.
    pub compact: bool,
}

impl Sizes {
    pub fn for_width(width: f32, show_views: Option<bool>, show_inspector: Option<bool>) -> Self {
        let views = show_views.unwrap_or(width >= VIEWS_PANEL_BREAKPOINT);
        let inspector = show_inspector.unwrap_or(width >= INSPECTOR_BREAKPOINT);
        Self {
            views: views.then(|| (width * 0.16).clamp(220.0, 280.0)),
            inspector: inspector.then(|| (width * 0.22).clamp(300.0, 460.0)),
            compact: width < INSPECTOR_BREAKPOINT,
        }
    }

    /// A wider inspector, for the tracker page: its list needs less room than the
    /// table, and the ticket card more. Never narrower than the usual inspector, and
    /// the list keeps [`TRACKER_LIST_MIN_WIDTH`].
    pub fn widen_inspector(&mut self, width: f32) {
        if let Some(inspector) = &mut self.inspector {
            let room = width - RAIL_WIDTH - TRACKER_LIST_MIN_WIDTH;
            *inspector = (width * 0.30).clamp(340.0, 580.0).min(room).max(*inspector);
        }
    }
}

// ── Title bar ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
