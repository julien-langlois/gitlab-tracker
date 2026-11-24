//! The status strip under the toolbar: counts by status, legend, next-step chips.

use super::*;

/// The MRs of the list by status at a glance: a bar, a segment per status as
/// wide as its share, over a legend with the counts. A click on either shows
/// that status alone (again: all of them).
pub(super) fn status_bar(g: &mut GuiApp, ui: &mut Ui) {
    let counts = g.app.status_counts();
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    if total == 0 {
        return;
    }
    let theme = g.theme;
    let picked = g.app.status_pick;
    let mut clicked = None;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The bar.
            const HEIGHT: f32 = 8.0;
            const GAP: f32 = 3.0;
            let (bar, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), HEIGHT + 8.0),
                Sense::hover(),
            );
            let usable = bar.width() - GAP * (counts.len() - 1) as f32;
            let mut x = bar.left();
            for &(kind, n) in &counts {
                let width = usable * n as f32 / total as f32;
                let segment = Rect::from_min_size(
                    egui::pos2(x, bar.center().y - HEIGHT / 2.0),
                    Vec2::new(width, HEIGHT),
                );
                x += width + GAP;
                let swatch = tones::status_swatch(kind, &theme.tones);
                let response = ui.interact(
                    segment.expand2(Vec2::new(GAP / 2.0, 4.0)),
                    ui.scope_id().with(("status segment", kind)),
                    Sense::click(),
                );
                let on = picked == Some(kind);
                let color = if picked.is_some() && !on {
                    swatch.gamma_multiply(0.25)
                } else if response.hovered() {
                    swatch.gamma_multiply(1.15)
                } else {
                    swatch
                };
                let radius = CornerRadius::same((HEIGHT / 2.0) as u8);
                ui.painter().rect_filled(segment, radius, color);
                if on {
                    ui.painter().rect_stroke(
                        segment.expand(2.0),
                        CornerRadius::same((HEIGHT / 2.0 + 2.0) as u8),
                        Stroke::new(1.5, theme.palette.text),
                        egui::StrokeKind::Middle,
                    );
                }
                if status_hint(response, kind, n, total, on).clicked() {
                    clicked = Some(kind);
                }
            }
            ui.add_space(4.0);
            // The legend, the total on the right.
            let total_text = if picked.is_some() {
                tl("Click it again to show them all").to_string()
            } else {
                tn(total as u64, "{n} MR in the list", "{n} MRs in the list")
            };
            let total_galley = ui.painter().layout_no_wrap(
                total_text,
                egui::FontId::proportional(theme.text_size - 2.0),
                theme.palette.text_muted,
            );
            ui.horizontal_top(|ui| {
                let legend_width = ui.available_width() - total_galley.size().x - 16.0;
                ui.allocate_ui_with_layout(
                    Vec2::new(legend_width, 0.0),
                    Layout::left_to_right(Align::Center).with_main_wrap(true),
                    |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for &(kind, n) in &counts {
                            let on = picked == Some(kind);
                            let dim = picked.is_some() && !on;
                            let response = legend_item(ui, &theme, kind, n, on, dim);
                            if status_hint(response, kind, n, total, on).clicked() {
                                clicked = Some(kind);
                            }
                        }
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(total_galley.size().x, 24.0),
                        Sense::hover(),
                    );
                    ui.painter().galley(
                        egui::pos2(rect.left(), rect.center().y - total_galley.size().y / 2.0),
                        total_galley,
                        theme.palette.text_muted,
                    );
                });
            });
        });
    if let Some(kind) = clicked {
        let pick = (picked != Some(kind)).then_some(kind);
        g.pending.push(Action::SetStatusPick(pick));
    }
}

/// A status of the legend: its dot, its name, its count; filled when `on`,
/// faded when `dim` (another status is picked).
pub(super) fn legend_item(
    ui: &mut Ui,
    theme: &Theme,
    kind: crate::status::StatusKind,
    count: usize,
    on: bool,
    dim: bool,
) -> egui::Response {
    let tone = tones::status_tone(kind, &theme.tones);
    let name_font = fonts::medium(theme.text_size - 1.5);
    let count_font = fonts::semibold(theme.text_size - 1.5);
    let text_color = if dim {
        theme.palette.text_muted
    } else {
        theme.palette.text_secondary
    };
    let name = ui
        .painter()
        .layout_no_wrap(tl(kind.name()).to_string(), name_font, text_color);
    let number = ui.painter().layout_no_wrap(
        count.to_string(),
        count_font,
        if dim {
            theme.palette.text_muted
        } else {
            theme.palette.text
        },
    );
    let size = Vec2::new(
        10.0 + 7.0 + 6.0 + name.size().x + 6.0 + number.size().x + 10.0,
        24.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = CornerRadius::same(12);
    if on {
        ui.painter().rect_filled(rect, radius, tone.bg);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tone.fg),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, theme.palette.raised);
    }
    let swatch = tones::status_swatch(kind, &theme.tones);
    let dot = if dim {
        swatch.gamma_multiply(0.35)
    } else {
        swatch
    };
    let mut x = rect.left() + 10.0;
    ui.painter()
        .circle_filled(egui::pos2(x + 3.5, rect.center().y), 3.5, dot);
    x += 13.0;
    let name_width = name.size().x;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - name.size().y / 2.0),
        name,
        text_color,
    );
    x += name_width + 6.0;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - number.size().y / 2.0),
        number,
        theme.palette.text,
    );
    super::super::widgets::focus_ring(ui, theme, &response, 12);
    response
}

