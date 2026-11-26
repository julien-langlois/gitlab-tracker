//! Will the milestone make its date: the weekly pace and a Monte Carlo forecast.

use super::*;

/// The weeks the forecast draws from.
pub(super) const WEEKS: usize = 8;

/// The weeks of history a forecast needs at least.
pub(super) const MIN_WEEKS: usize = 3;

/// The MRs merged in each of the last [`WEEKS`] weeks, the oldest first,
/// from the first week with a merge. The project's merges when they were read
/// (`overview`: every MR, tracked or not, the team's real pace), else those of
/// the tracked MRs. The weeks before the first merge are weeks nothing is
/// known of, not weeks without work: drawn as zeros they would slow the
/// forecast down for no reason.
///
/// Merges, not arrivals on the last branch: production gets MRs in batches at
/// release time, so its weeks say when releases happen, not how fast the work
/// is done.
pub fn weekly_merges(
    app: &App,
    overview: Option<&crate::gitlab_detail::ProjectOverview>,
    now: DateTime<Utc>,
) -> Vec<u32> {
    let mut weeks = vec![0_u32; WEEKS];
    // A history still being read for the first time has only its oldest weeks.
    let project: Vec<&str> = overview
        .filter(|o| o.merges.is_current(now))
        .map(|o| o.merges.merged.values().map(String::as_str).collect())
        .unwrap_or_default();
    let dates: Vec<&str> = if project.is_empty() {
        app.mrs
            .iter()
            .filter(|mr| mr.state == GitlabMrState::Merged)
            .filter_map(|mr| mr.merged_at.as_deref())
            .collect()
    } else {
        project
    };
    for at in dates {
        let Some(at) = board::parse_time(at) else {
            continue;
        };
        let ago = (now - at).num_days();
        if (0..(WEEKS as i64 * 7)).contains(&ago) {
            weeks[WEEKS - 1 - (ago / 7) as usize] += 1;
        }
    }
    let first = weeks.iter().position(|n| *n > 0).unwrap_or(WEEKS);
    weeks.split_off(first)
}

/// The MRs merged a week, on the last four weeks of `weekly` (fewer when the
/// history is shorter).
pub(super) fn pace_of(weekly: &[u32]) -> f64 {
    let recent = &weekly[weekly.len().saturating_sub(4)..];
    f64::from(recent.iter().sum::<u32>()) / recent.len().max(1) as f64
}

/// The MRs of `m` not merged yet: what the forecast waits for.
pub(super) fn left_to_merge(m: &Milestone) -> usize {
    m.items
        .iter()
        .filter(|i| !matches!(i.stand, Stand::Delivered | Stand::OnTheWay))
        .count()
}

/// When a milestone will be done, from a Monte Carlo simulation: weeks drawn
/// at random from the recent ones until what is left is delivered, many
/// times over.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Forecast {
    /// Half the runs were done by then.
    pub p50: Option<NaiveDate>,
    /// 85 % of them were: the date to commit to.
    pub p85: Option<NaiveDate>,
    /// The share of the runs done by the due date.
    pub chance: Option<f64>,
}

/// Runs of the simulation.
pub(super) const RUNS: usize = 2000;

/// A small, seeded random generator: the same forecast on every frame.
pub(super) struct XorShift(u64);

impl XorShift {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// When every MR of `m` is merged, drawing from `weekly` (MRs merged a week).
pub fn forecast(m: &Milestone, weekly: &[u32], now: DateTime<Utc>) -> Forecast {
    let today = now.date_naive();
    if left_to_merge(m) == 0 {
        return Forecast {
            p50: Some(today),
            p85: Some(today),
            chance: m.due.map(|_| 1.0),
        };
    }
    let Some(days) = simulate(m, weekly) else {
        return Forecast::default();
    };
    let at = |p: f64| {
        let d = days[((days.len() - 1) as f64 * p).round() as usize];
        (d < 104 * 7).then(|| today + Duration::days(d))
    };
    Forecast {
        p50: at(0.5),
        p85: at(0.85),
        // A due date already past has no chance left to tell: the dates say
        // how late.
        chance: m.due.filter(|due| *due >= today).map(|due| {
            let within = (due - today).num_days();
            days.iter().filter(|d| **d <= within).count() as f64 / days.len() as f64
        }),
    }
}

/// The days from today each run took to merge what is left of `m`, sorted;
/// `None` with fewer than [`MIN_WEEKS`] weeks of merges to draw from.
pub(super) fn simulate(m: &Milestone, weekly: &[u32]) -> Option<Vec<i64>> {
    let left = left_to_merge(m) as u32;
    if weekly.len() < MIN_WEEKS || weekly.iter().all(|n| *n == 0) {
        return None;
    }
    let seed = m.title.bytes().fold(0x9E37_79B9_7F4A_7C15_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01B3)
    }) | 1;
    let mut rng = XorShift(seed);
    let mut days: Vec<i64> = (0..RUNS)
        .map(|_| {
            let mut done = 0;
            let mut week = 0_i64;
            // Two years at most: past that, it is no forecast.
            while week < 104 {
                let drawn = weekly[(rng.next() % weekly.len() as u64) as usize];
                if done + drawn >= left && drawn > 0 {
                    // Within the week, in proportion to what was left.
                    let share = f64::from(left - done) / f64::from(drawn);
                    return week * 7 + (share * 7.0).ceil() as i64;
                }
                done += drawn;
                week += 1;
            }
            week * 7
        })
        .collect();
    days.sort_unstable();
    Some(days)
}

