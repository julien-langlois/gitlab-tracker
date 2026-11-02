//! The window's frame: title bar with the project tabs, icon rail, status bar, and
//! the responsive split between the side panels and the page.

use bevel_theme::{fonts, Theme};
use egui::{Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use super::widgets::{dot, icon_toggle, muted, spinner};
use super::{cockpit, dialogs, inspector, sidebar, table, tracker, GuiApp, Page};
use crate::actions::Action;
use crate::app::InputMode;
use crate::i18n::{tf, tl, tn};

pub fn draw(g: &mut GuiApp, ui: &mut Ui) {
    let width = ui.max_rect().width();
    let page = g.page();
    let mut sizes = Sizes::for_width(width, g.show_views, g.show_inspector);
    g.views_shown = page == Page::MergeRequests && sizes.views.is_some();
    if page == Page::Tracker {
        sizes.widen_inspector(width);
    } else {
        g.ticket_anchor = None;
    }
    title_bar(g, ui, &sizes, page);
    status_bar(g, ui);
    rail(g, ui, page);

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
        Page::Today => {
            egui::CentralPanel::default()
                .frame(content.inner_margin(Margin::symmetric(24, 20)))
                .show(ui, |ui| super::today::page(g, ui));
        }
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

const TITLE_BAR_HEIGHT: f32 = 40.0;

fn title_bar(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes, page: Page) {
    let theme = g.theme;
    egui::Panel::top("title_bar")
        .exact_size(TITLE_BAR_HEIGHT)
        .frame(egui::Frame::new().fill(theme.palette.chrome))
        .show(ui, |ui| {
            let bar = ui.max_rect();
            ui.painter().hline(
                bar.x_range(),
                bar.bottom() - 0.5,
                Stroke::new(1.0, theme.palette.border),
            );
            // Before the controls, so they sit on top and take their own clicks.
            bevel_window::title_bar::empty_area(ui, bar);
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal_centered(|ui| {
                if let Some(layout) = g.frame.layout() {
                    window_buttons(ui, &theme, &layout.left);
                }
                if bar_button(ui, &theme, 52.0, tl("Merge requests"), Icon::Home).clicked() {
                    g.open_page(Page::MergeRequests);
                }
                project_tabs(g, ui, sizes);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(layout) = g.frame.layout() {
                        let right: Vec<_> = layout.right.iter().rev().copied().collect();
                        window_buttons(ui, &theme, &right);
                    }
                    let (icon, label, next) = match theme.mode {
                        bevel_theme::Mode::Dark => {
                            (Icon::Sun, tl("Light theme"), bevel_theme::Mode::Light)
                        }
                        bevel_theme::Mode::Light => {
                            (Icon::Moon, tl("Dark theme"), bevel_theme::Mode::Dark)
                        }
                    };
                    if bar_button(ui, &theme, 44.0, label, icon).clicked() {
                        g.set_theme(next);
                    }
                    bell(g, ui);
                    if matches!(page, Page::MergeRequests | Page::Tracker) {
                        let shown = sizes.inspector.is_some();
                        let label = tl(if shown {
                            "Hide the inspector"
                        } else {
                            "Show the inspector"
                        });
                        let style = theme.icon_button();
                        if icon_toggle(ui, label, Icon::PanelRight, &style, 30.0, shown).clicked() {
                            g.show_inspector = Some(!shown);
                        }
                        ui.add_space(6.0);
                    }
                    if !sizes.compact {
                        ui.add_space(8.0);
                        sync_state(g, ui);
                        ui.add_space(8.0);
                    }
                });
            });
        });
}

