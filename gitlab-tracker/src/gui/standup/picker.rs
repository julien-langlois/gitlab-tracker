//! Today's "Copy my stand-up | ▾" button, its panel, and the preview as the chat will show it.

use super::*;

pub(super) const PANEL_WIDTH: f32 = 780.0;
pub(super) const LEFT_WIDTH: f32 = 330.0;

/// Today's "Copy my stand-up | ▾", in a right-to-left row: a click copies, the
/// ▾ opens the panel.
pub fn button(g: &mut GuiApp, ui: &mut Ui, items: &[Item]) {
    let theme = g.theme;
    let p = &theme.palette;
    let label = tl("Copy my stand-up");
    let font = fonts::medium(theme.text_size - 0.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, p.text_secondary);
    let chevron_w = 30.0;
    let main_w = 10.0 + 20.0 + galley.size().x + 12.0;
    let height = theme.spacing.control_height + 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(main_w + chevron_w, height), Sense::hover());
    let main_rect = Rect::from_min_size(rect.min, Vec2::new(main_w, height));
    let chevron_rect = Rect::from_min_max(egui::pos2(main_rect.right(), rect.top()), rect.max);
    let main = ui
        .interact(
            main_rect,
            ui.scope_id().with("standup-copy"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let chevron = ui
        .interact(
            chevron_rect,
            ui.scope_id().with("standup-menu"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&chevron));
    if ui.is_rect_visible(rect) {
        let r = theme.radii.medium;
        let painter = ui.painter();
        let half = |rect: Rect, left: bool, on: bool| {
            if on {
                let radius = if left {
                    CornerRadius {
                        nw: r,
                        sw: r,
                        ne: 0,
                        se: 0,
                    }
                } else {
                    CornerRadius {
                        nw: 0,
                        sw: 0,
                        ne: r,
                        se: r,
                    }
                };
                painter.rect_filled(rect, radius, p.raised);
            }
        };
        half(main_rect, true, main.hovered());
        half(chevron_rect, false, chevron.hovered() || open);
        painter.rect_stroke(
            rect,
            CornerRadius::same(r),
            Stroke::new(1.0, if open { p.accent } else { p.border }),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [
                egui::pos2(main_rect.right(), rect.top() + 6.0),
                egui::pos2(main_rect.right(), rect.bottom() - 6.0),
            ],
            Stroke::new(1.0, p.border),
        );
        let tint = |hovered: bool| if hovered { p.text } else { p.text_secondary };
        let icon_rect = Rect::from_center_size(
            egui::pos2(main_rect.left() + 17.0, rect.center().y),
            Vec2::splat(14.0),
        );
        icons::paint(painter, icon_rect, Icon::Copy, tint(main.hovered()));
        painter.galley(
            egui::pos2(
                main_rect.left() + 30.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            tint(main.hovered()),
        );
        let chevron_icon = Rect::from_center_size(chevron_rect.center(), Vec2::splat(12.0));
        icons::paint(
            painter,
            chevron_icon,
            if open { Icon::ArrowUp } else { Icon::ArrowDown },
            if open {
                p.accent
            } else {
                tint(chevron.hovered())
            },
        );
    }
    super::super::widgets::focus_ring(ui, &theme, &main, theme.radii.medium);
    let main = main.tip(tl(
        "What you did since the last working day, what is on today, and what waits on others, as text for the daily meeting",
    ));
    Tip::new()
        .text(tl("Pick the period, the sections and the format"))
        .show(chevron.clone());

    let mut settings = g.standup;
    let mut copy = main.clicked();
    let frame = egui::Frame::new()
        .fill(p.raised)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .shadow(egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(90),
        });
    // Development aid (debug builds): `GITLAB_TRACKER_GUI_PAGE=standup` keeps it open.
    #[cfg(debug_assertions)]
    let forced = std::env::var("GITLAB_TRACKER_GUI_PAGE").as_deref() == Ok("standup");
    #[cfg(not(debug_assertions))]
    let forced = false;
    let now = chrono::Local::now();
    egui::Popup::menu(&chevron)
        .open_memory(if forced {
            Some(egui::SetOpenCommand::Bool(true))
        } else {
            chevron.clicked().then_some(egui::SetOpenCommand::Toggle)
        })
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let standup = build(g, items, &settings, now);
            if panel(ui, &theme, &standup, &mut settings, now) {
                copy = true;
                ui.close();
            }
        });
    if copy {
        ui.ctx().copy_text(text(g, items, &settings, now));
        settings.last_copied = Some(now.timestamp());
        g.app
            .flash(tl("Stand-up copied: paste it in the team's chat"), false);
    }
    g.standup = settings;
}

