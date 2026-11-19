//! The pipeline schedules on the CI page: the nightly runs nobody watches.
//! Each schedule with its cron in words, its latest runs as a strip, when it
//! runs next and whose it is; a banner when one keeps failing, a notification
//! in the bell when one fails while the app is open, and a warning when its
//! owner's account is blocked (GitLab no longer runs it).

use std::collections::HashMap;

use bevel_theme::{fonts, Theme};
use chrono::{DateTime, Utc};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2};

use super::board::{self, paint_text};
use super::icons::{self, Icon};
use super::tip::{Tip, TipExt};
use super::toasts::{Kind, Toasts};
use crate::gitlab_detail::{ScheduleRecord, ScheduleRun, SCHEDULE_RUNS};
use crate::i18n::{tc, tf, tl, tn};
use crate::models::PipelineState;

/// Failed runs in a row that make a banner.
const STREAK: usize = 2;

/// The failed runs at the end of its history, the runs still going aside.
pub fn failing_streak(s: &ScheduleRecord) -> usize {
    s.runs
        .iter()
        .rev()
        .filter(|r| finished(r.status))
        .take_while(|r| r.status == PipelineState::Failed)
        .count()
}

fn finished(status: PipelineState) -> bool {
    !matches!(
        status,
        PipelineState::Running | PipelineState::Pending | PipelineState::Created
    )
}

/// Whether its owner can no longer run it: blocked, deactivated, banned.
pub fn owner_gone(s: &ScheduleRecord) -> bool {
    !s.owner_state.is_empty() && s.owner_state != "active"
}

fn day_name(n: u32) -> &'static str {
    tc(
        "date",
        match n {
            1 => "Monday",
            2 => "Tuesday",
            3 => "Wednesday",
            4 => "Thursday",
            5 => "Friday",
            6 => "Saturday",
            _ => "Sunday",
        },
    )
}

/// The cron in words when it is a common one ("every day at 02:00",
/// "weekdays at 19:00", "every Monday at 08:00", "every hour at :15"), in its
/// own zone; `None` otherwise (the cron shows as it is).
pub fn humanize(cron: &str) -> Option<String> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    let [minute, hour, day, month, weekday] = parts.as_slice() else {
        return None;
    };
    let minute: u32 = minute.parse().ok().filter(|m| *m < 60)?;
    if *day != "*" || *month != "*" {
        return None;
    }
    if *hour == "*" {
        return (*weekday == "*")
            .then(|| tf("every hour at :{m}", &[("m", &format!("{minute:02}"))]));
    }
    let hour: u32 = hour.parse().ok().filter(|h| *h < 24)?;
    let time = format!("{hour:02}:{minute:02}");
    match *weekday {
        "*" => Some(tf("every day at {time}", &[("time", &time)])),
        "1-5" | "MON-FRI" | "mon-fri" => Some(tf("weekdays at {time}", &[("time", &time)])),
        "0,6" | "6,0" | "6-7" | "SAT,SUN" | "sat,sun" => {
            Some(tf("weekends at {time}", &[("time", &time)]))
        }
        d => {
            let n: u32 = d.parse().ok().filter(|n| *n <= 7)?;
            Some(tf(
                "every {day} at {time}",
                &[("day", day_name(n)), ("time", &time)],
            ))
        }
    }
}

/// The schedule that fails the most runs in a row (at least [`STREAK`]),
/// active only, and its streak.
pub fn worst(schedules: &[ScheduleRecord]) -> Option<(&ScheduleRecord, usize)> {
    schedules
        .iter()
        .filter(|s| s.active)
        .map(|s| (s, failing_streak(s)))
        .filter(|(_, n)| *n >= STREAK)
        .max_by_key(|(_, n)| *n)
}

