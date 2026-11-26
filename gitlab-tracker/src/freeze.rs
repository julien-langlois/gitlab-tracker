//! Deploy freezes: when a project should not deploy.
//!
//! GitLab's freeze periods (Deploy freezes, in the project's CI/CD settings)
//! are read first: a start cron, an end cron and the time zone both are read
//! in. GitLab sets `$CI_DEPLOY_FREEZE` in the pipelines that run inside one,
//! for the deploy jobs to check. A project that declares none in GitLab can
//! declare some in the app (the `freeze_periods` setting, same crons): those
//! only warn, nothing in the CI knows of them.
//!
//! Time zones: the crons are read in the period's own zone, with its daylight
//! saving rules (a 18:00 start is 18:00 in Paris in winter and in summer), and
//! the moments found are kept in UTC; the window shows them in the machine's
//! zone. A local time that does not exist (the hour skipped in spring) is
//! skipped, one that happens twice (in autumn) is taken the first time.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc};

/// The five fields of a cron (minute, hour, day of month, month, day of week),
/// as the sets of values they match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    /// Bits 1 to 31.
    days: u32,
    /// Bits 1 to 12.
    months: u16,
    /// Bits 0 (Sunday) to 6.
    weekdays: u8,
    /// Whether the day of month, and the day of week, were `*`: a day matches
    /// either restricted field when both are (as cron does), else both.
    any_day: bool,
    any_weekday: bool,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// The values of one field: `*`, `5`, `1-5`, `*/15`, `MON-FRI`, `1,15`.
fn field(text: &str, min: u32, max: u32, names: &[&str]) -> Option<(u64, bool)> {
    let value = |part: &str| -> Option<u32> {
        if let Ok(n) = part.parse::<u32>() {
            return Some(n);
        }
        let lower = part.to_ascii_lowercase();
        names
            .iter()
            .position(|n| *n == lower)
            .map(|i| i as u32 + min)
    };
    let mut bits = 0u64;
    for item in text.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, step.parse::<u32>().ok().filter(|s| *s > 0)?),
            None => (item, 1),
        };
        let (from, to) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a)?, value(b)?)
        } else {
            let at = value(range)?;
            // `5/10`: from 5 to the end, every 10.
            (at, if item.contains('/') { max } else { at })
        };
        if from < min || to > max || from > to {
            return None;
        }
        let mut n = from;
        while n <= to {
            bits |= 1 << n;
            n += step;
        }
    }
    (bits != 0).then_some((bits, text == "*"))
}