/// The panel's content; whether "Copy" was clicked.
pub(super) fn panel(
    ui: &mut Ui,
    theme: &Theme,
    standup: &Standup,
    settings: &mut Settings,
    now: chrono::DateTime<chrono::Local>,
) -> bool {
    ui.set_width(PANEL_WIDTH);
    ui.spacing_mut().item_spacing.y = 0.0;
    let p = &theme.palette;
    // Header: title, the period's start, reset; the periods.
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new(tl("Stand-up")).font(fonts::semibold(theme.text_size + 2.0)),
                );
                ui.add(bevel_theme::Chip::new(&standup.since, theme.tones.blue).pill());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let changed = Settings {
                        last_copied: settings.last_copied,
                        ..Settings::default()
                    } != *settings;
                    if changed && link(ui, theme, tl("Reset")).clicked() {
                        *settings = Settings {
                            last_copied: settings.last_copied,
                            ..Settings::default()
                        };
                    }
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (period, label) in [
                    (Period::LastWorkingDay, tl("Last working day")),
                    (Period::Day, tl("24 h")),
                    (Period::Week, tl("7 days")),
                    (Period::SinceLastCopy, tl("Since the last copy")),
                ] {
                    let never = period == Period::SinceLastCopy && settings.last_copied.is_none();
                    let r = ui.add_enabled_ui(!never, |ui| {
                        super::super::widgets::pill_toggle(
                            ui,
                            theme,
                            label,
                            theme.tones.blue,
                            settings.period == period,
                        )
                    });
                    let r = r.inner;
                    if never {
                        Tip::new()
                            .text(tl("Once you have copied a stand-up"))
                            .show(r.clone());
                    } else if period == Period::SinceLastCopy {
                        if let Some(at) = settings.last_copied() {
                            let at = at.with_timezone(&chrono::Local);
                            Tip::new()
                                .text(tf(
                                    "Last copied {when}",
                                    &[("when", &at.format("%Y-%m-%d %H:%M").to_string())],
                                ))
                                .show(r.clone());
                        }
                    }
                    if r.clicked() {
                        settings.period = period;
                    }
                }
            });
        });
    separator(ui, theme);

    // Body: the sections and the format on the left, the preview on the right.
    let mut body_height = 0.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let left = ui
            .allocate_ui_with_layout(
                Vec2::new(LEFT_WIDTH, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(LEFT_WIDTH);
                    ui.add_space(6.0);
                    heading(ui, theme, tl("What I did"));
                    for s in Section::DONE {
                        toggle_row(ui, theme, s, standup.of(s).len(), settings);
                    }
                    heading(ui, theme, tl("Today"));
                    for s in Section::TODAY.into_iter().chain([Section::Waiting]) {
                        toggle_row(ui, theme, s, standup.of(s).len(), settings);
                    }
                    heading(ui, theme, tl("Format"));
                    egui::Frame::new()
                        .inner_margin(Margin {
                            left: 16,
                            right: 12,
                            top: 2,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for (format, label, tip) in [
                                    (
                                        Format::Markdown,
                                        tl("Markdown"),
                                        tl("GitLab, Mattermost, Teams, Discord"),
                                    ),
                                    (Format::Slack, tl("Slack"), tl("Slack's own bold and links")),
                                    (Format::Plain, tl("Plain text"), tl("No markup, no links")),
                                ] {
                                    let r = super::super::widgets::pill_toggle(
                                        ui,
                                        theme,
                                        label,
                                        theme.tones.purple,
                                        settings.format == format,
                                    )
                                    .tip(tip);
                                    if r.clicked() {
                                        settings.format = format;
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            ui.add_enabled_ui(settings.format != Format::Plain, |ui| {
                                let on = settings.links && settings.format != Format::Plain;
                                if super::super::widgets::switch(
                                    ui,
                                    theme,
                                    on,
                                    tl("Links to the MRs and tickets"),
                                )
                                .clicked()
                                {
                                    settings.links = !settings.links;
                                }
                            });
                        });
                    ui.add_space(10.0);
                },
            )
            .response
            .rect;
        body_height = left.height();
        // The line between the columns.
        let x = left.right();
        ui.painter().line_segment(
            [
                egui::pos2(x, left.top()),
                egui::pos2(x, left.top() + body_height),
            ],
            Stroke::new(1.0, p.border),
        );
        ui.allocate_ui_with_layout(
            Vec2::new(PANEL_WIDTH - LEFT_WIDTH, body_height),
            Layout::top_down(Align::Min),
            |ui| {
                egui::Frame::new()
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 12,
                        bottom: 12,
                    })
                    .show(ui, |ui| {
                        preview(ui, theme, standup, settings, body_height - 24.0);
                    });
            },
        );
    });
    separator(ui, theme);

    // Footer: what is kept, and Copy.
    let mut copy = false;
    egui::Frame::new()
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let kept = match settings.last_copied() {
                    Some(at) => format!(
                        "{} · {}",
                        tl("Kept for the next stand-ups"),
                        tf(
                            "last copied {time} ago",
                            &[(
                                "time",
                                &super::super::board::duration_text(
                                    super::super::board::days_between(
                                        at,
                                        now.with_timezone(&chrono::Utc)
                                    )
                                    .max(0.0)
                                )
                            )]
                        )
                    ),
                    None => tl("Kept for the next stand-ups").to_string(),
                };
                ui.label(muted(theme, kept).size(theme.text_size - 1.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    copy = super::super::widgets::button(
                        ui,
                        theme,
                        Some(Icon::Copy),
                        tl("Copy"),
                        true,
                    )
                    .clicked();
                });
            });
        });
    copy
}

