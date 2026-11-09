//! The demo's clock: every date of the demo is written relative to the moment
//! it starts, in working days at a local time of day (people do not merge on
//! Sundays at 3 am), or in hours for what happened today.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc, Weekday};
use std::sync::OnceLock;

/// A moment of the demo, relative to its start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum When {
    /// Minutes before now.
    Ago(i64),
    /// `n` working days before today, at a local time (minutes past midnight).
    Day(u32, i64),
    /// `n` working days after today, at a local time (minutes past midnight).
    Ahead(u32, i64),
}

/// `hours` before now.
pub const fn h(hours: i64) -> When {
    When::Ago(hours * 60)
}

/// `minutes` before now.
pub const fn m(minutes: i64) -> When {
    When::Ago(minutes)
}

/// `days` working days ago, at `hh:mm` local time.
pub const fn d(days: u32, hh: i64, mm: i64) -> When {
    When::Day(days, hh * 60 + mm)
}

/// `days` working days from today, at `hh:mm` local time.
pub const fn ahead(days: u32, hh: i64, mm: i64) -> When {
    When::Ahead(days, hh * 60 + mm)
}

impl When {
    /// The same moment `minutes` later.
    pub const fn plus(self, minutes: i64) -> When {
        match self {
            When::Ago(ago) => When::Ago(ago - minutes),
            When::Day(n, at) => When::Day(n, at + minutes),
            When::Ahead(n, at) => When::Ahead(n, at + minutes),
        }
    }
}

/// The demo's "now", fixed when it starts so that every page reads the same
/// dates.
pub struct Clock {
    pub now: DateTime<Utc>,
}

static CLOCK: OnceLock<Clock> = OnceLock::new();

pub fn clock() -> &'static Clock {
    CLOCK.get_or_init(|| {
        let now = Utc::now();
        Clock {
            now: now - Duration::seconds(i64::from(now.timestamp() as u32 % 60)),
        }
    })
}

fn weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

impl Clock {
    pub fn at(&self, when: When) -> DateTime<Utc> {
        match when {
            When::Ago(minutes) => self.now - Duration::minutes(minutes),
            When::Day(n, minutes) => self.on(self.workday(-i64::from(n)), minutes),
            When::Ahead(n, minutes) => self.on(self.workday(i64::from(n)), minutes),
        }
    }

    /// The working day `offset` working days away from today.
    fn workday(&self, offset: i64) -> NaiveDate {
        let mut date = self.now.with_timezone(&Local).date_naive();
        let mut left = offset.abs();
        while left > 0 {
            date += Duration::days(offset.signum());
            if !weekend(date) {
                left -= 1;
            }
        }
        date
    }

    fn on(&self, date: NaiveDate, minutes: i64) -> DateTime<Utc> {
        let midnight = date.and_hms_opt(0, 0, 0).expect("midnight exists");
        let local = Local
            .from_local_datetime(&midnight)
            .earliest()
            .unwrap_or_else(|| Local.from_utc_datetime(&midnight));
        local.with_timezone(&Utc) + Duration::minutes(minutes)
    }
}

/// GitLab's timestamp format.
pub fn iso(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S.000Z").to_string()
}

/// A local calendar date, `YYYY-MM-DD`.
pub fn date(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

/// A stable hash of `parts`, for the SHAs and ids the demo makes up.
pub fn hash(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes().chain([0xff]) {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

/// A 40-character commit SHA, stable for `parts`.
pub fn sha(parts: &[&str]) -> String {
    let mut out = String::with_capacity(48);
    for seed in ["a", "b", "c"] {
        let mut keyed: Vec<&str> = parts.to_vec();
        keyed.push(seed);
        out.push_str(&format!("{:016x}", hash(&keyed)));
    }
    out.truncate(40);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn working_days_skip_the_weekends() {
        let clock = clock();
        for n in 1..12 {
            let day = clock.at(d(n, 10, 0)).with_timezone(&Local);
            assert!(!weekend(day.date_naive()), "{day}");
            assert!(day < clock.now.with_timezone(&Local));
        }
        assert!(clock.at(ahead(1, 9, 0)) > clock.now);
        assert!(clock.at(d(2, 9, 0)) < clock.at(d(1, 9, 0)));
    }

    #[test]
    fn shas_are_stable_and_distinct() {
        assert_eq!(sha(&["x"]), sha(&["x"]));
        assert_ne!(sha(&["x"]), sha(&["y"]));
        assert_eq!(sha(&["x"]).len(), 40);
    }
}