impl Cron {
    /// A five-field cron, as GitLab's freeze periods have them. `None` when it
    /// does not read as one.
    pub fn parse(text: &str) -> Option<Cron> {
        let parts: Vec<&str> = text.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = parts.as_slice() else {
            return None;
        };
        let (minutes, _) = field(minute, 0, 59, &[])?;
        let (hours, _) = field(hour, 0, 23, &[])?;
        let (days, any_day) = field(day, 1, 31, &[])?;
        let (months, _) = field(month, 1, 12, &MONTHS)?;
        // 7 is Sunday too.
        let (mut weekdays, any_weekday) = field(weekday, 0, 7, &WEEKDAYS)?;
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays | 1) & 0x7f;
        }
        Some(Cron {
            minutes,
            hours: hours as u32,
            days: days as u32,
            months: months as u16,
            weekdays: weekdays as u8,
            any_day,
            any_weekday,
        })
    }

    fn on(&self, date: NaiveDate) -> bool {
        if self.months & (1 << date.month()) == 0 {
            return false;
        }
        let by_day = self.days & (1 << date.day()) != 0;
        let by_weekday = self.weekdays & (1 << date.weekday().num_days_from_sunday()) != 0;
        match (self.any_day, self.any_weekday) {
            (false, false) => by_day || by_weekday,
            _ => by_day && by_weekday,
        }
    }

    /// The times of day it matches, earliest first.
    fn times(&self) -> impl DoubleEndedIterator<Item = (u32, u32)> + '_ {
        (0..24u32)
            .filter(|h| self.hours & (1 << h) != 0)
            .flat_map(|h| {
                (0..60u32)
                    .filter(|m| self.minutes & (1 << m) != 0)
                    .map(move |m| (h, m))
            })
    }

    /// The moments it matches on `date` in `zone`, earliest first.
    fn moments<Tz: TimeZone>(&self, zone: &Tz, date: NaiveDate) -> Vec<DateTime<Utc>> {
        if !self.on(date) {
            return Vec::new();
        }
        self.times()
            .filter_map(|(h, m)| {
                let naive = date.and_hms_opt(h, m, 0)?;
                match zone.from_local_datetime(&naive) {
                    LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => {
                        Some(at.with_timezone(&Utc))
                    }
                    LocalResult::None => None,
                }
            })
            .collect()
    }

    /// The first moment it matches after `after`.
    pub fn next<Tz: TimeZone>(&self, zone: &Tz, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let start = after.with_timezone(zone).date_naive() - Duration::days(1);
        (0..SEARCH_DAYS)
            .filter_map(|i| start.checked_add_signed(Duration::days(i)))
            .find_map(|date| self.moments(zone, date).into_iter().find(|at| *at > after))
    }

    /// The last moment it matched, `at` included.
    pub fn previous<Tz: TimeZone>(&self, zone: &Tz, at: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let start = at.with_timezone(zone).date_naive() + Duration::days(1);
        (0..SEARCH_DAYS)
            .filter_map(|i| start.checked_sub_signed(Duration::days(i)))
            .find_map(|date| {
                self.moments(zone, date)
                    .into_iter()
                    .rev()
                    .find(|t| *t <= at)
            })
    }
}

/// How many days a search goes through: enough for a cron of the 29th of
/// February.
const SEARCH_DAYS: i64 = 366 * 4 + 2;

/// The zone a period's crons are read in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zone {
    Named(chrono_tz::Tz),
    /// The machine's: a period declared in the app without a zone.
    Local,
}

impl Zone {
    pub fn parse(name: &str) -> Option<Zone> {
        // GitLab writes `UTC` (its default) or an IANA name.
        name.trim().parse::<chrono_tz::Tz>().ok().map(Zone::Named)
    }

    pub fn name(&self) -> String {
        match self {
            Zone::Named(tz) => tz.name().to_string(),
            Zone::Local => crate::i18n::tl("local time").to_string(),
        }
    }
}

/// Where a period was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// In GitLab: the pipelines know of it (`$CI_DEPLOY_FREEZE`).
    Gitlab,
    /// In the app's settings: a warning only.
    App,
}

/// A freeze period: from each moment `start` matches to the next one `end`
/// matches.
#[derive(Debug, Clone, PartialEq)]
pub struct Period {
    pub start: Cron,
    pub end: Cron,
    pub start_text: String,
    pub end_text: String,
    pub zone: Zone,
}

impl Period {
    pub fn new(start: &str, end: &str, zone: Zone) -> Option<Period> {
        Some(Period {
            start: Cron::parse(start)?,
            end: Cron::parse(end)?,
            start_text: start.split_whitespace().collect::<Vec<_>>().join(" "),
            end_text: end.split_whitespace().collect::<Vec<_>>().join(" "),
            zone,
        })
    }

    /// The period in words when both crons repeat alike, e.g. "Friday 18:00 →
    /// Monday 07:00, every week"; `None` otherwise (the crons show as they are).
    pub fn words(&self) -> Option<String> {
        period_words(&self.start_text, &self.end_text)
    }

    fn next(&self, cron: &Cron, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self.zone {
            Zone::Named(tz) => cron.next(&tz, after),
            Zone::Local => cron.next(&chrono::Local, after),
        }
    }

    fn previous(&self, cron: &Cron, at: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self.zone {
            Zone::Named(tz) => cron.previous(&tz, at),
            Zone::Local => cron.previous(&chrono::Local, at),
        }
    }