/// The banner of a schedule failing run after run: which, how many, the last
/// green run; the url of its last run when "Open the last run" is clicked.
pub fn banner(
    ui: &mut Ui,
    theme: &Theme,
    schedules: &[ScheduleRecord],
    project: &str,
) -> Option<String> {
    let (s, n) = worst(schedules)?;
    let red = theme.tones.red;
    let last_green = s
        .runs
        .iter()
        .rev()
        .find(|r| r.status == PipelineState::Success)
        .and_then(|r| board::parse_time(&r.created_at));
    let title = tf(
        "{name} has failed {n} runs in a row on {project}",
        &[
            ("name", &name(s)),
            ("n", &n.to_string()),
            ("project", project),
        ],
    );
    let detail = match last_green {
        Some(at) => tf(
            "Last green run: {when}",
            &[("when", &crate::freeze::moment_text(at))],
        ),
        None => tf(
            "No green run in the last {n}",
            &[("n", &s.runs.len().to_string())],
        ),
    };
    let mut open = None;
    egui::Frame::new()
        .fill(red.bg)
        .stroke(Stroke::new(1.0, red.fg.gamma_multiply(0.4)))
        .corner_radius(CornerRadius::same(theme.radii.large))
        .inner_margin(egui::Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (icon, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                icons::paint(ui.painter(), icon, Icon::Pulse, red.fg);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        egui::RichText::new(&title)
                            .font(fonts::semibold(theme.text_size))
                            .color(red.fg),
                    );
                    ui.label(super::widgets::muted(theme, &detail));
                });
                if let Some(run) = s.runs.last().filter(|r| !r.web_url.is_empty()) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if super::widgets::button(
                            ui,
                            theme,
                            Some(Icon::ExternalLink),
                            tl("Open the last run"),
                            false,
                        )
                        .clicked()
                        {
                            open = Some(run.web_url.clone());
                        }
                    });
                }
            });
        });
    open
}

fn name(s: &ScheduleRecord) -> String {
    if s.description.trim().is_empty() {
        tf("Schedule #{id}", &[("id", &s.id.to_string())])
    } else {
        s.description.clone()
    }
}

/// Column widths of the table: schedule, when, runs, next, owner (the owner
/// dropped when narrow).
/// The owner's avatar, beside their name.
const OWNER_AVATAR: f32 = 22.0;

/// The Next cell's two lines: running now, else the next run, else a dash.
fn next_lines(s: &ScheduleRecord, now: DateTime<Utc>) -> (String, String) {
    let running = s.runs.last().filter(|r| !finished(r.status));
    let next = s
        .next_run_at
        .as_deref()
        .and_then(board::parse_time)
        .filter(|_| s.active);
    if let Some(r) = running {
        let since = board::parse_time(&r.created_at)
            .map(|t| board::ago(t, now))
            .unwrap_or_default();
        (
            tl("running").to_string(),
            tf("started {ago}", &[("ago", &since)]),
        )
    } else if owner_gone(s) && s.active {
        (
            tl("will not run").to_string(),
            tl("owner blocked").to_string(),
        )
    } else if let Some(at) = next {
        (
            crate::snooze::when_text(at, &chrono::Local::now()),
            tf("in {left}", &[("left", &crate::freeze::left_text(now, at))]),
        )
    } else {
        (board::dash().to_string(), String::new())
    }
}

/// The owner's account state, when it no longer runs anything.
fn owner_state_text(s: &ScheduleRecord) -> String {
    match s.owner_state.as_str() {
        "blocked" | "blocked_pending_approval" => tl("account blocked").to_string(),
        "deactivated" => tl("account deactivated").to_string(),
        "banned" => tl("account banned").to_string(),
        other => tf("account {state}", &[("state", other)]),
    }
}

/// What When, Next and Owner need to show every schedule whole.
struct Needs {
    when: f32,
    next: f32,
    owner: f32,
}

/// The columns: When, Next and Owner as wide as their widest line as long
/// as Schedule keeps room for its names, the rest to Schedule.
fn columns(width: f32, need: &Needs) -> [f32; 5] {
    let runs = SCHEDULE_RUNS as f32 * 13.0;
    let next = need.next.clamp(90.0, 160.0);
    let owner = if width > 760.0 {
        need.owner.clamp(110.0, 220.0)
    } else {
        0.0
    };
    let free = width - runs - next - owner - 48.0;
    let when = need.when.min(free - 200.0).max(170.0);
    let name = (free - when).max(120.0);
    [name, when, runs, next, owner]
}

fn needs(ui: &Ui, theme: &Theme, schedules: &[ScheduleRecord], now: DateTime<Utc>) -> Needs {
    let measure = |text: &str, font: FontId| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font, Color32::WHITE)
            .size()
            .x
    };
    let line = FontId::proportional(theme.text_size - 1.0);
    let small = FontId::proportional(theme.text_size - 2.0);
    let widest =
        |f: &dyn Fn(&ScheduleRecord) -> f32| schedules.iter().map(f).fold(0.0, f32::max) + 12.0;
    Needs {
        when: widest(&|s| {
            let words = humanize(&s.cron).unwrap_or_else(|| s.cron.clone());
            let cron = format!("{} · {}", s.cron, s.zone);
            measure(&words, line.clone())
                .max(measure(&cron, FontId::monospace(theme.text_size - 2.0)))
        }),
        next: widest(&|s| {
            let (first, second) = next_lines(s, now);
            measure(&first, line.clone()).max(measure(&second, small.clone()))
        }),
        owner: widest(&|s| {
            let state = if owner_gone(s) {
                measure(&owner_state_text(s), small.clone())
            } else {
                0.0
            };
            OWNER_AVATAR + 8.0 + measure(&s.owner, line.clone()).max(state)
        }),
    }
}