/// The bell: the session's toasts, the unread ones counted on it.
fn bell(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let response = bar_button(ui, &theme, 44.0, tl("Notifications"), Icon::Bell);
    let unread = g.toasts.unread;
    if unread > 0 {
        let center = egui::pos2(
            response.rect.center().x + 7.0,
            response.rect.center().y - 7.0,
        );
        ui.painter()
            .circle_filled(center, 7.5, theme.palette.chrome);
        ui.painter().circle_filled(center, 6.0, theme.tones.red.fg);
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            if unread > 9 {
                "9+".to_string()
            } else {
                unread.to_string()
            },
            fonts::semibold(8.5),
            theme.palette.chrome,
        );
    }
    let mut picked = None;
    let mut clear = false;
    egui::Popup::menu(&response)
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            g.toasts.unread = 0;
            ui.set_width(320.0);
            ui.horizontal(|ui| {
                ui.label(fonts_label(&theme, tl("Notifications")));
                if !g.toasts.history.is_empty() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button(tl("Clear")).clicked() {
                            clear = true;
                        }
                    });
                }
            });
            ui.separator();
            if g.toasts.history.is_empty() {
                ui.label(muted(
                    &theme,
                    tl("Nothing yet: merges, status changes and pipelines show up here."),
                ));
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for past in g.toasts.history.iter().rev() {
                        let response = ui
                            .horizontal(|ui| {
                                dot(ui, past.tone.fg, 7.0);
                                ui.label(muted(&theme, &past.at));
                                ui.label(&past.text);
                            })
                            .response
                            .interact(Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            picked = Some(past.id.clone());
                        }
                    }
                });
        });
    if clear {
        g.toasts.history.clear();
    }
    if let Some(id) = picked {
        g.show_mr(&id);
    }
}

fn fonts_label(theme: &Theme, text: &str) -> RichText {
    RichText::new(text).font(fonts::semibold(theme.text_size))
}

/// A banner over the table when MRs could not be refreshed: how many, the first
/// error, and a retry.
pub fn fetch_errors(g: &mut GuiApp, ui: &mut Ui) {
    let failed: Vec<&str> = g
        .app
        .mrs
        .iter()
        .filter_map(|mr| match &mr.status {
            crate::models::MrStatus::Error(message) => Some(message.as_str()),
            _ => None,
        })
        .collect();
    let Some(first) = failed.first() else {
        return;
    };
    let theme = g.theme;
    let tone = theme.tones.red;
    let mut retry = false;
    egui::Frame::new()
        .fill(tone.bg)
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(12, 8))
        .outer_margin(Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new("⚠").color(tone.fg));
                let text = tn(
                    failed.len() as u64,
                    "{n} MR could not be refreshed",
                    "{n} MRs could not be refreshed",
                );
                ui.label(
                    RichText::new(text)
                        .font(fonts::semibold(theme.text_size))
                        .color(tone.fg),
                );
                ui.add(egui::Label::new(muted(&theme, *first)).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    retry =
                        super::widgets::button(ui, &theme, Some(Icon::Refresh), tl("Retry"), false)
                            .clicked();
                });
            });
        });
    if retry {
        g.pending.push(Action::Refresh);
    }
}

/// A full-height borderless button in the title bar.
fn bar_button(ui: &mut Ui, theme: &Theme, width: f32, label: &str, icon: Icon) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, TITLE_BAR_HEIGHT - 1.0), Sense::click());
    let color = if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, theme.palette.raised);
        theme.palette.text
    } else {
        theme.palette.text_secondary
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(15.0)),
        icon,
        color,
    );
    response.on_hover_text(label)
}

/// What a tab shows, read before drawing.
struct TabView {
    name: String,
    tooltip: String,
    active: bool,
    loading: bool,
    /// Changes noted while hidden (the bell's unread), and failed fetches.
    unread: usize,
    errors: bool,
}

