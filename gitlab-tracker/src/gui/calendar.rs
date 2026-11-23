//! A day picked on a month's calendar, in the window's language and colours:
//! a button showing the day, the month under it on a click.

use bevel_theme::{fonts, Theme};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use egui::{Color32, CornerRadius, Margin, RichText, Sense, Stroke, Ui, Vec2};

use super::icons::Icon;
use super::tip::TipExt;

const CELL: Vec2 = Vec2::new(32.0, 26.0);

/// A button with `value` ("16/11/2026", or `placeholder`); a click shows the
/// calendar, `range` shaded on it. The day picked, once picked.
pub fn date_button(
    ui: &mut Ui,
    theme: &Theme,
    id_salt: &str,
    value: Option<NaiveDate>,
    placeholder: &str,
    range: (Option<NaiveDate>, Option<NaiveDate>),
) -> Option<NaiveDate> {
    let text = value.map_or_else(
        || placeholder.to_string(),
        |d| d.format("%d/%m/%Y").to_string(),
    );
    let response = super::widgets::button(ui, theme, Some(Icon::Calendar), &text, false);
    let id = ui.make_persistent_id(("calendar", id_salt));
    let mut picked = None;
    let frame = egui::Frame::new()
        .fill(theme.palette.raised)
        .stroke(Stroke::new(1.0, theme.palette.border))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(Margin::same(10));
    egui::Popup::menu(&response)
        .frame(frame)
        .gap(4.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            picked = month(ui, theme, id, value, range);
            if picked.is_some() {
                ui.close();
            }
        });
    picked
}

fn first_of(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

fn add_months(first: NaiveDate, months: i32) -> NaiveDate {
    let index = first.year() * 12 + first.month0() as i32 + months;
    NaiveDate::from_ymd_opt(index.div_euclid(12), index.rem_euclid(12) as u32 + 1, 1)
        .unwrap_or(first)
}

/// The month shown (kept under `id` while browsing), weeks from Monday; the day
/// clicked.
fn month(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    selected: Option<NaiveDate>,
    range: (Option<NaiveDate>, Option<NaiveDate>),
) -> Option<NaiveDate> {
    let today = chrono::Local::now().date_naive();
    let mut shown: NaiveDate = ui
        .data(|d| d.get_temp(id))
        .unwrap_or_else(|| first_of(selected.unwrap_or(today)));
    let p = &theme.palette;
    let mut picked = None;
    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);

    ui.horizontal(|ui| {
        ui.set_width(CELL.x * 7.0);
        let arrow = |ui: &mut Ui, text: &str| {
            ui.add_sized(
                CELL,
                egui::Button::new(RichText::new(text).color(p.text_secondary)).frame(false),
            )
        };
        if arrow(ui, "‹")
            .tip(crate::i18n::tl("Previous month"))
            .clicked()
        {
            shown = add_months(shown, -1);
        }
        let title = format!(
            "{} {}",
            super::widgets::month_name(shown.month0() as usize),
            shown.year()
        );
        ui.add_sized(
            Vec2::new(CELL.x * 5.0, CELL.y),
            egui::Label::new(
                RichText::new(title)
                    .font(fonts::semibold(theme.text_size))
                    .color(p.text),
            ),
        );
        if arrow(ui, "›").tip(crate::i18n::tl("Next month")).clicked() {
            shown = add_months(shown, 1);
        }
    });

    ui.horizontal(|ui| {
        for day in [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ] {
            ui.add_sized(
                CELL,
                egui::Label::new(
                    RichText::new(crate::snooze::weekday_short(day))
                        .size(theme.text_size - 2.0)
                        .color(p.text_muted),
                ),
            );
        }
    });

    let start = shown - Duration::days(i64::from(shown.weekday().num_days_from_monday()));
    let next = add_months(shown, 1);
    let in_range = |day: NaiveDate| match range {
        (Some(from), Some(to)) => day >= from && day <= to,
        _ => false,
    };
    let mut day = start;
    while day < next {
        ui.horizontal(|ui| {
            for _ in 0..7 {
                let (rect, response) = ui.allocate_exact_size(CELL, Sense::click());
                let other_month = day.month() != shown.month();
                let chosen = selected == Some(day);
                let radius = CornerRadius::same(theme.radii.small);
                let fill = if chosen {
                    p.accent_fill
                } else if in_range(day) {
                    p.selection
                } else if response.hovered() {
                    p.selection.gamma_multiply(0.6)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect.shrink(1.0), radius, fill);
                if day == today && !chosen {
                    ui.painter().rect_stroke(
                        rect.shrink(1.0),
                        radius,
                        Stroke::new(1.0, p.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                let color = if chosen {
                    Color32::WHITE
                } else if other_month {
                    p.text_muted
                } else {
                    p.text
                };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    day.day().to_string(),
                    egui::FontId::proportional(theme.text_size - 1.0),
                    color,
                );
                if response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    picked = Some(day);
                }
                day += Duration::days(1);
            }
        });
    }
    if ui
        .add(
            egui::Button::new(RichText::new(crate::i18n::tl("Today")).color(p.accent)).frame(false),
        )
        .clicked()
    {
        picked = Some(today);
    }
    ui.data_mut(|d| d.insert_temp(id, shown));
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_roll_over_the_year() {
        let d = |y, m| NaiveDate::from_ymd_opt(y, m, 1).unwrap();
        assert_eq!(add_months(d(2026, 12), 1), d(2027, 1));
        assert_eq!(add_months(d(2026, 1), -1), d(2025, 12));
        assert_eq!(add_months(d(2026, 11), 0), d(2026, 11));
    }
}
