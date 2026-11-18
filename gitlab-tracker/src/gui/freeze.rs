//! Deploy freezes in the window: a banner where one would deploy (Promote,
//! Environments, Today) while a freeze holds or is about to, and the card of
//! the next two weeks on the Environments page. See [`crate::freeze`].

use bevel_theme::{fonts, Theme, Tone};
use chrono::{DateTime, Duration, Local, TimeZone, Utc};
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, paint_text};
use super::icons::{self, Icon};
use crate::app::App;
use crate::freeze::{left_text, moment_text, zoned_text, Freezes, Period, Source, Status};
use crate::gitlab_detail::ProjectOverview;
use crate::i18n::{tf, tl, tn};

/// How soon a freeze to come gets its banner.
const SOON_HOURS: i64 = 48;
/// Days of the strip.
const STRIP_DAYS: i64 = 14;

/// The project's freezes: GitLab's, else the app's setting.
pub fn of(app: &App, overview: Option<&ProjectOverview>) -> Freezes {
    let (declared, denied) = overview
        .map(|o| (o.freezes.as_slice(), o.freezes_denied))
        .unwrap_or((&[], false));
    Freezes::of(declared, denied, &app.config.freeze_periods)
}

/// What the banner says, when it shows.
struct Banner {
    title: String,
    detail: String,
    chip: String,
    frozen: bool,
}

fn banner_text(freezes: &Freezes, now: DateTime<Utc>, project: Option<&str>) -> Option<Banner> {
    let (period, title, chip, frozen) = match freezes.status(now) {
        Status::Frozen { period, until, .. } => {
            let title = match until {
                Some(until) => tf(
                    "Deploy freeze until {when}",
                    &[("when", &moment_text(until))],
                ),
                None => tl("Deploy freeze, with no end in sight").to_string(),
            };
            let chip = until
                .map(|until| tf("ends in {left}", &[("left", &left_text(now, until))]))
                .unwrap_or_default();
            (period, title, chip, true)
        }
        Status::Next { period, start, end } if start - now <= Duration::hours(SOON_HOURS) => {
            let title = match end {
                Some(end) => tf(
                    "Deploy freeze from {start} to {end}",
                    &[("start", &moment_text(start)), ("end", &moment_text(end))],
                ),
                None => tf(
                    "Deploy freeze from {start}",
                    &[("start", &moment_text(start))],
                ),
            };
            let chip = tf("starts in {left}", &[("left", &left_text(now, start))]);
            (period, title, chip, false)
        }
        _ => return None,
    };
    let title = match project {
        Some(name) => format!("{name} · {title}"),
        None => title,
    };
    let p = &freezes.periods[period];
    let why = match freezes.source {
        Source::Gitlab => tl("deploy jobs that check $CI_DEPLOY_FREEZE do not run"),
        Source::App => tl("from the app's settings: a warning, the pipelines do not know of it"),
    };
    let detail = format!(
        "{} → {} · {} · {why}",
        p.start_text,
        p.end_text,
        p.zone.name()
    );
    Some(Banner {
        title,
        detail,
        chip,
        frozen,
    })
}