/// The card of the schedules; the url to open when a run or the GitLab link
/// is clicked. `schedules_url`: the project's schedules page.
pub fn card(
    ui: &mut Ui,
    theme: &Theme,
    schedules: &[ScheduleRecord],
    schedules_url: Option<&str>,
    now: DateTime<Utc>,
) -> Option<String> {
    let mut open = None;
    let note = tf(
        "last {n} runs, oldest left · times in your zone",
        &[("n", &SCHEDULE_RUNS.to_string())],
    );
    board::card(ui, theme, tl("Pipeline schedules"), &note, 0.0, |ui| {
        let width = ui.available_width();
        let cols = columns(width, &needs(ui, theme, schedules, now));
        header(ui, theme, &cols);
        for (i, s) in schedules.iter().enumerate() {
            if let Some(url) = row(ui, theme, s, &cols, now, i + 1 == schedules.len()) {
                open = Some(url);
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.add(
                egui::Label::new(super::widgets::muted(
                    theme,
                    tl("A failed run lands in the bell. A schedule whose owner is blocked no longer runs in GitLab: it is flagged."),
                ))
                .wrap(),
            );
        });
        if let Some(url) = schedules_url {
            ui.add_space(4.0);
            if ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(tl("Open the schedules in GitLab"))
                            .color(theme.palette.accent)
                            .size(theme.text_size - 1.0),
                    )
                    .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                open = Some(url.to_string());
            }
        }
    });
    open
}

fn header(ui: &mut Ui, theme: &Theme, cols: &[f32; 5]) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let labels = [
        tl("Schedule"),
        tl("When"),
        tl("Runs"),
        tl("Next"),
        tl("Owner"),
    ];
    let mut x = rect.left();
    for (w, label) in cols.iter().zip(labels) {
        if *w > 0.0 {
            paint_text(
                ui,
                egui::pos2(x, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(theme.text_size - 1.5),
                theme.palette.text_muted,
                *w - 8.0,
            );
        }
        x += w + 12.0;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0, theme.palette.border_soft),
    );
}

fn run_color(theme: &Theme, run: Option<&ScheduleRun>) -> Color32 {
    match run.map(|r| r.status) {
        Some(PipelineState::Success) => theme.tones.green.fg,
        Some(PipelineState::Failed) => theme.tones.red.fg,
        Some(PipelineState::Running | PipelineState::Pending | PipelineState::Created) => {
            theme.tones.blue.fg
        }
        Some(_) => theme.palette.text_muted,
        None => theme.palette.border_soft,
    }
}