    /// The freeze `at` is in, as (since, until): it is when the last start
    /// came after the last end (GitLab's own rule).
    pub fn active(&self, at: DateTime<Utc>) -> Option<(DateTime<Utc>, Option<DateTime<Utc>>)> {
        let started = self.previous(&self.start, at)?;
        let ended = self.previous(&self.end, at);
        if ended.is_some_and(|ended| ended >= started) {
            return None;
        }
        Some((started, self.next(&self.end, at)))
    }

    /// The freezes that meet `from..to`, in order, as (start, end).
    pub fn windows(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Window> {
        let mut windows = Vec::new();
        let mut at = from;
        if let Some((since, until)) = self.active(from) {
            windows.push(Window {
                start: since,
                end: until,
            });
            match until {
                Some(until) => at = until,
                None => return windows,
            }
        }
        while windows.len() < MAX_WINDOWS {
            let Some(start) = self.next(&self.start, at).filter(|s| *s < to) else {
                break;
            };
            let end = self.next(&self.end, start);
            windows.push(Window { start, end });
            match end {
                Some(end) => at = end,
                None => break,
            }
        }
        windows
    }
}

/// Freezes listed at most for one period.
const MAX_WINDOWS: usize = 64;

/// One freeze, from `start` to `end` (none: it never ends).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

/// A project's freeze periods: GitLab's when it has some, else the app's.
#[derive(Debug, Clone, PartialEq)]
pub struct Freezes {
    pub periods: Vec<Period>,
    pub source: Source,
    /// Periods that could not be read (a cron or a zone), not counted in.
    pub unreadable: usize,
    /// The token may not read GitLab's (403).
    pub denied: bool,
}

/// A period as GitLab gives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Declared {
    pub start: String,
    pub end: String,
    pub zone: String,
}

impl Freezes {
    /// GitLab's periods when it has some, else those of the app's setting
    /// (`setting`, see [`parse_setting`]).
    pub fn of(gitlab: &[Declared], denied: bool, setting: &str) -> Freezes {
        if !gitlab.is_empty() {
            let periods: Vec<Period> = gitlab
                .iter()
                .filter_map(|p| Period::new(&p.start, &p.end, Zone::parse(&p.zone)?))
                .collect();
            return Freezes {
                unreadable: gitlab.len() - periods.len(),
                periods,
                source: Source::Gitlab,
                denied,
            };
        }
        let (periods, unreadable) = parse_setting(setting);
        Freezes {
            periods,
            source: Source::App,
            unreadable,
            denied,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.periods.is_empty()
    }

    /// Where the project stands at `now`.
    pub fn status(&self, now: DateTime<Utc>) -> Status {
        // In several at once: frozen until the last one ends.
        let frozen = self
            .periods
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.active(now).map(|(since, until)| (i, since, until)))
            .max_by_key(|(_, _, until)| until.unwrap_or(DateTime::<Utc>::MAX_UTC));
        if let Some((period, since, until)) = frozen {
            return Status::Frozen {
                period,
                since,
                until,
            };
        }
        let next = self
            .periods
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let start = p.next(&p.start, now)?;
                Some((i, start, p.next(&p.end, start)))
            })
            .min_by_key(|(_, start, _)| *start);
        match next {
            Some((period, start, end)) => Status::Next { period, start, end },
            None => Status::Clear,
        }
    }

    /// The freezes of every period that meet `from..to`, in order.
    pub fn windows(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<(usize, Window)> {
        let mut all: Vec<(usize, Window)> = self
            .periods
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.windows(from, to).into_iter().map(move |w| (i, w)))
            .collect();
        all.sort_by_key(|(_, w)| w.start);
        all
    }
}

/// Where a project stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// In a freeze (of `period`), until `until` (none: it never ends).
    Frozen {
        period: usize,
        since: DateTime<Utc>,
        until: Option<DateTime<Utc>>,
    },
    /// Not frozen; the next freeze starts at `start`.
    Next {
        period: usize,
        start: DateTime<Utc>,
        end: Option<DateTime<Utc>>,
    },
    /// No freeze to come.
    Clear,
}