/// A line across the panel: `border`, as `border_soft` hardly shows on `raised`.
pub(super) fn separator(ui: &mut Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme.palette.border);
}

pub(super) fn link(ui: &mut Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .color(theme.palette.accent)
                .size(theme.text_size - 1.0),
        )
        .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small caps heading, as the notifications' days.
pub(super) fn heading(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(muted(theme, text.to_uppercase()).size(theme.text_size - 2.5));
        });
}

/// A section's row: its icon, its name, how many MRs it has, its switch.
pub(super) fn toggle_row(
    ui: &mut Ui,
    theme: &Theme,
    section: Section,
    count: usize,
    settings: &mut Settings,
) {
    let p = &theme.palette;
    let on = settings.shows(section);
    let (outer, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    let rect = outer.shrink2(Vec2::new(8.0, 1.0));
    if ui.is_rect_visible(outer) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, CornerRadius::same(theme.radii.medium), p.border_soft);
        }
        let tone = section.tone(theme);
        let how_on = ui.ctx().animate_bool(response.id, on);
        let center = egui::pos2(rect.left() + 20.0, rect.center().y);
        painter.circle_filled(center, 12.0, lerp(p.border_soft, tone.bg, how_on));
        icons::paint(
            painter,
            Rect::from_center_size(center, Vec2::splat(12.0)),
            section.icon(),
            lerp(p.text_muted, tone.fg, how_on),
        );
        let name = painter.layout_no_wrap(
            section.label().to_string(),
            FontId::proportional(theme.text_size),
            if on { p.text } else { p.text_muted },
        );
        painter.galley(
            egui::pos2(rect.left() + 42.0, rect.center().y - name.size().y / 2.0),
            name,
            p.text,
        );
        let track = super::super::widgets::switch_track(
            ui,
            theme,
            egui::pos2(rect.right() - 38.0, rect.center().y),
            how_on,
        );
        // The count, as a pill in the section's tone while it has some.
        let n = painter.layout_no_wrap(
            count.to_string(),
            fonts::semibold(theme.text_size - 2.0),
            if count > 0 && on {
                tone.fg
            } else {
                p.text_muted
            },
        );
        let pill = Rect::from_center_size(
            egui::pos2(
                track.left() - 12.0 - (n.size().x + 14.0) / 2.0,
                rect.center().y,
            ),
            Vec2::new((n.size().x + 14.0).max(22.0), 18.0),
        );
        if count > 0 && on {
            painter.rect_filled(pill, CornerRadius::same(9), tone.bg);
        }
        painter.galley(pill.center() - n.size() / 2.0, n, p.text_muted);
    }
    super::super::widgets::focus_ring(ui, theme, &response, theme.radii.medium);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        settings.hidden ^= section.bit();
    }
}

