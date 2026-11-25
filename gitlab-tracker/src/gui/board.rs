//! What the project pages (team, journal, environments, CI, milestones,
//! overlaps) are built from: the three-column screen — views on the left, the
//! page in the middle, the inspector on the right —, the side panel's header,
//! views and segmented controls, the inspector's sections, and small painted
//! pieces (pills, chips, bars, rows) laid out in exact rects so that nothing
//! overlaps whatever the width.

use std::sync::Arc;

use bevel_theme::{fonts, Chip, Theme, Tone};
use chrono::{DateTime, Utc};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Galley, Layout, Margin, Rect, Sense, Stroke, Ui,
    Vec2,
};

use super::jump::{self, Entry, Jumps};
use super::shell::Sizes;
use super::tip::{Tip, TipExt};
use super::widgets::{focus_ring, muted, rule, semibold};
use super::GuiApp;
use crate::i18n::{tf, tl};

mod figure_row;
mod formats;
mod layout;
mod links;
mod nav;
mod painting;

pub use figure_row::*;
pub use formats::*;
pub use layout::*;
pub use links::*;
pub use nav::*;
pub use painting::*;

/// The gap between cards.
pub const GAP: f32 = 12.0;

/// Draws a project page: `nav` in the left panel and `inspector` in the right
/// one when the window is wide enough for them (each scrolls on its own), and
/// `main` in the middle, scrolling.
pub fn screen(
    g: &mut GuiApp,
    ui: &mut Ui,
    sizes: &Sizes,
    id: &'static str,
    nav: impl FnOnce(&mut GuiApp, &mut Ui),
    inspector: impl FnOnce(&mut GuiApp, &mut Ui),
    main: impl FnOnce(&mut GuiApp, &mut Ui),
) {
    screen_with_jumps(g, ui, sizes, id, None, nav, inspector, |g, ui, _| {
        main(g, ui)
    });
}

/// The sections of a page's middle column, for [`screen_with_jumps`]: where the
/// page keeps them, and the bar's entries.
pub type PageJumps = (fn(&mut GuiApp) -> &mut Jumps, Vec<Entry>);

/// [`screen`], with a bar of shortcuts to the sections of the middle column
/// above it when that column is long. `main` gets the page's [`Jumps`] to wrap
/// its sections in (a default one without a bar).
#[allow(
    clippy::too_many_arguments,
    reason = "screen's arguments, plus the bar"
)]
pub fn screen_with_jumps(
    g: &mut GuiApp,
    ui: &mut Ui,
    sizes: &Sizes,
    id: &'static str,
    jumps: Option<PageJumps>,
    nav: impl FnOnce(&mut GuiApp, &mut Ui),
    inspector: impl FnOnce(&mut GuiApp, &mut Ui),
    main: impl FnOnce(&mut GuiApp, &mut Ui, &mut Jumps),
) {
    let theme = g.theme;
    if let Some(width) = sizes.views {
        egui::Panel::left(egui::Id::unique((id, "nav")))
            .exact_size(width)
            .resizable(false)
            .frame(theme.panel_frame())
            .show(ui, |ui| {
                ui.set_max_width(width);
                let edge = ui.max_rect();
                ui.painter().vline(
                    edge.right() - 0.5,
                    edge.y_range(),
                    Stroke::new(1.0, theme.palette.border),
                );
                egui::ScrollArea::vertical()
                    .id_salt((id, "nav_scroll"))
                    .max_width(ui.available_width())
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let room = ui.available_width();
                        ui.set_width(room);
                        ui.set_max_width(room);
                        nav(g, ui);
                    });
            });
    }
    if let Some(width) = sizes.inspector {
        egui::Panel::right(egui::Id::unique((id, "inspector")))
            .exact_size(width)
            .resizable(false)
            .frame(theme.panel_frame())
            .show(ui, |ui| {
                ui.set_max_width(width);
                let edge = ui.max_rect();
                ui.painter().vline(
                    edge.left() + 0.5,
                    edge.y_range(),
                    Stroke::new(1.0, theme.palette.border),
                );
                egui::ScrollArea::vertical()
                    .id_salt((id, "inspector_scroll"))
                    .max_width(ui.available_width())
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let room = ui.available_width();
                        ui.set_width(room);
                        ui.set_max_width(room);
                        inspector(g, ui);
                    });
            });
    }
    egui::CentralPanel::default()
        .frame(theme.content_frame().inner_margin(Margin {
            left: 24,
            right: 14,
            top: 20,
            bottom: 20,
        }))
        .show(ui, |ui| {
            let (get, entries) = match jumps {
                Some((get, entries)) => (Some(get), entries),
                None => (None, Vec::new()),
            };
            let mut page_jumps = get.map(|get| std::mem::take(get(g))).unwrap_or_default();
            if get.is_some() {
                let room = ui.available_width() - 10.0;
                ui.allocate_ui(Vec2::new(room, 0.0), |ui| {
                    if jump::bar(ui, &theme, &mut page_jumps, &entries).is_some() {
                        ui.ctx().request_repaint();
                    }
                });
                if page_jumps.shown() {
                    ui.add_space(10.0);
                }
            }
            let output = egui::ScrollArea::vertical()
                .id_salt((id, "main_scroll"))
                .max_width(ui.available_width())
                .auto_shrink(false)
                .show(ui, |ui| {
                    // Room on the right for the scroll bar, over nothing.
                    let room = ui.available_width() - 10.0;
                    ui.set_width(room);
                    ui.set_max_width(room);
                    page_jumps.begin(ui);
                    main(g, ui, &mut page_jumps);
                    ui.add_space(8.0);
                });
            if let Some(get) = get {
                page_jumps.end(&output);
                *get(g) = page_jumps;
            }
        });
}