/// The open projects as tabs: the shown one on the content color, merged into
/// the page below. A click shows a tab, a middle click or its `×` closes it (the
/// last one stays); `+` opens the project picker, to open another or add one.
fn project_tabs(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let theme = g.theme;
    let views: Vec<TabView> = g
        .tabs
        .iter()
        .map(|slot| match &slot.state {
            super::SlotState::Active => TabView {
                name: project_name(g),
                tooltip: tf(
                    "{url} · project {id}",
                    &[("url", &slot.url), ("id", &slot.id)],
                ),
                active: true,
                loading: false,
                unread: 0,
                errors: false,
            },
            super::SlotState::Parked(tab) => TabView {
                name: tab
                    .app
                    .project_name
                    .clone()
                    .unwrap_or_else(|| tab.app.project_id.clone()),
                tooltip: tf(
                    "{url} · project {id}",
                    &[("url", &slot.url), ("id", &slot.id)],
                ),
                active: false,
                loading: !tab.app.pending_initial_fetches.is_empty(),
                unread: tab.toasts.unread,
                errors: tab.has_errors(),
            },
            super::SlotState::Loading { .. } | super::SlotState::Closed => TabView {
                name: slot.name.clone(),
                tooltip: tl("Loading…").to_string(),
                active: false,
                loading: true,
                unread: 0,
                errors: false,
            },
        })
        .collect();
    let closable = g.tabs.len() > 1;
    // Room for the tabs: the title bar less its buttons, shared evenly.
    let room = (ui.available_width() - if sizes.compact { 220.0 } else { 520.0 }).max(160.0);
    let max_width = (room / views.len() as f32).clamp(110.0, 240.0);
    let mut show = None;
    let mut close = None;
    for (index, view) in views.iter().enumerate() {
        let font = if view.active {
            fonts::medium(theme.text_size)
        } else {
            egui::FontId::proportional(theme.text_size)
        };
        let text_room = max_width - 66.0;
        let mut job =
            egui::text::LayoutJob::simple_singleline(view.name.clone(), font, theme.palette.text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_room);
        let galley = ui.painter().layout_job(job);
        let min_width = if view.active && !sizes.compact {
            160.0
        } else {
            0.0
        };
        let width = (galley.size().x + 66.0).clamp(min_width, max_width.max(min_width));
        let height = if view.active {
            TITLE_BAR_HEIGHT
        } else {
            TITLE_BAR_HEIGHT - 1.0
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
        let hovered = response.hovered();
        let painter = ui.painter();
        let border = Stroke::new(1.0, theme.palette.border);
        let color = if view.active {
            painter.rect_filled(rect, 0.0, theme.palette.content);
            painter.vline(rect.left() + 0.5, rect.y_range(), border);
            painter.vline(rect.right() - 0.5, rect.y_range(), border);
            theme.palette.text
        } else if hovered {
            painter.rect_filled(rect, 0.0, theme.palette.raised);
            theme.palette.text
        } else {
            theme.palette.text_secondary
        };
        let icon_color = if view.active {
            theme.palette.accent
        } else {
            color
        };
        tab_label(ui, rect, galley, Icon::MergeRequests, icon_color, color);
        if view.loading {
            let at = Rect::from_center_size(
                egui::pos2(rect.left() + 21.0, rect.center().y),
                Vec2::splat(16.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                spinner(ui, 14.0, theme.palette.accent);
            });
        }
        // Hidden tabs: a dot for what changed (red when a fetch failed).
        if !view.active && (view.unread > 0 || view.errors) {
            let tone = if view.errors {
                theme.tones.red.fg
            } else {
                theme.palette.accent
            };
            ui.painter().circle_filled(
                egui::pos2(rect.left() + 28.0, rect.center().y - 7.0),
                3.5,
                tone,
            );
        }
        // The close button: on hover, or always on the shown tab.
        let close_rect = Rect::from_center_size(
            egui::pos2(rect.right() - 16.0, rect.center().y),
            Vec2::splat(18.0),
        );
        let mut close_clicked = false;
        if closable && (hovered || view.active || ui.rect_contains_pointer(close_rect)) {
            let close_response = ui
                .interact(
                    close_rect,
                    ui.scope_id().with(("close_tab", index)),
                    Sense::click(),
                )
                .on_hover_text(tl("Close the tab"));
            if close_response.hovered() {
                ui.painter()
                    .rect_filled(close_rect, CornerRadius::same(4), theme.palette.border);
            }
            icons::paint(
                ui.painter(),
                Rect::from_center_size(close_rect.center(), Vec2::splat(10.0)),
                Icon::Close,
                if close_response.hovered() {
                    theme.palette.text
                } else {
                    theme.palette.text_secondary
                },
            );
            close_clicked = close_response.clicked();
        }
        let tooltip = if view.unread > 0 {
            format!(
                "{}\n{}",
                view.tooltip,
                tn(
                    view.unread as u64,
                    "{n} change since shown",
                    "{n} changes since shown"
                )
            )
        } else {
            view.tooltip.clone()
        };
        let response = response.on_hover_text(tooltip);
        if close_clicked || (closable && response.middle_clicked()) {
            close = Some(index);
        } else if response.clicked() && !view.active {
            show = Some(index);
        }
    }
    if !g.app.read_only
        && bar_button(
            ui,
            &theme,
            36.0,
            tl("Open or add a project (W)"),
            Icon::Plus,
        )
        .clicked()
    {
        g.pending.push(Action::OpenProjectPicker);
    }
    if let Some(index) = close {
        g.close_tab(index);
    } else if let Some(index) = show {
        g.activate(index);
    }
}

/// A tab's icon and name, from the tab's left edge.
fn tab_label(
    ui: &Ui,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    icon: Icon,
    icon_color: Color32,
    text_color: Color32,
) {
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 21.0, rect.center().y),
        Vec2::splat(14.0),
    );
    icons::paint(ui.painter(), icon_rect, icon, icon_color);
    let pos = egui::pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, text_color);
}

