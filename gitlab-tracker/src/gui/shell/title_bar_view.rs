//! The title bar: the update pill, the bell, fetch errors, its buttons.

use super::*;

pub(super) const TITLE_BAR_HEIGHT: f32 = 40.0;

pub(super) fn title_bar(g: &mut GuiApp, ui: &mut Ui, sizes: &Sizes, page: Page) {
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
                    update_pill(g, ui);
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
/// The bell: the unread notifications of every tab counted on it, the panel
/// under it ([`super::super::toasts::panel`]).
/// A newer version is out: a pill with its number, left of the bell. A click
/// opens its release page; nothing is downloaded from the app.
pub(super) fn update_pill(g: &mut GuiApp, ui: &mut Ui) {
    let Some(release) = g.update.clone() else {
        return;
    };
    let theme = g.theme;
    let tone = theme.tones.blue;
    let text = tf("v{version} available", &[("version", &release.version)]);
    let font = fonts::medium(theme.text_size - 1.0);
    let galley = ui.painter().layout_no_wrap(text.clone(), font, tone.fg);
    // An arrow, the text, the room around them.
    let size = Vec2::new(galley.size().x + 12.0 + 10.0 + 14.0, 22.0);
    ui.add_space(4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, ui.is_enabled(), &text));
    let fill = if response.hovered() {
        tone.bg.gamma_multiply(1.8)
    } else {
        tone.bg
    };
    ui.painter().rect_filled(rect, CornerRadius::same(11), fill);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            egui::pos2(rect.left() + 13.0, rect.center().y),
            Vec2::splat(11.0),
        ),
        Icon::ArrowUp,
        tone.fg,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0),
        galley,
        tone.fg,
    );
    ui.add_space(6.0);
    let tip = Tip::new()
        .title(tf(
            "Version {version} is available",
            &[("version", &release.version)],
        ))
        .text(tl("Open the release page"));
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .rich(tip)
        .clicked()
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(&release.url));
    }
}

pub(super) fn bell(g: &mut GuiApp, ui: &mut Ui) {
    let theme = g.theme;
    let response = bar_button(ui, &theme, 44.0, tl("Notifications"), Icon::Bell);
    let unread = g.toasts.unread()
        + g.tabs
            .iter()
            .filter_map(|slot| match &slot.state {
                super::super::SlotState::Parked(t) => Some(t.toasts.unread()),
                _ => None,
            })
            .sum::<usize>();
    if unread > 0 {
        // A disc for one digit, stretched into a pill around "9+".
        let text = if unread > 9 {
            "9+".to_string()
        } else {
            unread.to_string()
        };
        let galley = ui
            .painter()
            .layout_no_wrap(text, fonts::semibold(8.5), theme.palette.chrome);
        let rect = Rect::from_center_size(
            egui::pos2(
                response.rect.center().x + 7.0,
                response.rect.center().y - 7.0,
            ),
            Vec2::new((galley.size().x + 5.0).max(12.0), 12.0),
        );
        ui.painter().rect_filled(
            rect.expand(1.5),
            CornerRadius::same(8),
            theme.palette.chrome,
        );
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), theme.tones.red.fg);
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            theme.palette.chrome,
        );
    }
    super::super::toasts::panel(g, &response);
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
                    retry = super::super::widgets::button(
                        ui,
                        &theme,
                        Some(Icon::Refresh),
                        tl("Retry"),
                        false,
                    )
                    .clicked();
                });
            });
        });
    if retry {
        g.pending.push(Action::Refresh);
    }
}

/// A full-height borderless button in the title bar.
pub(super) fn bar_button(
    ui: &mut Ui,
    theme: &Theme,
    width: f32,
    label: &str,
    icon: Icon,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, TITLE_BAR_HEIGHT - 1.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
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
    response.tip(label)
}