/// The inspector's forecast: the share of the simulations done by the end of
/// each coming week, the due date's week marked, and the 50 / 85 / 95 % dates.
pub(super) fn forecast_section(
    ui: &mut Ui,
    theme: &Theme,
    m: &Milestone,
    weekly: &[u32],
    now: DateTime<Utc>,
) {
    let Some(days) = simulate(m, weekly) else {
        return;
    };
    let today = now.date_naive();
    let f = forecast(m, weekly, now);
    board::section(
        ui,
        theme,
        tl("Forecast"),
        &tf("{runs} simulations", &[("runs", &RUNS.to_string())]),
        |ui| {
            let p95 = days[((days.len() - 1) as f64 * 0.95).round() as usize];
            let weeks = ((p95 + 6) / 7).clamp(1, 16) as usize;
            // Every run done within the week: one full bar says nothing, the
            // dates under it do.
            if weeks > 1 {
                let due_week = m
                    .due
                    .map(|d| ((d - today).num_days() as f64 / 7.0).ceil() as i64);
                let (area, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 92.0), Sense::hover());
                let slot = area.width() / weeks as f32;
                for w in 0..weeks {
                    let end = (w as i64 + 1) * 7;
                    let share =
                        days.iter().filter(|d| **d <= end).count() as f32 / days.len() as f32;
                    let h = (area.height() - 18.0) * share;
                    let bar = Rect::from_min_max(
                        egui::pos2(
                            area.left() + w as f32 * slot + 2.0,
                            area.bottom() - 16.0 - h,
                        ),
                        egui::pos2(
                            area.left() + (w as f32 + 1.0) * slot - 2.0,
                            area.bottom() - 16.0,
                        ),
                    );
                    let in_time = due_week.is_none_or(|d| (w as i64) < d.max(0));
                    let color = if in_time {
                        theme.tones.green.fg
                    } else {
                        theme.tones.amber.fg
                    };
                    ui.painter()
                        .rect_filled(bar, CornerRadius::same(2), color.gamma_multiply(0.8));
                    ui.interact(
                        Rect::from_min_max(egui::pos2(bar.left(), area.top()), bar.max),
                        ui.scope_id().with(("forecast_week", w)),
                        Sense::hover(),
                    )
                    .tip(tf(
                        "{share} done by {date}",
                        &[
                            ("share", &board::percent(f64::from(share))),
                            ("date", &crate::utils::day_text(today + Duration::days(end))),
                        ],
                    ));
                    if w % (weeks / 4).max(1) == 0 {
                        board::paint_text(
                            ui,
                            egui::pos2(bar.center().x, area.bottom() - 6.0),
                            Align2::CENTER_CENTER,
                            &tf("W{n}", &[("n", &(w + 1).to_string())]),
                            FontId::proportional(theme.text_size - 3.0),
                            theme.palette.text_muted,
                            slot * 2.0,
                        );
                    }
                }
                if let Some(d) = due_week.filter(|d| *d > 0 && (*d as usize) <= weeks) {
                    let x = area.left() + d as f32 * slot;
                    dashed(
                        ui,
                        egui::pos2(x, area.top()),
                        egui::pos2(x, area.bottom() - 14.0),
                        theme.tones.amber.fg,
                    );
                }
            }
            ui.add_space(8.0);
            let date = |d: Option<NaiveDate>| {
                d.map_or_else(|| board::dash().to_string(), crate::utils::day_text)
            };
            let p95_date = (p95 < 104 * 7).then(|| today + Duration::days(p95));
            let mut rows = vec![
                (tl("50 % sure by"), date(f.p50)),
                (tl("85 % sure by"), date(f.p85)),
                (tl("95 % sure by"), date(p95_date)),
            ];
            if let Some(c) = f.chance {
                rows.push((tl("Chance by the due date"), board::percent(c)));
            }
            board::facts(ui, theme, &rows);
        },
    );
}

/// When the milestone can be promised: the forecast's 85 % date.
pub(super) fn projection(m: &Milestone, weekly: &[u32], now: DateTime<Utc>) -> Option<NaiveDate> {
    forecast(m, weekly, now).p85
}