pub fn project_name(g: &GuiApp) -> String {
    g.app
        .project_name
        .clone()
        .unwrap_or_else(|| g.app.project_id.clone())
}

fn window_buttons(ui: &mut Ui, theme: &Theme, buttons: &[bevel_window::Button]) {
    for button in buttons {
        let (label, icon) = match button {
            bevel_window::Button::Minimize => (tl("Minimize"), Icon::Minimize),
            bevel_window::Button::Maximize => (tl("Maximize"), Icon::Maximize),
            bevel_window::Button::Close => (tl("Close"), Icon::Close),
        };
        if bar_button(ui, theme, 46.0, label, icon).clicked() {
            let command = match button {
                bevel_window::Button::Minimize => egui::ViewportCommand::Minimized(true),
                bevel_window::Button::Maximize => {
                    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                    egui::ViewportCommand::Maximized(!maximized)
                }
                bevel_window::Button::Close => egui::ViewportCommand::Close,
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

/// "Syncing…" while fetches run, else the countdown to the next refresh.
/// Laid out right to left.
fn sync_state(g: &GuiApp, ui: &mut Ui) {
    let theme = &g.theme;
    ui.spacing_mut().item_spacing.x = 6.0;
    let pending = g.app.pending_initial_fetches.len() + g.app.pending_refresh_fetches.len();
    if pending > 0 {
        ui.label(muted(
            theme,
            tf("Syncing · {n} left", &[("n", &pending.to_string())]),
        ));
        spinner(ui, 12.0, theme.palette.accent);
    } else {
        let secs = g.app.time_left;
        // The TUI's countdown colors: green, amber under a minute, red under 30 s.
        let color = if secs < 30 {
            theme.tones.red.fg
        } else if secs < 60 {
            theme.tones.amber.fg
        } else {
            theme.tones.green.fg
        };
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(
            RichText::new(format!("{:02}:{:02}", secs / 60, secs % 60))
                .font(fonts::semibold(theme.text_size - 1.0))
                .color(color),
        );
        ui.label(muted(theme, tl("Next refresh in")));
        ui.add_space(2.0);
        dot(ui, color, 7.0);
        if let Some(at) = g.last_sync {
            let minutes = (chrono::Local::now() - at).num_minutes();
            let ago = if minutes < 1 {
                tl("Updated just now").to_string()
            } else {
                tn(minutes as u64, "Updated {n} min ago", "Updated {n} min ago")
            };
            ui.add_space(6.0);
            ui.label(muted(theme, format!("{ago} ·")))
                .on_hover_text(at.format("%H:%M:%S").to_string());
        }
    }
}

// ── Status bar ───────────────────────────────────────────────────────────────

fn status_bar(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    egui::Panel::bottom("status_bar")
        .exact_size(28.0)
        .frame(
            egui::Frame::new()
                .fill(theme.palette.chrome)
                .inner_margin(Margin::symmetric(14, 0)),
        )
        .show(ui, |ui| {
            let bar = ui.max_rect().expand2(Vec2::new(14.0, 0.0));
            ui.painter().hline(
                bar.x_range(),
                bar.top() + 0.5,
                Stroke::new(1.0, theme.palette.border),
            );
            ui.horizontal_centered(|ui| {
                if let Some(flash) = &g.app.flash {
                    let color = if flash.is_error {
                        theme.tones.red.fg
                    } else {
                        theme.palette.text_secondary
                    };
                    let text = egui::RichText::new(&flash.text)
                        .size(theme.text_size - 1.0)
                        .color(color);
                    ui.add(egui::Label::new(text).truncate());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    ui.label(muted(&theme, tl("? shortcuts")));
                    let filter = g.app.active_filter.label(&g.app.filter_defs);
                    let tracked = tf(
                        "{n} tracked · View: {view}",
                        &[("n", &g.app.mrs.len().to_string()), ("view", &filter)],
                    );
                    let text = if g.app.config.hide_merged {
                        format!("{tracked} · {}", tl("merged hidden"))
                    } else {
                        tracked
                    };
                    ui.label(muted(&theme, text));
                    status_separator(ui, &theme);
                    auto_polling(ui, &theme, g.app.discovery_enabled);
                    if let Some(tracker) = &g.app.tracker {
                        request_estimate(
                            ui,
                            &theme,
                            tracker.name(),
                            g.app.estimated_tracker_calls,
                            g.app.startup_tracker_estimate,
                        );
                    }
                    request_estimate(
                        ui,
                        &theme,
                        "GitLab",
                        g.app.estimated_gitlab_calls,
                        g.app.startup_gitlab_estimate,
                    );
                });
            });
        });
}

/// A thin vertical line between groups of the status bar.
fn status_separator(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 14.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.palette.border);
}

/// "Auto-polling ON" in green, or "OFF" dimmed, as in the TUI's status bar.
/// Laid out right to left.
fn auto_polling(ui: &mut Ui, theme: &Theme, on: bool) {
    let hint = tl(if on {
        "Each refresh also looks for the team's new MRs (discover_new_mrs)"
    } else {
        "Only the tracked MRs are refreshed; set discover_new_mrs to find the team's new ones"
    });
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (text, tone) = if on {
                (tl("ON"), theme.tones.green)
            } else {
                (tl("OFF"), theme.tones.gray)
            };
            ui.add(
                bevel_theme::Chip::new(text, tone)
                    .strong()
                    .font(fonts::semibold(theme.text_size - 2.5)),
            );
            ui.label(muted(theme, tl("Auto-polling")).size(theme.text_size - 1.0));
        })
        .response;
    response.on_hover_text(hint);
}