pub(super) fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// The stand-up as the chat will show it: bold title, the parts in small caps,
/// each heading in its section's tone, the links in the accent.
pub(super) fn preview(
    ui: &mut Ui,
    theme: &Theme,
    standup: &Standup,
    settings: &Settings,
    height: f32,
) {
    let p = &theme.palette;
    let lines = lines(standup, settings);
    let mrs = lines.iter().filter(|l| matches!(l, Line::Item(_))).count();
    ui.horizontal(|ui| {
        ui.label(muted(theme, tl("Preview").to_uppercase()).size(theme.text_size - 2.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(muted(theme, tn(mrs as u64, "{n} MR", "{n} MRs")).size(theme.text_size - 2.5));
        });
    });
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(p.content)
        .stroke(Stroke::new(1.0, p.border_soft))
        .corner_radius(CornerRadius::same(theme.radii.medium))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height((height - 52.0).max(120.0))
                .min_scrolled_height((height - 52.0).max(120.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let wrap = ui.available_width();
                    let size = theme.text_size - 1.0;
                    for line in &lines {
                        match line {
                            Line::Title(t) => {
                                ui.label(RichText::new(t).font(fonts::semibold(size + 1.5)));
                            }
                            Line::Part(t) => {
                                ui.label(
                                    RichText::new(t)
                                        .italics()
                                        .size(size - 0.5)
                                        .color(p.text_muted),
                                );
                            }
                            Line::Heading(s) => {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    super::super::widgets::dot(ui, s.tone(theme).fg, 6.0);
                                    ui.label(
                                        RichText::new(s.label())
                                            .font(fonts::semibold(size))
                                            .color(s.tone(theme).fg),
                                    );
                                });
                            }
                            Line::Item(t) => {
                                let mut job = LayoutJob::default();
                                let plain = TextFormat {
                                    font_id: FontId::proportional(size),
                                    color: p.text_secondary,
                                    ..TextFormat::default()
                                };
                                for (piece, url) in pieces(t) {
                                    let linked = url.is_some() && settings.format != Format::Plain;
                                    job.append(
                                        piece,
                                        0.0,
                                        if linked {
                                            TextFormat {
                                                color: p.accent,
                                                ..plain.clone()
                                            }
                                        } else {
                                            plain.clone()
                                        },
                                    );
                                }
                                job.wrap.max_width = wrap - 34.0;
                                // The bullet hangs: the wrapped lines align
                                // with the text.
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.add_sized(
                                        Vec2::new(16.0, size + 4.0),
                                        egui::Label::new(
                                            RichText::new("•").size(size).color(p.text_muted),
                                        ),
                                    );
                                    ui.add(egui::Label::new(job).wrap());
                                });
                            }
                            Line::Nothing(t) => {
                                ui.label(
                                    RichText::new(format!("•  {t}"))
                                        .size(size)
                                        .italics()
                                        .color(p.text_muted),
                                );
                            }
                            Line::Gap => ui.add_space(8.0),
                        }
                    }
                });
        });
}