/// The tone of a next-step bucket: mine in orange, the rest by stage.
pub fn bucket_tone(bucket: crate::next_step::Bucket, tones: &bevel_theme::Tones) -> Tone {
    use crate::next_step::Bucket;
    match bucket {
        Bucket::OnMe => tones.orange,
        Bucket::OnAuthor => tones.amber,
        Bucket::OnReviewers => tones.purple,
        Bucket::ToMerge => tones.teal,
        Bucket::ToDeploy => tones.blue,
        Bucket::Done => tones.green,
        Bucket::Unknown => tones.gray,
    }
}

/// Who the tracked MRs wait on, a chip per bucket with its count: a click lists
/// that bucket alone (again: all of them).
pub(super) fn next_chips(g: &mut GuiApp, ui: &mut Ui) {
    use crate::next_step::Bucket;
    if g.app.mrs.is_empty() {
        return;
    }
    let theme = g.theme;
    let steps = crate::next_step::all(&g.app);
    let current = g.app.focus.as_ref().map(|f| f.key.clone());
    let mut clicked = None;
    egui::Frame::new()
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 0,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
                for bucket in Bucket::ALL {
                    let n = steps.values().filter(|s| s.bucket() == bucket).count();
                    if bucket == Bucket::Unknown || bucket == Bucket::ToMerge && n == 0 {
                        continue;
                    }
                    let on = current.as_deref() == Some(bucket.key());
                    let tone = bucket_tone(bucket, &theme.tones);
                    let response = pill(
                        ui,
                        &theme,
                        bucket.label(),
                        n,
                        tone,
                        on,
                        current.is_some() && !on,
                    );
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .tip(tl(if on {
                            "Click to show them all"
                        } else {
                            "Click to show only these"
                        }))
                        .clicked()
                    {
                        clicked = Some((bucket, on));
                    }
                }
            });
        });
    match clicked {
        Some((_, true)) => g.next_focus = Some(None),
        Some((bucket, false)) => g.focus_on(bucket.key(), bucket.label()),
        None => {}
    }
}

/// A rounded chip with a dot, a name and a count; filled when `on`.
pub(super) fn pill(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    count: usize,
    tone: Tone,
    on: bool,
    dim: bool,
) -> egui::Response {
    let font = fonts::medium(theme.text_size - 1.5);
    let text_color = if dim {
        theme.palette.text_muted
    } else {
        theme.palette.text_secondary
    };
    let name_g = ui
        .painter()
        .layout_no_wrap(name.to_owned(), font.clone(), text_color);
    let count_g = ui.painter().layout_no_wrap(
        count.to_string(),
        fonts::semibold(theme.text_size - 1.5),
        if count > 0 && !dim {
            tone.fg
        } else {
            theme.palette.text_muted
        },
    );
    let size = Vec2::new(
        10.0 + 13.0 + name_g.size().x + 6.0 + count_g.size().x + 10.0,
        24.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = CornerRadius::same(12);
    if on {
        ui.painter().rect_filled(rect, radius, tone.bg);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, tone.fg),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, radius, theme.palette.raised);
    } else {
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, theme.palette.border_soft),
            egui::StrokeKind::Inside,
        );
    }
    let mut x = rect.left() + 10.0;
    ui.painter().circle_filled(
        egui::pos2(x + 3.5, rect.center().y),
        3.5,
        if dim {
            tone.fg.gamma_multiply(0.35)
        } else {
            tone.fg
        },
    );
    x += 13.0;
    let w = name_g.size().x;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - name_g.size().y / 2.0),
        name_g,
        text_color,
    );
    x += w + 6.0;
    ui.painter().galley(
        egui::pos2(x, rect.center().y - count_g.size().y / 2.0),
        count_g,
        theme.palette.text,
    );
    super::super::widgets::focus_ring(ui, theme, &response, 12);
    response
}

/// "Conflict · 1 MR (14 %)" and what a click does, on a segment or a legend item.
pub(super) fn status_hint(
    response: egui::Response,
    kind: crate::status::StatusKind,
    count: usize,
    total: usize,
    on: bool,
) -> egui::Response {
    let share = (count as f32 * 100.0 / total as f32).round();
    let hint = format!(
        "{} · {} ({share} %)\n{}",
        tl(kind.name()),
        tn(count as u64, "{n} MR", "{n} MRs"),
        tl(if on {
            "Click to show them all"
        } else {
            "Click to show only these"
        }),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .tip(hint)
}