/// "GitLab ~12" — the requests the next refresh should send, the count in the
/// accent — with the estimate at startup when it differs, as in the TUI. Laid
/// out right to left.
fn request_estimate(ui: &mut Ui, theme: &Theme, name: &str, next: usize, startup: Option<usize>) {
    let size = theme.text_size - 1.0;
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(startup) = startup.filter(|s| *s != next) {
                let text = tf("({n} at startup)", &[("n", &startup.to_string())]);
                ui.label(muted(theme, text).size(size - 0.5));
            }
            ui.label(muted(theme, tl("req.")).size(size));
            ui.label(
                RichText::new(format!("~{next}"))
                    .font(fonts::semibold(size))
                    .color(theme.tones.teal.fg),
            );
            ui.label(muted(theme, format!("{name}:")).size(size));
        })
        .response;
    let mut hint = tf(
        "{name}: about {n} API requests at the next refresh",
        &[("name", name), ("n", &next.to_string())],
    );
    if let Some(s) = startup {
        hint.push('\n');
        hint.push_str(&tf(
            "{n} at startup, from the saved state",
            &[("n", &s.to_string())],
        ));
    }
    response.on_hover_text(hint);
}

// ── Icon rail ────────────────────────────────────────────────────────────────

fn rail(g: &mut GuiApp, ui: &mut Ui, page: Page) {
    let theme = g.theme;
    egui::Panel::left("rail")
        .exact_size(RAIL_WIDTH)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(theme.palette.rail)
                .inner_margin(Margin::symmetric(6, 10)),
        )
        .show(ui, |ui| {
            let edge = ui.max_rect().expand2(Vec2::new(6.0, 10.0));
            ui.painter().vline(
                edge.right() - 0.5,
                edge.y_range(),
                Stroke::new(1.0, theme.palette.border),
            );
            ui.spacing_mut().item_spacing.y = 6.0;
            let count = |key: &str| g.members.get(key).map_or(0, Vec::len);
            let tones = &theme.tones;
            // What waits on each page: my reviews, the blocked MRs, the tickets
            // over their estimate.
            let today = super::today::waiting_count(g);
            let badges = [
                (Page::Today, today, tones.amber, tl("waiting for you")),
                (
                    Page::MergeRequests,
                    count("To review (me)"),
                    tones.amber,
                    tl("waiting for your review"),
                ),
                (Page::Cockpit, count("Blocked"), tones.red, tl("blocked")),
                (
                    Page::Tracker,
                    count("Over estimate"),
                    tones.red,
                    tl("over their estimate"),
                ),
            ];
            ui.vertical_centered(|ui| {
                let mut items = vec![
                    (
                        Page::Today,
                        Icon::Clock,
                        tl("Today"),
                        tl("What waits for you, in every tab"),
                    ),
                    (
                        Page::MergeRequests,
                        Icon::MergeRequests,
                        tl("MRs"),
                        tl("Merge requests"),
                    ),
                    (Page::Cockpit, Icon::Cockpit, tl("Cockpit"), tl("Cockpit")),
                ];
                if g.app.branches.len() >= 2 {
                    items.push((
                        Page::Promote,
                        Icon::ArrowUp,
                        tl("Promote"),
                        tl("What is ready to go to the next branch"),
                    ));
                }
                if g.app.tracker.is_some() {
                    items.push((
                        Page::Tracker,
                        Icon::Ticket,
                        tl("Tracker"),
                        tl("Linked tickets"),
                    ));
                }
                #[cfg(feature = "stats")]
                items.push((Page::Stats, Icon::Stats, tl("Stats"), tl("Statistics (G)")));
                for (target, icon, label, hint) in items {
                    let badge = badges
                        .iter()
                        .find(|b| b.0 == target && b.1 > 0)
                        .map(|b| (b.1, b.2));
                    let hint = match badges.iter().find(|b| b.0 == target && b.1 > 0) {
                        Some((_, n, _, what)) => format!("{hint} · {n} {what}"),
                        None => hint.to_string(),
                    };
                    let response = rail_item(ui, &theme, icon, Some(label), page == target);
                    if let Some((n, tone)) = badge {
                        rail_badge(ui, &theme, response.rect, n, tone);
                    }
                    if response.on_hover_text(hint).clicked() {
                        g.open_page(target);
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    if rail_item(ui, &theme, Icon::Settings, None, false)
                        .on_hover_text(tl("Settings (,)"))
                        .clicked()
                    {
                        g.pending.push(Action::OpenSettings);
                    }
                    if rail_item(ui, &theme, Icon::Help, None, false)
                        .on_hover_text(tl("Keyboard shortcuts (?)"))
                        .clicked()
                    {
                        g.app.input_mode = InputMode::Help;
                    }
                });
            });
        });
}