/// One schedule; the url of a run clicked.
fn row(
    ui: &mut Ui,
    theme: &Theme,
    s: &ScheduleRecord,
    cols: &[f32; 5],
    now: DateTime<Utc>,
    last: bool,
) -> Option<String> {
    let height = 48.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let mut open = None;
    let top = rect.top() + 15.0;
    let bottom = rect.top() + 33.0;
    let dim = !s.active;
    let text = if dim {
        theme.palette.text_secondary
    } else {
        theme.palette.text
    };
    let small = FontId::proportional(theme.text_size - 2.0);
    // The schedule: its name, then its branch, inactive or failing.
    let mut x = rect.left();
    paint_text(
        ui,
        egui::pos2(x, top),
        Align2::LEFT_CENTER,
        &name(s),
        fonts::semibold(theme.text_size - 0.5),
        text,
        cols[0] - 8.0,
    );
    let streak = failing_streak(s);
    let (state, state_color) = if !s.active {
        (tl("inactive").to_string(), theme.palette.text_muted)
    } else if streak > 0 {
        (
            tn(streak as u64, "{n} failed run", "{n} failed runs in a row"),
            theme.tones.red.fg,
        )
    } else {
        (String::new(), theme.palette.text_muted)
    };
    let w = paint_text(
        ui,
        egui::pos2(x, bottom),
        Align2::LEFT_CENTER,
        &s.ref_name,
        FontId::monospace(theme.text_size - 2.5),
        theme.palette.text_muted,
        cols[0] * 0.5,
    );
    if !state.is_empty() {
        paint_text(
            ui,
            egui::pos2(x + w + 8.0, bottom),
            Align2::LEFT_CENTER,
            &format!("· {state}"),
            small.clone(),
            state_color,
            cols[0] - w - 16.0,
        );
    }
    x += cols[0] + 12.0;
    // When: in words, the cron under it.
    let words = humanize(&s.cron).unwrap_or_else(|| s.cron.clone());
    paint_text(
        ui,
        egui::pos2(x, top),
        Align2::LEFT_CENTER,
        &words,
        FontId::proportional(theme.text_size - 1.0),
        text,
        cols[1] - 8.0,
    );
    paint_text(
        ui,
        egui::pos2(x, bottom),
        Align2::LEFT_CENTER,
        &format!("{} · {}", s.cron, s.zone),
        FontId::monospace(theme.text_size - 2.0),
        theme.palette.text_muted,
        cols[1] - 8.0,
    );
    let when_rect = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(cols[1], height));
    ui.interact(
        when_rect,
        ui.scope_id().with(("sched_when", s.id)),
        Sense::hover(),
    )
    .rich(Tip::new().mono(&s.cron).text(tf(
        "{cron}, read in {zone}",
        &[("cron", &s.cron), ("zone", &s.zone)],
    )));
    x += cols[1] + 12.0;
    // The runs, oldest left, padded with empty slots.
    let pad = SCHEDULE_RUNS.saturating_sub(s.runs.len());
    for k in 0..SCHEDULE_RUNS {
        let run = k.checked_sub(pad).and_then(|i| s.runs.get(i));
        let cell = Rect::from_min_size(
            egui::pos2(x + k as f32 * 13.0, rect.center().y - 10.0),
            Vec2::new(10.0, 20.0),
        );
        ui.painter()
            .rect_filled(cell, CornerRadius::same(2), run_color(theme, run));
        let Some(run) = run else {
            continue;
        };
        let response = ui
            .interact(
                cell.expand2(Vec2::new(1.5, 2.0)),
                ui.scope_id().with(("sched_run", s.id, run.id)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() {
            let (label, tone) = super::tones::pipeline(run.status, &theme.tones);
            let when = board::parse_time(&run.created_at)
                .map(crate::freeze::moment_text)
                .unwrap_or_default();
            response.clone().rich(
                Tip::new()
                    .item(format!("#{}", run.id), &when)
                    .chip(label.to_lowercase(), tone),
            );
        }
        if response.clicked() && !run.web_url.is_empty() {
            open = Some(run.web_url.clone());
        }
    }
    x += cols[2] + 12.0;
    // Next: running now, else the next run, else a dash.
    let running = s.runs.last().filter(|r| !finished(r.status));
    let (first, second) = next_lines(s, now);
    paint_text(
        ui,
        egui::pos2(x, top),
        Align2::LEFT_CENTER,
        &first,
        FontId::proportional(theme.text_size - 1.0),
        if running.is_some() {
            theme.tones.blue.fg
        } else {
            text
        },
        cols[3] - 8.0,
    );
    paint_text(
        ui,
        egui::pos2(x, bottom),
        Align2::LEFT_CENTER,
        &second,
        small.clone(),
        theme.palette.text_muted,
        cols[3] - 8.0,
    );
    x += cols[3] + 12.0;
    // The owner, flagged when their account no longer runs anything.
    if cols[4] > 0.0 {
        let gone = owner_gone(s);
        if !s.owner.is_empty() {
            let cell =
                Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(OWNER_AVATAR, height));
            let mut cell = board::inside(ui, cell);
            super::widgets::avatar(&mut cell, theme, &s.owner, OWNER_AVATAR)
                .rich(Tip::new().person(&s.owner));
        }
        let x = x + OWNER_AVATAR + 8.0;
        // One line centred on the avatar, two lines centred together.
        let name_y = if gone { top } else { rect.center().y };
        paint_text(
            ui,
            egui::pos2(x, name_y),
            Align2::LEFT_CENTER,
            if s.owner.is_empty() {
                board::dash()
            } else {
                &s.owner
            },
            FontId::proportional(theme.text_size - 1.0),
            if gone { theme.tones.amber.fg } else { text },
            cols[4] - OWNER_AVATAR - 12.0,
        );
        if gone {
            paint_text(
                ui,
                egui::pos2(x, bottom),
                Align2::LEFT_CENTER,
                &owner_state_text(s),
                small,
                theme.tones.amber.fg,
                cols[4] - OWNER_AVATAR - 12.0,
            );
        }
    }
    if !last {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, theme.palette.border_soft),
        );
    }
    open
}

