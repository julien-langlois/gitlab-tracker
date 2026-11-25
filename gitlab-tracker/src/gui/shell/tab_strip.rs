//! The open projects as tabs, and the window's buttons.

use super::*;

/// What a tab shows, read before drawing.
pub(super) struct TabView {
    pub(super) name: String,
    pub(super) tooltip: String,
    pub(super) active: bool,
    pub(super) loading: bool,
    /// Changes noted while hidden (the bell's unread), and failed fetches.
    pub(super) unread: usize,
    pub(super) errors: bool,
}

/// The open projects as tabs: the shown one on the content color, merged into
/// the page below. A click shows a tab, a middle click or its `×` closes it (the
/// last one stays); `+` opens the project picker, to open another or add one.
pub(super) fn project_tabs(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes) {
    let theme = g.theme;
    let views: Vec<TabView> = g
        .tabs
        .iter()
        .map(|slot| match &slot.state {
            super::super::SlotState::Active => TabView {
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
            super::super::SlotState::Parked(tab) => TabView {
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
                unread: tab.toasts.unread(),
                errors: tab.has_errors(),
            },
            super::super::SlotState::Loading { .. } | super::super::SlotState::Closed => TabView {
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
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), view.active, &view.name)
        });
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
            let close_response = ui.interact(
                close_rect,
                ui.scope_id().with(("close_tab", index)),
                Sense::click(),
            );
            close_response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, tl("Close the tab"))
            });
            let close_response = close_response.tip(tl("Close the tab"));
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
        let mut tooltip = Tip::new().title(&view.name).text(&view.tooltip);
        if view.unread > 0 {
            tooltip = tooltip.dot(
                theme.palette.accent,
                tn(
                    view.unread as u64,
                    "{n} change since shown",
                    "{n} changes since shown",
                ),
            );
        }
        let response = response.rich(tooltip);
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
pub(super) fn tab_label(
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

pub(super) fn window_buttons(ui: &mut Ui, theme: &Theme, buttons: &[bevel_window::Button]) {
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