/// A count on a rail entry's top-right corner, in `tone`.
fn rail_badge(ui: &Ui, theme: &Theme, item: Rect, n: usize, tone: bevel_theme::Tone) {
    let text = if n > 99 {
        "99+".to_string()
    } else {
        n.to_string()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, fonts::semibold(9.5), Color32::WHITE);
    let width = (galley.size().x + 8.0).max(16.0);
    let rect = Rect::from_center_size(
        egui::pos2(item.right() - 6.0, item.top() + 6.0),
        Vec2::new(width, 16.0),
    );
    ui.painter()
        .rect_filled(rect.expand(1.5), CornerRadius::same(10), theme.palette.rail);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), tone.fg);
    let text_color = if theme.mode == bevel_theme::Mode::Dark {
        theme.palette.rail
    } else {
        Color32::WHITE
    };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, text_color);
}

/// A rail entry: an icon over a short caption, highlighted when `active`.
fn rail_item(
    ui: &mut Ui,
    theme: &Theme,
    icon: Icon,
    label: Option<&str>,
    active: bool,
) -> egui::Response {
    let height = if label.is_some() { 46.0 } else { 34.0 };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(40.0, height), Sense::click());
    let p = &theme.palette;
    let (fill, color) = if active {
        (p.rail_active, p.accent)
    } else if response.hovered() {
        (p.raised, p.text)
    } else {
        (Color32::TRANSPARENT, p.text_secondary)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    let icon_center = match label {
        Some(_) => egui::pos2(rect.center().x, rect.top() + 16.0),
        None => rect.center(),
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(icon_center, Vec2::splat(18.0)),
        icon,
        color,
    );
    if let Some(label) = label {
        let font = if active {
            fonts::medium(10.0)
        } else {
            egui::FontId::proportional(10.0)
        };
        ui.painter().text(
            egui::pos2(rect.center().x, rect.bottom() - 10.0),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            color,
        );
    }
    super::widgets::focus_ring(ui, theme, &response, 8);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_panels_follow_the_window_width() {
        // A small laptop window: only the table.
        let small = Sizes::for_width(1024.0, None, None);
        assert_eq!((small.views, small.inspector), (None, None));
        assert!(small.compact);
        let close = |a: Option<f32>, b: f32| a.is_some_and(|a| (a - b).abs() < 0.01);
        // The mockup's 1440 points: both panels, near their design widths.
        let laptop = Sizes::for_width(1440.0, None, None);
        assert!(close(laptop.views, 230.4), "{laptop:?}");
        assert!(close(laptop.inspector, 316.8), "{laptop:?}");
        // 1080p, or 4K at 200 %: wider panels.
        let hd = Sizes::for_width(1920.0, None, None);
        assert!(close(hd.views, 280.0), "{hd:?}");
        assert!(close(hd.inspector, 422.4), "{hd:?}");
        // Ultrawide 4K at 100 %: the panels stop growing, the table takes the room.
        let ultrawide = Sizes::for_width(5120.0, None, None);
        assert_eq!(
            (ultrawide.views, ultrawide.inspector),
            (Some(280.0), Some(460.0))
        );
    }

    #[test]
    fn the_tracker_page_widens_the_inspector() {
        let widened = |width: f32| {
            let mut sizes = Sizes::for_width(width, None, Some(true));
            sizes.widen_inspector(width);
            sizes.inspector.unwrap()
        };
        // 1080p: much wider, the list keeps over 1200 points.
        assert!((widened(1920.0) - 576.0).abs() < 0.01);
        // A laptop: wider, the list keeps its minimum.
        assert!(1440.0 - RAIL_WIDTH - widened(1440.0) >= TRACKER_LIST_MIN_WIDTH);
        assert!(widened(1440.0) > 400.0);
        // A small window: a little wider, the list still keeps its minimum.
        assert!((widened(1024.0) - 332.0).abs() < 0.01);
        // Too narrow to widen: the usual inspector.
        assert!((widened(900.0) - 300.0).abs() < 0.01);
        assert_eq!(widened(5120.0), 580.0);
    }

    #[test]
    fn toggles_override_the_width() {
        let forced = Sizes::for_width(1024.0, Some(true), Some(true));
        assert_eq!((forced.views, forced.inspector), (Some(220.0), Some(300.0)));
        let hidden = Sizes::for_width(2560.0, Some(false), Some(false));
        assert_eq!((hidden.views, hidden.inspector), (None, None));
    }
}