/// The separators between a period's start and its end, in the setting.
const ARROWS: [&str; 3] = ["->", "→", ".."];

/// The periods of the `freeze_periods` setting, and how many could not be
/// read: `start -> end [zone]`, separated by `;` or new lines, the zone an
/// IANA name (the machine's when left out):
/// `0 18 * * 5 -> 0 7 * * 1 Europe/Paris; 0 0 20 12 * -> 0 0 5 1 *`.
pub fn parse_setting(text: &str) -> (Vec<Period>, usize) {
    let mut periods = Vec::new();
    let mut unreadable = 0;
    for entry in text
        .split([';', '\n'])
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        match parse_entry(entry) {
            Some(period) => periods.push(period),
            None => unreadable += 1,
        }
    }
    (periods, unreadable)
}

fn parse_entry(entry: &str) -> Option<Period> {
    let (start, rest) = ARROWS.iter().find_map(|arrow| entry.split_once(arrow))?;
    let parts: Vec<&str> = rest.split_whitespace().collect();
    let (end, zone) = match parts.len() {
        5 => (parts.join(" "), Zone::Local),
        6 => (parts[..5].join(" "), Zone::parse(parts[5])?),
        _ => return None,
    };
    Period::new(start, &end, zone)
}

/// How a freeze's cron repeats, for [`Period::words`].
#[derive(PartialEq)]
enum Repeat {
    Day,
    Week,
    Year,
}

/// A freeze cron as a moment of its repeat: "18:00", "Friday 18:00" or
/// "26/11 11:14".
fn cron_words(cron: &str) -> Option<(Repeat, String)> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    let [minute, hour, day, month, weekday] = parts.as_slice() else {
        return None;
    };
    let minute: u32 = minute.parse().ok().filter(|m| *m < 60)?;
    let hour: u32 = hour.parse().ok().filter(|h| *h < 24)?;
    let time = format!("{hour:02}:{minute:02}");
    match (*day, *month, *weekday) {
        ("*", "*", "*") => Some((Repeat::Day, time)),
        ("*", "*", d) => {
            let name = match d.parse::<u32>().ok().filter(|n| *n <= 7)? {
                1 => "Monday",
                2 => "Tuesday",
                3 => "Wednesday",
                4 => "Thursday",
                5 => "Friday",
                6 => "Saturday",
                _ => "Sunday",
            };
            Some((
                Repeat::Week,
                format!("{} {time}", crate::i18n::tc("date", name)),
            ))
        }
        (d, m, "*") => {
            let d: u32 = d.parse().ok().filter(|d| (1..=31).contains(d))?;
            let m: u32 = m.parse().ok().filter(|m| (1..=12).contains(m))?;
            Some((Repeat::Year, format!("{d:02}/{m:02} {time}")))
        }
        _ => None,
    }
}

/// Two freeze crons in words when they repeat alike; see [`Period::words`].
fn period_words(start: &str, end: &str) -> Option<String> {
    let (repeat, from) = cron_words(start)?;
    let (end_repeat, to) = cron_words(end)?;
    if repeat != end_repeat {
        return None;
    }
    let span = format!("{from} → {to}");
    Some(match repeat {
        Repeat::Day => crate::i18n::tf("{span}, every day", &[("span", &span)]),
        Repeat::Week => crate::i18n::tf("{span}, every week", &[("span", &span)]),
        Repeat::Year => crate::i18n::tf("{span}, every year", &[("span", &span)]),
    })
}

/// `at` in the machine's zone, as "Fri 21/11 18:00".
pub fn moment_text(at: DateTime<Utc>) -> String {
    crate::utils::moment_text(at)
}

