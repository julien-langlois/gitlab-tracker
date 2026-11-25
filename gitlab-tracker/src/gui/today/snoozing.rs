//! Snoozing a row: its button and panel, and the folded card of snoozed MRs.

use super::*;

/// Width of the snooze panel.
pub(super) const SNOOZE_WIDTH: f32 = 340.0;
/// Height of one choice of the snooze panel.
pub(super) const SNOOZE_ROW: f32 = 54.0;

/// The "Snooze" button of a row and its panel, drawn like the notifications':
/// the MR on top, then each choice with its icon, the day and time it wakes
/// (local time) and how far away that is.
pub(super) fn snooze_menu(ui: &mut Ui, theme: &Theme, id: &str, title: &str) -> Option<Until> {
    let response = super::super::widgets::button(
        ui,
        theme,
        Some(super::super::icons::Icon::Moon),
        tl("Snooze"),
        false,
    )
    .rich(Tip::new().title(tl("Snooze")).text(tl(
        "Out of Today until then; back early on a push, a failed pipeline or a new comment",
    )));
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(egui::Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .shadow(egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(90),
        });
    let mut picked = None;
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_SNOOZE=214` keeps the
    // panel of !214 open, for a screenshot.
    #[cfg(debug_assertions)]
    let forced = std::env::var("GITLAB_TRACKER_GUI_SNOOZE").as_deref() == Ok(id);
    #[cfg(not(debug_assertions))]
    let forced = false;
    egui::Popup::menu(&response)
        .open_memory(if forced {
            Some(egui::SetOpenCommand::Bool(true))
        } else {
            response.clicked().then_some(egui::SetOpenCommand::Toggle)
        })
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(SNOOZE_WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;
            let tone = theme.tones.purple;
            // Header: the moon in its circle, what is snoozed.
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 16,
                    right: 16,
                    top: 14,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let (circle, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                        ui.painter().circle_filled(circle.center(), 16.0, tone.bg);
                        super::super::icons::paint(
                            ui.painter(),
                            egui::Rect::from_center_size(circle.center(), Vec2::splat(16.0)),
                            super::super::icons::Icon::Moon,
                            tone.fg,
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(
                                    RichText::new(tl("Snooze"))
                                        .font(fonts::semibold(theme.text_size + 1.0)),
                                );
                                ui.label(
                                    mono(format!("!{id}"), theme.text_size - 1.0)
                                        .color(theme.palette.text_muted),
                                );
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(title)
                                        .size(theme.text_size - 1.5)
                                        .color(theme.palette.text_secondary),
                                )
                                .truncate(),
                            );
                        });
                    });
                });
            super::super::widgets::rule(ui, theme);

            ui.add_space(6.0);
            let now = chrono::Local::now();
            let tomorrow = Until::Tomorrow.resolve(&now);
            for until in Until::ALL {
                let at = until.resolve(&now);
                // On a Sunday, next Monday is tomorrow morning: offered once.
                if until == Until::NextWeek && at == tomorrow {
                    continue;
                }
                if snooze_choice(ui, theme, until, at, &now) {
                    picked = Some(until);
                }
            }
            ui.add_space(6.0);

            super::super::widgets::rule(ui, theme);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(16, 9))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            muted(
                                theme,
                                tf(
                                    "Back early if it moves · local time ({zone})",
                                    &[("zone", &now.format("UTC%:z").to_string())],
                                ),
                            )
                            .size(theme.text_size - 1.5),
                        );
                    });
                });
            if picked.is_some() {
                ui.close();
            }
        });
    picked
}

/// One choice of the snooze panel; `true` when clicked.
pub(super) fn snooze_choice(
    ui: &mut Ui,
    theme: &Theme,
    until: Until,
    at: Option<chrono::DateTime<chrono::Utc>>,
    now: &chrono::DateTime<chrono::Local>,
) -> bool {
    use super::super::icons::Icon;
    let t = &theme.tones;
    let (icon, tone) = match until {
        Until::Hour => (Icon::Clock, t.blue),
        Until::Tomorrow => (Icon::Sun, t.amber),
        Until::NextWeek => (Icon::Calendar, t.teal),
        Until::Change => (Icon::Pulse, t.purple),
    };
    let (outer, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), SNOOZE_ROW), Sense::click());
    let rect = outer.shrink2(Vec2::new(8.0, 2.0));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(
            rect,
            CornerRadius::same(theme.radii.medium),
            theme.palette.border_soft,
        );
    }
    let center = egui::pos2(rect.left() + 24.0, rect.center().y);
    painter.circle_filled(center, 15.0, tone.bg);
    super::super::icons::paint(
        painter,
        egui::Rect::from_center_size(center, Vec2::splat(15.0)),
        icon,
        tone.fg,
    );
    let left = rect.left() + 50.0;
    let right = rect.right() - 12.0;
    // How far away, on the right: "in 3 h", "in 6 d".
    let away = at.map(|at| {
        let minutes = (at - now.with_timezone(&chrono::Utc)).num_minutes().max(0);
        let hours = (minutes as f64 / 60.0).round() as u64;
        if hours < 24 {
            tn(hours.max(1), "in {n} h", "in {n} h")
        } else {
            tn((hours as f64 / 24.0).round() as u64, "in {n} d", "in {n} d")
        }
    });
    let mut away_width = 0.0;
    if let Some(away) = away {
        let galley = painter.layout_no_wrap(
            away,
            egui::FontId::proportional(theme.text_size - 1.5),
            theme.palette.text_muted,
        );
        away_width = galley.size().x + 10.0;
        painter.galley(
            egui::pos2(
                right - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            theme.palette.text_muted,
        );
    }
    // What, then when: "Wednesday 18 November · 09:00".
    let when = match at {
        Some(at) => {
            let local = at.with_timezone(&chrono::Local);
            format!(
                "{} · {}",
                super::super::journal::day_label(local.date_naive(), now.date_naive()),
                local.format("%H:%M")
            )
        }
        None => tl("A push, a failed CI or a comment").to_string(),
    };
    let width = right - left - away_width;
    let title = painter.layout(
        until.label().to_string(),
        fonts::semibold(theme.text_size),
        theme.palette.text,
        width,
    );
    let mut sub = egui::text::LayoutJob::simple_singleline(
        when,
        egui::FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
    );
    sub.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let sub = painter.layout_job(sub);
    let top = rect.center().y - (title.size().y + 3.0 + sub.size().y) / 2.0;
    painter.galley(egui::pos2(left, top), title.clone(), theme.palette.text);
    painter.galley(
        egui::pos2(left, top + title.size().y + 3.0),
        sub,
        theme.palette.text_muted,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// The snoozed MRs, folded by default: each with when it wakes and a button
/// to wake it now.
pub(super) fn snoozed_card<'a>(
    ui: &mut Ui,
    theme: &Theme,
    items: &[&'a Item],
    show_project: bool,
) -> Option<(&'a Item, RowClick)> {
    let id = egui::Id::unique("today-snoozed-open");
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let mut click = None;
    theme.card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), theme.text_size + 6.0),
            Sense::click(),
        );
        let tone = theme.tones.purple;
        let mut header = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        header.style_mut().interaction.selectable_labels = false;
        super::super::icons::paint(
            header.painter(),
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 7.0, rect.center().y),
                Vec2::splat(13.0),
            ),
            super::super::icons::Icon::Moon,
            tone.fg,
        );
        header.add_space(20.0);
        header.label(
            RichText::new(format!("{} · {}", tl("Snoozed"), items.len()))
                .font(fonts::semibold(theme.text_size))
                .color(tone.fg),
        );
        header.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(muted(theme, if open { "▾" } else { "▸" }));
        });
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .tip(tl(if open { "Fold" } else { "Unfold" }))
            .clicked()
        {
            ui.data_mut(|d| d.insert_temp(id, !open));
        }
        if !open {
            return;
        }
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for item in items {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(theme.radii.medium),
                    theme.palette.raised,
                );
            }
            let mut line = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(Vec2::new(12.0, 0.0)))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            line.style_mut().interaction.selectable_labels = false;
            line.spacing_mut().item_spacing.x = 10.0;
            if show_project {
                line.allocate_ui_with_layout(
                    Vec2::new(PROJECT_WIDTH, 20.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_width(PROJECT_WIDTH);
                        ui.add(
                            Chip::new(&item.project, theme.tones.gray)
                                .font(fonts::medium(theme.text_size - 2.5)),
                        );
                    },
                );
            }
            line.allocate_ui_with_layout(
                Vec2::new(ID_WIDTH, 20.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(ID_WIDTH);
                    ui.label(
                        mono(format!("!{}", item.id), theme.text_size - 1.0)
                            .color(theme.palette.text_secondary),
                    );
                },
            );
            line.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if super::super::widgets::button(ui, theme, None, tl("Wake now"), false).clicked() {
                    click = Some((*item, RowClick::Wake));
                }
                if let Some(until) = &item.snoozed {
                    ui.label(
                        RichText::new(until)
                            .size(theme.text_size - 1.5)
                            .color(tone.fg),
                    );
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&item.title).color(theme.palette.text_secondary),
                        )
                        .truncate(),
                    );
                });
            });
            if click.is_none()
                && response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .rich(
                        Tip::new()
                            .mr(&item.id, &item.title)
                            .hint(tl("Open its page")),
                    )
                    .clicked()
            {
                click = Some((*item, RowClick::Open));
            }
        }
    });
    click
}