/// The latest run seen of each schedule, by project and schedule: what the
/// bell has already been told.
pub type Seen = HashMap<(String, u64), (u64, PipelineState)>;

/// Adds a notification for each schedule whose latest run failed since the
/// last look. The first look only records what is there: what failed before
/// the app opened shows in the banner, not in the bell.
pub fn watch(
    toasts: &mut Toasts,
    seen: &mut Seen,
    project: &str,
    schedules: &[ScheduleRecord],
    theme: &Theme,
    now: f64,
) {
    for s in schedules {
        let Some(run) = s.runs.last() else {
            continue;
        };
        let before = seen.insert((project.to_string(), s.id), (run.id, run.status));
        let Some(before) = before else {
            continue;
        };
        if before != (run.id, run.status) && run.status == PipelineState::Failed {
            toasts.push(
                &schedule_id(s.id),
                &name(s),
                Kind::ScheduleFailed,
                tf("{name} · scheduled pipeline failed", &[("name", &name(s))]),
                theme.tones.red,
                now,
            );
        }
    }
}

/// The id of a schedule's notification: it opens the CI page, not an MR.
pub fn schedule_id(id: u64) -> String {
    format!("schedule:{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(statuses: &[PipelineState]) -> ScheduleRecord {
        ScheduleRecord {
            id: 1,
            description: "Nightly e2e".into(),
            ref_name: "main".into(),
            cron: "0 2 * * *".into(),
            zone: "Europe/Paris".into(),
            next_run_at: None,
            active: true,
            owner: "Alex".into(),
            owner_state: "active".into(),
            runs: statuses
                .iter()
                .enumerate()
                .map(|(i, s)| ScheduleRun {
                    id: i as u64,
                    status: *s,
                    created_at: String::new(),
                    web_url: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn crons_read_in_words_when_common() {
        assert_eq!(humanize("0 2 * * *").as_deref(), Some("every day at 02:00"));
        assert_eq!(
            humanize("0 19 * * 1-5").as_deref(),
            Some("weekdays at 19:00")
        );
        assert_eq!(
            humanize("0 8 * * 1").as_deref(),
            Some("every Monday at 08:00")
        );
        assert_eq!(humanize("15 * * * *").as_deref(), Some("every hour at :15"));
        assert_eq!(humanize("0 0 1 * *"), None);
        assert_eq!(humanize("*/5 * * * *"), None);
    }

    #[test]
    fn the_streak_counts_the_last_failures_past_a_running_one() {
        use PipelineState::*;
        assert_eq!(failing_streak(&schedule(&[Success, Failed, Failed])), 2);
        assert_eq!(failing_streak(&schedule(&[Failed, Failed, Running])), 2);
        assert_eq!(failing_streak(&schedule(&[Failed, Success])), 0);
        let mut inactive = schedule(&[Failed, Failed, Failed]);
        assert!(worst(std::slice::from_ref(&inactive)).is_some());
        inactive.active = false;
        assert!(worst(std::slice::from_ref(&inactive)).is_none());
    }

    #[test]
    fn the_bell_hears_of_a_new_failure_only() {
        use PipelineState::*;
        let theme = Theme::dark();
        let mut toasts = Toasts::default();
        let mut seen = Seen::new();
        let mut s = schedule(&[Success, Failed]);
        // First look: recorded, not notified.
        watch(
            &mut toasts,
            &mut seen,
            "p",
            std::slice::from_ref(&s),
            &theme,
            0.0,
        );
        assert_eq!(toasts.unread(), 0);
        // The same run again: nothing.
        watch(
            &mut toasts,
            &mut seen,
            "p",
            std::slice::from_ref(&s),
            &theme,
            1.0,
        );
        assert_eq!(toasts.unread(), 0);
        // A new run that failed: one notification.
        s.runs.push(ScheduleRun {
            id: 9,
            status: Failed,
            created_at: String::new(),
            web_url: String::new(),
        });
        watch(
            &mut toasts,
            &mut seen,
            "p",
            std::slice::from_ref(&s),
            &theme,
            2.0,
        );
        assert_eq!(toasts.unread(), 1);
        assert_eq!(toasts.history[0].id, "schedule:1");
    }
}