/// The banner of a freeze that holds, or starts within [`SOON_HOURS`];
/// nothing otherwise. `project` names the project (on Today). Clicked: the
/// Environments page shows the freezes.
pub fn banner(
    ui: &mut Ui,
    theme: &Theme,
    freezes: &Freezes,
    now: DateTime<Utc>,
    project: Option<&str>,
) -> Option<egui::Response> {
    let b = banner_text(freezes, now, project)?;
    let tone = if b.frozen {
        theme.tones.blue
    } else {
        theme.tones.gray
    };
    let accent = theme.tones.blue.fg;
    let height = 56.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(theme.radii.large),
        tone.bg,
        Stroke::new(
            1.0,
            accent.gamma_multiply(if b.frozen { 0.45 } else { 0.25 }),
        ),
        egui::StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        Vec2::splat(20.0),
    );
    icons::paint(painter, icon, Icon::Snowflake, accent);
    let chip_font = fonts::medium(theme.text_size - 1.5);
    let chip_w = if b.chip.is_empty() {
        0.0
    } else {
        let galley = board::one_line(ui, &b.chip, chip_font.clone(), accent, 200.0);
        let size = galley.size() + Vec2::new(16.0, 6.0);
        let chip = Rect::from_min_size(
            egui::pos2(rect.right() - 16.0 - size.x, rect.center().y - size.y / 2.0),
            size,
        );
        ui.painter().rect_filled(
            chip,
            CornerRadius::same(theme.radii.small),
            theme.tones.blue.bg,
        );
        ui.painter()
            .galley(chip.center() - galley.size() / 2.0, galley, accent);
        size.x + 16.0
    };
    let left = rect.left() + 48.0;
    let room = (rect.right() - 16.0 - chip_w - left).max(40.0);
    let title_color = if b.frozen {
        tone.fg
    } else {
        theme.palette.text
    };
    paint_text(
        ui,
        egui::pos2(left, rect.top() + 19.0),
        Align2::LEFT_CENTER,
        &b.title,
        fonts::semibold(theme.text_size),
        title_color,
        room,
    );
    paint_text(
        ui,
        egui::pos2(left, rect.top() + 38.0),
        Align2::LEFT_CENTER,
        &b.detail,
        FontId::proportional(theme.text_size - 1.5),
        theme.palette.text_muted,
        room,
    );
    Some(
        response
            .on_hover_text(format!("{}\n{}", b.title, b.detail))
            .on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

/// The card of the freezes: the next [`STRIP_DAYS`] days, frozen hours in
/// blue, then each period with its crons, its zone and when it comes next.
/// Nothing when the project has none (a note when the token may not read
/// GitLab's).
pub fn card(ui: &mut Ui, theme: &Theme, freezes: &Freezes, now: DateTime<Utc>) {
    if freezes.is_empty() && freezes.unreadable == 0 {
        if freezes.denied {
            board::card(ui, theme, tl("Deploy freezes"), "", 0.0, |ui| {
                board::empty(
                    ui,
                    theme,
                    tl("The token may not read the project's deploy freezes (Freeze Period › Read)."),
                );
            });
            ui.add_space(board::GAP);
        }
        return;
    }
    let note = match freezes.source {
        Source::Gitlab => tl("declared in GitLab · times in your zone"),
        Source::App => tl("declared in the app's settings · times in your zone"),
    };
    board::card(ui, theme, tl("Deploy freezes"), note, 0.0, |ui| {
        strip(ui, theme, freezes, now);
        ui.add_space(12.0);
        for (i, p) in freezes.periods.iter().enumerate() {
            period_row(ui, theme, p, now, i + 1 == freezes.periods.len());
        }
        let mut notes = Vec::new();
        if freezes.unreadable > 0 {
            notes.push(tn(
                freezes.unreadable as u64,
                "{n} period could not be read (its cron or its time zone).",
                "{n} periods could not be read (their cron or their time zone).",
            ));
        }
        match freezes.source {
            Source::Gitlab => notes.push(
                tl("GitLab sets $CI_DEPLOY_FREEZE in the pipelines that run during one: the deploy jobs that check it do not run.")
                    .to_string(),
            ),
            Source::App => {
                notes.push(
                    tl("The project declares none in GitLab: these come from the app's settings, as a warning only.")
                        .to_string(),
                );
                if freezes.denied {
                    notes.push(
                        tl("The token may not read GitLab's (Freeze Period › Read).").to_string(),
                    );
                }
            }
        }
        ui.add_space(8.0);
        for n in notes {
            ui.add(egui::Label::new(super::widgets::muted(theme, n)).wrap());
        }
    });
    ui.add_space(board::GAP);
}

/// The days to come, each a cell, the frozen hours filled in blue; a line at
/// the present moment.
fn strip(ui: &mut Ui, theme: &Theme, freezes: &Freezes, now: DateTime<Utc>) {
    let today = now.with_timezone(&Local).date_naive();
    let midnight = |offset: i64| -> DateTime<Utc> {
        let date = today + Duration::days(offset);
        let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
        Local
            .from_local_datetime(&naive)
            .earliest()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
    };
    let from = midnight(0);
    let to = midnight(STRIP_DAYS);
    let windows = freezes.windows(from, to);
    let gap = 4.0;
    let width = ui.available_width();
    let cell_w = ((width - gap * (STRIP_DAYS - 1) as f32) / STRIP_DAYS as f32).floor();
    let height = 42.0;
    let (line, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let blue = theme.tones.blue;
    for i in 0..STRIP_DAYS {
        let cell = Rect::from_min_size(
            egui::pos2(line.left() + i as f32 * (cell_w + gap), line.top()),
            Vec2::new(cell_w, height),
        );
        let (start, end) = (midnight(i), midnight(i + 1));
        let span = (end - start).num_seconds().max(1) as f32;
        // The day's name above, its hours in a bar below.
        let bar = Rect::from_min_max(egui::pos2(cell.left(), cell.top() + 20.0), cell.max);
        let radius = CornerRadius::same(theme.radii.small);
        ui.painter()
            .rect_filled(bar, radius, theme.palette.border_soft.gamma_multiply(0.7));
        let mut frozen = false;
        for (_, w) in &windows {
            let a = w.start.max(start);
            let b = w.end.unwrap_or(to).min(end);
            if a >= b {
                continue;
            }
            frozen = true;
            let x = |t: DateTime<Utc>| {
                bar.left() + bar.width() * (t - start).num_seconds() as f32 / span
            };
            let part =
                Rect::from_min_max(egui::pos2(x(a), bar.top()), egui::pos2(x(b), bar.bottom()));
            ui.painter()
                .rect_filled(part, radius, blue.fg.gamma_multiply(0.55));
        }
        let day = start.with_timezone(&Local);
        let label = format!(
            "{} {}",
            crate::snooze::weekday_short(chrono::Datelike::weekday(&day)),
            day.format("%d")
        );
        let (font, color) = if i == 0 {
            (fonts::semibold(theme.text_size - 2.5), theme.palette.text)
        } else if frozen {
            (fonts::medium(theme.text_size - 2.5), blue.fg)
        } else {
            (
                fonts::medium(theme.text_size - 2.5),
                theme.palette.text_muted,
            )
        };
        paint_text(
            ui,
            egui::pos2(cell.left() + 2.0, cell.top() + 8.0),
            Align2::LEFT_CENTER,
            &label,
            font,
            color,
            cell.width() - 4.0,
        );
        if i == 0 {
            // Now.
            let x = bar.left() + bar.width() * (now - start).num_seconds() as f32 / span;
            ui.painter().line_segment(
                [
                    egui::pos2(x, bar.top() - 3.0),
                    egui::pos2(x, bar.bottom() + 3.0),
                ],
                Stroke::new(2.0, theme.palette.text),
            );
        }
        // The day's freezes, in your zone and in the period's.
        let hover = ui.interact(cell, ui.scope_id().with(("freeze_day", i)), Sense::hover());
        if hover.hovered() {
            hover.on_hover_text(day_text(freezes, &windows, start, end));
        }
    }
}

/// The freezes that meet a day, for its tooltip.
fn day_text(
    freezes: &Freezes,
    windows: &[(usize, crate::freeze::Window)],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> String {
    let lines: Vec<String> = windows
        .iter()
        .filter(|(_, w)| w.start < end && w.end.is_none_or(|e| e > start))
        .map(|(i, w)| {
            let p = &freezes.periods[*i];
            let until = w.end.map(moment_text).unwrap_or_else(|| "…".into());
            // The second line: the start as the period's cron reads it.
            format!(
                "{} → {until}\n  {}",
                moment_text(w.start),
                zoned_text(w.start, p.zone)
            )
        })
        .collect();
    if lines.is_empty() {
        tl("No freeze this day").to_string()
    } else {
        lines.join("\n")
    }
}

/// A period: its crons and zone, then whether it holds now or when it comes.
fn period_row(ui: &mut Ui, theme: &Theme, p: &Period, now: DateTime<Utc>, last: bool) {
    let height = 34.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let w = rect.width();
    let cron = format!("{} → {}", p.start_text, p.end_text);
    paint_text(
        ui,
        egui::pos2(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        &cron,
        FontId::monospace(theme.text_size - 1.5),
        theme.palette.text,
        w * 0.42 - 12.0,
    );
    paint_text(
        ui,
        egui::pos2(rect.left() + w * 0.42, rect.center().y),
        Align2::LEFT_CENTER,
        &p.zone.name(),
        FontId::proportional(theme.text_size - 1.0),
        theme.palette.text_muted,
        w * 0.2 - 12.0,
    );
    let (text, tone): (String, Option<Tone>) = match p.active(now) {
        Some((_, Some(until))) => (
            tf("now, until {when}", &[("when", &moment_text(until))]),
            Some(theme.tones.blue),
        ),
        Some((_, None)) => (tl("now, with no end").to_string(), Some(theme.tones.blue)),
        None => match p.windows(now, now + Duration::days(366 * 4)).first() {
            Some(next) => (
                match next.end {
                    Some(end) => tf(
                        "next: {start} → {end}",
                        &[
                            ("start", &moment_text(next.start)),
                            ("end", &moment_text(end)),
                        ],
                    ),
                    None => tf("next: {start}", &[("start", &moment_text(next.start))]),
                },
                None,
            ),
            None => (tl("never again").to_string(), None),
        },
    };
    paint_text(
        ui,
        egui::pos2(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        &text,
        if tone.is_some() {
            fonts::medium(theme.text_size - 1.0)
        } else {
            FontId::proportional(theme.text_size - 1.0)
        },
        tone.map(|t| t.fg).unwrap_or(theme.palette.text),
        w * 0.38,
    );
    if !last {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, theme.palette.border_soft),
        );
    }
    response.on_hover_text(tf(
        "Read in {zone}, with its daylight saving changes",
        &[("zone", &p.zone.name())],
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::freeze::Declared;

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn weekend(denied: bool, setting: &str, gitlab: bool) -> Freezes {
        let declared = [Declared {
            start: "0 18 * * 5".into(),
            end: "0 7 * * 1".into(),
            zone: "Europe/Paris".into(),
        }];
        Freezes::of(if gitlab { &declared } else { &[] }, denied, setting)
    }

    #[test]
    fn the_banner_shows_in_a_freeze_and_just_before_one() {
        let f = weekend(false, "", true);
        // Wednesday: the weekend is more than two days away.
        assert!(banner_text(&f, utc("2026-11-18T11:00:00Z"), None).is_none());
        // Thursday noon: it starts in a day and a few hours.
        let soon = banner_text(&f, utc("2026-11-19T11:00:00Z"), None).unwrap();
        assert!(!soon.frozen);
        assert!(soon.chip.starts_with("starts in 1 d"));
        // Saturday: frozen, the GitLab variable named.
        let frozen = banner_text(&f, utc("2026-11-21T11:00:00Z"), Some("Orbit API")).unwrap();
        assert!(frozen.frozen);
        assert!(frozen.title.starts_with("Orbit API · Deploy freeze until"));
        assert!(frozen.detail.contains("$CI_DEPLOY_FREEZE"));
        assert!(frozen.detail.contains("Europe/Paris"));
    }

    #[test]
    fn an_app_freeze_says_it_is_only_a_warning() {
        let f = weekend(true, "0 18 * * 5 -> 0 7 * * 1 Europe/Paris", false);
        let frozen = banner_text(&f, utc("2026-11-21T11:00:00Z"), None).unwrap();
        assert!(frozen.detail.contains("a warning"));
        assert!(!frozen.detail.contains("$CI_DEPLOY_FREEZE"));
    }
}
