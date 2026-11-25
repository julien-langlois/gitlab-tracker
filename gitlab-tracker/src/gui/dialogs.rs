//! Dialogs, shown for the app's popup modes like the TUI's popups: settings,
//! columns, project picker, log time and keyboard shortcuts.
//!
//! They share one shell: a header (the dialog's icon on a tile, its title and
//! a line about it, a close button), the body, then a footer holding the
//! buttons on the panel's color.

use bevel_theme::Theme;
use egui::{
    Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2,
};
use gitlab_tracker_core::{ProjectSettingKind, ProjectSettingValue};

use super::board::one_line;
use super::icons::{self, Icon};
use super::table;
use super::tip::TipExt;
use super::widgets::{button, focus_ring, muted, primary_button, rule, semibold, switch};
use super::GuiApp;
use crate::actions::Action;
use crate::app::{InputMode, LogTimeForm};
use crate::i18n::{tf, tl, tn};

mod columns;
mod help;
mod projects;
mod settings;
mod settings_rows;

use columns::*;
use help::*;
use projects::*;
use settings::*;
use settings_rows::*;

pub fn show(g: &mut GuiApp, ctx: &egui::Context) {
    if g.wizard.is_some() {
        wizard(g, ctx);
        return;
    }
    // Wide where the window allows: the settings and the shortcuts read in
    // columns, the column picker as a grid of cards.
    let room = ctx.content_rect().width() - 96.0;
    let (icon, title, width): (Icon, &str, f32) = match g.app.input_mode {
        InputMode::Settings => (Icon::Settings, tl("Settings"), room.clamp(560.0, 940.0)),
        InputMode::ColumnPicker => (Icon::Columns, tl("Columns"), room.clamp(480.0, 700.0)),
        InputMode::ProjectPicker => (Icon::Home, tl("Projects"), room.clamp(440.0, 560.0)),
        InputMode::LogTime => (Icon::Clock, tl("Log time"), room.clamp(400.0, 480.0)),
        InputMode::Help => (
            Icon::Help,
            tl("Keyboard shortcuts"),
            room.clamp(520.0, 1040.0),
        ),
        _ => return,
    };
    let subtitle = subtitle(g);
    let theme = g.theme;
    // A pixel of margin keeps the frame's border clear of the footer's fill.
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(1));
    let mut closed = false;
    let response = egui::Modal::new(egui::Id::unique("dialog"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            closed = header(ui, &theme, icon, title, subtitle.as_deref());
            rule(ui, &theme);
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing.item.y;
                match g.app.input_mode {
                    InputMode::Settings => settings(g, ui),
                    InputMode::ColumnPicker => columns(g, ui),
                    InputMode::ProjectPicker => projects(g, ui),
                    InputMode::LogTime => log_time(g, ui),
                    InputMode::Help => help(g, ui),
                    _ => {}
                }
            });
        });
    // The close button, Escape or a click outside: cancel, as Esc does in the TUI.
    if closed || response.should_close() {
        close(g);
    }
}

/// The line under the dialog's title.
fn subtitle(g: &GuiApp) -> Option<String> {
    match g.app.input_mode {
        InputMode::Settings => Some(tf(
            "{project} · saved to projects.toml",
            &[("project", &super::shell::project_name(g))],
        )),
        InputMode::ColumnPicker => Some(tl("What the merge request table shows").to_string()),
        InputMode::ProjectPicker => Some(
            tl("Each project opens in its own tab, and keeps refreshing while another one is shown.")
                .to_string(),
        ),
        InputMode::LogTime => g
            .app
            .selected_mr()
            .and_then(|mr| mr.linked_ticket.as_ref())
            .map(|t| format!("#{} — {}", t.id, t.subject)),
        _ => None,
    }
}

/// Closes the dialog without applying it (the columns are saved as they change).
fn close(g: &mut GuiApp) {
    match g.app.input_mode {
        InputMode::Settings => {
            g.app.settings_editor.cancel();
            g.language_draft = g.language.clone();
            g.settings_search.clear();
        }
        InputMode::ColumnPicker => save_columns(g),
        InputMode::LogTime => g.app.log_time_form = LogTimeForm::default(),
        _ => {}
    }
    g.app.input_mode = InputMode::Normal;
}

/// The dialog's header: its icon on an accent tile, the title over `subtitle`,
/// and the close button. Whether the close button was clicked.
pub(super) fn header(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    title: &str,
    subtitle: Option<&str>,
) -> bool {
    let mut close = false;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 20,
            right: 14,
            top: 16,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                let radius = CornerRadius::same(theme.radii.large);
                ui.painter()
                    .rect_filled(tile, radius, theme.palette.accent.gamma_multiply(0.14));
                ui.painter().rect_stroke(
                    tile,
                    radius,
                    Stroke::new(1.0, theme.palette.accent.gamma_multiply(0.3)),
                    StrokeKind::Inside,
                );
                icons::paint(ui.painter(), tile.shrink(10.0), icon, theme.palette.accent);
                let text_width = ui.available_width() - 44.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(text_width, 40.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(text_width);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        // Centered on the tile: one line or two.
                        let lines = if subtitle.is_some() { 38.0 } else { 20.0 };
                        ui.add_space(((40.0 - lines) / 2.0_f32).max(0.0));
                        ui.label(semibold(title, theme.text_size + 4.0));
                        if let Some(subtitle) = subtitle {
                            ui.add(egui::Label::new(muted(theme, subtitle)).wrap());
                        }
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = format!("{} (Esc)", tl("Close"));
                    close = super::widgets::icon_button(
                        ui,
                        &label,
                        Icon::Close,
                        &theme.icon_button(),
                        30.0,
                    )
                    .clicked();
                });
            });
        });
    close
}

/// The dialog's body, with its margins.
pub(super) fn body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(20, 18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// The dialog's footer, on the panel's color under a rule: `add` lays out its
/// line from the left; the buttons go in a right-to-left layout at its end.
pub(super) fn footer(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui)) {
    rule(ui, theme);
    let radius = theme.radii.large.saturating_sub(1);
    egui::Frame::new()
        .fill(theme.palette.panel)
        .corner_radius(CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius,
            se: radius,
        })
        .inner_margin(Margin::symmetric(20, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(theme.spacing.control_height + 2.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                add(ui);
            });
        });
}

/// The buttons at the footer's right end, the first added rightmost.
fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        add(ui);
    });
}

/// A group's caption: small capitals, muted.
fn caption(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(semibold(text.to_uppercase(), theme.text_size - 2.5).color(theme.palette.text_muted));
    ui.add_space(4.0);
}

/// A card of rows: the panel's fill, a border, rounded.
fn card<R>(ui: &mut Ui, theme: &Theme, rows: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme.palette.panel)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            rows(ui)
        })
        .inner
}

/// A small dot in `color`, `size` across, with `tip` on hover.
fn tip_dot(ui: &mut Ui, color: Color32, size: f32, tip: &str) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size + 4.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    response.tip(tip);
}

// ── Settings ─────────────────────────────────────────────────────────────────