/// `at` in the period's own zone, as "Fri 21/11 18:00 Europe/Paris": the time
/// its cron says.
pub fn zoned_text(at: DateTime<Utc>, zone: Zone) -> String {
    let (weekday, text) = match zone {
        Zone::Named(tz) => {
            let t = at.with_timezone(&tz);
            (t.weekday(), t.format("%d/%m %H:%M").to_string())
        }
        Zone::Local => {
            let t = at.with_timezone(&chrono::Local);
            (t.weekday(), t.format("%d/%m %H:%M").to_string())
        }
    };
    format!(
        "{} {text} {}",
        crate::snooze::weekday_short(weekday),
        zone.name()
    )
}

/// How long until `at`, short: "3 h 20 min", "2 d 14 h".
pub fn left_text(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let minutes = (at - now).num_minutes().max(1);
    let (days, hours, mins) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 {
        crate::i18n::tf(
            "{d} d {h} h",
            &[("d", &days.to_string()), ("h", &hours.to_string())],
        )
    } else if hours > 0 {
        crate::i18n::tf(
            "{h} h {m} min",
            &[("h", &hours.to_string()), ("m", &mins.to_string())],
        )
    } else {
        crate::i18n::tf("{m} min", &[("m", &mins.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_reads_in_words_when_its_crons_repeat_alike() {
        // Another test may install a translator: the words in its language.
        use crate::i18n::{tc, tf};
        let week = format!(
            "{} 18:00 → {} 07:00",
            tc("date", "Friday"),
            tc("date", "Monday")
        );
        assert_eq!(
            period_words("0 18 * * 5", "0 7 * * 1"),
            Some(tf("{span}, every week", &[("span", week.as_str())]))
        );
        assert_eq!(
            period_words("0 0 20 12 *", "0 0 5 1 *"),
            Some(tf(
                "{span}, every year",
                &[("span", "20/12 00:00 → 05/01 00:00")]
            ))
        );
        assert_eq!(
            period_words("0 20 * * *", "0 7 * * *"),
            Some(tf("{span}, every day", &[("span", "20:00 → 07:00")]))
        );
        // Unlike repeats, or steps and lists, stay as crons.
        assert_eq!(period_words("0 18 * * 5", "0 7 * * *"), None);
        assert_eq!(period_words("*/5 * * * *", "0 7 * * 1"), None);
        assert_eq!(period_words("0 18 * * 1-5", "0 7 * * 1"), None);
    }
    use chrono_tz::Europe::Paris;

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn weekend() -> Period {
        Period::new("0 18 * * 5", "0 7 * * 1", Zone::Named(Paris)).unwrap()
    }

    #[test]
    fn crons_read_as_cron_does() {
        assert!(Cron::parse("0 18 * * 5").is_some());
        assert!(Cron::parse("*/15 9-17 * * MON-FRI").is_some());
        assert!(Cron::parse("0 0 20 12 *").is_some());
        assert!(Cron::parse("0 0 * * 7").is_some());
        assert!(Cron::parse("0 18 * *").is_none());
        assert!(Cron::parse("61 18 * * 5").is_none());
        assert!(Cron::parse("0 18 * * fri-mon").is_none());
        let sunday = Cron::parse("0 0 * * 7").unwrap();
        assert!(sunday.on(NaiveDate::from_ymd_opt(2026, 11, 22).unwrap()));
    }

    #[test]
    fn a_weekend_freeze_holds_from_friday_evening_to_monday_morning() {
        let p = weekend();
        // Wednesday 18 November 2026, noon in Paris (UTC+1): not frozen.
        assert_eq!(p.active(utc("2026-11-18T11:00:00Z")), None);
        // Saturday: frozen since Friday 18:00 Paris (17:00 UTC), until Monday
        // 07:00 Paris (06:00 UTC).
        let (since, until) = p.active(utc("2026-11-21T10:00:00Z")).unwrap();
        assert_eq!(since, utc("2026-11-20T17:00:00Z"));
        assert_eq!(until, Some(utc("2026-11-23T06:00:00Z")));
        // On Monday 07:00 sharp it is over.
        assert_eq!(p.active(utc("2026-11-23T06:00:00Z")), None);
    }

    #[test]
    fn the_period_keeps_its_zone_through_daylight_saving() {
        let p = weekend();
        // Summer: Paris is UTC+2, so 18:00 there is 16:00 UTC.
        let windows = p.windows(utc("2026-07-01T00:00:00Z"), utc("2026-07-08T00:00:00Z"));
        assert_eq!(windows[0].start, utc("2026-07-03T16:00:00Z"));
        // The weekend of the clock change (25 October 2026): starts at UTC+2,
        // ends at UTC+1.
        let windows = p.windows(utc("2026-10-20T00:00:00Z"), utc("2026-10-27T00:00:00Z"));
        assert_eq!(windows[0].start, utc("2026-10-23T16:00:00Z"));
        assert_eq!(windows[0].end, Some(utc("2026-10-26T06:00:00Z")));
    }

    #[test]
    fn a_year_end_freeze_crosses_the_new_year() {
        let p = Period::new("0 0 20 12 *", "0 0 5 1 *", Zone::Named(Paris)).unwrap();
        let (since, until) = p.active(utc("2026-12-31T12:00:00Z")).unwrap();
        assert_eq!(since, utc("2026-12-19T23:00:00Z"));
        assert_eq!(until, Some(utc("2027-01-04T23:00:00Z")));
        let windows = p.windows(utc("2026-11-18T00:00:00Z"), utc("2026-12-02T00:00:00Z"));
        assert!(windows.is_empty());
    }

    #[test]
    fn the_status_names_the_freeze_that_ends_last_or_the_next_one() {
        let freezes = Freezes::of(
            &[
                Declared {
                    start: "0 18 * * 5".into(),
                    end: "0 7 * * 1".into(),
                    zone: "Europe/Paris".into(),
                },
                Declared {
                    start: "0 0 20 12 *".into(),
                    end: "0 0 5 1 *".into(),
                    zone: "Europe/Paris".into(),
                },
                Declared {
                    start: "0 0 * * *".into(),
                    end: "0 1 * * *".into(),
                    zone: "Mars/Olympus".into(),
                },
            ],
            false,
            "0 9 * * * -> 0 10 * * *",
        );
        assert_eq!(freezes.source, Source::Gitlab);
        assert_eq!((freezes.periods.len(), freezes.unreadable), (2, 1));
        match freezes.status(utc("2026-11-18T11:00:00Z")) {
            Status::Next { period, start, .. } => {
                assert_eq!(period, 0);
                assert_eq!(start, utc("2026-11-20T17:00:00Z"));
            }
            other => panic!("{other:?}"),
        }
        // Saturday 26 December: in both; the year-end one lasts longer.
        match freezes.status(utc("2026-12-26T12:00:00Z")) {
            Status::Frozen { period, until, .. } => {
                assert_eq!(period, 1);
                assert_eq!(until, Some(utc("2027-01-04T23:00:00Z")));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_setting_is_read_when_gitlab_has_none() {
        let freezes = Freezes::of(
            &[],
            true,
            "0 18 * * 5 -> 0 7 * * 1 Europe/Paris; 0 0 20 12 * → 0 0 5 1 *\nnonsense",
        );
        assert_eq!(freezes.source, Source::App);
        assert!(freezes.denied);
        assert_eq!(freezes.periods.len(), 2);
        assert_eq!(freezes.unreadable, 1);
        assert_eq!(freezes.periods[1].zone, Zone::Local);
        assert_eq!(freezes.periods[0].start_text, "0 18 * * 5");
        assert!(Freezes::of(&[], false, "  ").is_empty());
    }

    #[test]
    fn the_time_left_reads_short() {
        let now = utc("2026-11-18T11:00:00Z");
        assert_eq!(left_text(now, utc("2026-11-18T11:25:00Z")), "25 min");
        assert_eq!(left_text(now, utc("2026-11-18T14:05:00Z")), "3 h 5 min");
        assert_eq!(left_text(now, utc("2026-11-21T01:00:00Z")), "2 d 14 h");
    }
}
