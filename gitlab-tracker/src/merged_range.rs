//! The merge days a list is narrowed to: the "Merged between" filter's value,
//! written `2026-11-16..2026-11-22` (either end may be left out, a single day
//! stands for itself), and the periods the cockpit counts merges over.
//!
//! Days are the user's, in local time: a merge at 23:30 UTC is tomorrow in
//! UTC+2, as the stats count it.

// The filter reads the range in every build; the periods and the text are the
// window's (its sidebar, the cockpit's counts).
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

use chrono::{Datelike, Duration, Local, NaiveDate};

/// The days from `from` to `to`, both included; an end left out is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergedRange {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

const FORMAT: &str = "%Y-%m-%d";

impl MergedRange {
    /// Reads `2026-11-16..2026-11-22`, `2026-11-16..`, `..2026-11-22` or
    /// `2026-11-20`; `None` when it is none of them (still being typed).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let day = |s: &str| -> Option<Option<NaiveDate>> {
            let s = s.trim();
            if s.is_empty() {
                Some(None)
            } else {
                NaiveDate::parse_from_str(s, FORMAT).ok().map(Some)
            }
        };
        let range = match text.split_once("..") {
            Some((from, to)) => Self {
                from: day(from)?,
                to: day(to)?,
            },
            None => {
                let only = day(text)??;
                Self {
                    from: Some(only),
                    to: Some(only),
                }
            }
        };
        (range.from.is_some() || range.to.is_some()).then_some(range)
    }

    /// The filter's value: what [`MergedRange::parse`] reads back.
    pub fn query(&self) -> String {
        let day =
            |d: Option<NaiveDate>| d.map(|d| d.format(FORMAT).to_string()).unwrap_or_default();
        format!("{}..{}", day(self.from), day(self.to))
    }

    pub fn contains(&self, day: NaiveDate) -> bool {
        self.from.is_none_or(|from| day >= from) && self.to.is_none_or(|to| day <= to)
    }

    /// The days in words: the cockpit's period they are ("This week"), else
    /// "16/11/2026 → 22/11/2026", "since 16/11/2026", "until 22/11/2026".
    pub fn describe(&self, today: NaiveDate) -> String {
        use crate::i18n::{tf, tl};
        if let Some(period) = Period::of(self, today) {
            return tl(period.label()).to_string();
        }
        let day = |d: NaiveDate| d.format("%d/%m/%Y").to_string();
        match (self.from, self.to) {
            (Some(from), Some(to)) if from == to => day(from),
            (Some(from), Some(to)) => format!("{} → {}", day(from), day(to)),
            (Some(from), None) => tf("since {day}", &[("day", &day(from))]),
            (None, Some(to)) => tf("until {day}", &[("day", &day(to))]),
            (None, None) => String::new(),
        }
    }

    /// Whether GitLab's `merged_at` falls on one of the days; `false` without one.
    pub fn contains_at(&self, merged_at: Option<&str>) -> bool {
        merge_day(merged_at).is_some_and(|day| self.contains(day))
    }
}

/// The local day of GitLab's `merged_at`.
pub fn merge_day(merged_at: Option<&str>) -> Option<NaiveDate> {
    merged_at
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.with_timezone(&Local).date_naive())
}

/// The periods the cockpit's Flow card counts merges over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    /// Monday to Sunday (ISO week).
    ThisWeek,
    ThisMonth,
    /// Today and the 6 days before.
    Last7Days,
    /// Today and the 29 days before.
    Last30Days,
}

impl Period {
    pub const ALL: [Period; 5] = [
        Period::Today,
        Period::ThisWeek,
        Period::ThisMonth,
        Period::Last7Days,
        Period::Last30Days,
    ];

    /// Its days, seen from `today`.
    pub fn range(self, today: NaiveDate) -> MergedRange {
        let back = |days: i64| Some(today - Duration::days(days));
        let (from, to) = match self {
            Period::Today => (Some(today), Some(today)),
            Period::ThisWeek => {
                let monday = back(i64::from(today.weekday().num_days_from_monday()));
                (monday, monday.map(|m| m + Duration::days(6)))
            }
            Period::ThisMonth => {
                let first = today.with_day(1);
                let next = first.and_then(|f| {
                    if f.month() == 12 {
                        NaiveDate::from_ymd_opt(f.year() + 1, 1, 1)
                    } else {
                        NaiveDate::from_ymd_opt(f.year(), f.month() + 1, 1)
                    }
                });
                (first, next.map(|n| n - Duration::days(1)))
            }
            Period::Last7Days => (back(6), Some(today)),
            Period::Last30Days => (back(29), Some(today)),
        };
        MergedRange { from, to }
    }

    /// The period whose days are `range`, seen from `today`.
    pub fn of(range: &MergedRange, today: NaiveDate) -> Option<Period> {
        Self::ALL.into_iter().find(|p| p.range(today) == *range)
    }

    /// Its name (English: the window translates it).
    pub fn label(self) -> &'static str {
        match self {
            Period::Today => "Today",
            Period::ThisWeek => "This week",
            Period::ThisMonth => "This month",
            Period::Last7Days => "Last 7 days",
            Period::Last30Days => "Last 30 days",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, FORMAT).unwrap()
    }

    #[test]
    fn reads_what_it_writes() {
        for text in ["2026-11-16..2026-11-22", "2026-11-16..", "..2026-11-22"] {
            assert_eq!(MergedRange::parse(text).unwrap().query(), text);
        }
        let day = MergedRange::parse(" 2026-11-20 ").unwrap();
        assert_eq!(
            (day.from, day.to),
            (Some(d("2026-11-20")), Some(d("2026-11-20")))
        );
        for text in ["", "..", "2026-11", "2026-11-16..soon", "yesterday"] {
            assert_eq!(MergedRange::parse(text), None, "{text}");
        }
    }

    #[test]
    fn both_ends_are_in() {
        let range = MergedRange::parse("2026-11-16..2026-11-22").unwrap();
        assert!(range.contains(d("2026-11-16")) && range.contains(d("2026-11-22")));
        assert!(!range.contains(d("2026-11-15")) && !range.contains(d("2026-11-23")));
        let open = MergedRange::parse("..2026-11-22").unwrap();
        assert!(open.contains(d("2000-01-01")));
        assert!(!range.contains_at(None));
    }

    #[test]
    fn the_periods_follow_the_calendar() {
        let monday = d("2026-11-23");
        let q = |p: Period, today| p.range(today).query();
        assert_eq!(q(Period::Today, monday), "2026-11-23..2026-11-23");
        assert_eq!(q(Period::ThisWeek, monday), "2026-11-23..2026-11-29");
        assert_eq!(
            q(Period::ThisWeek, d("2026-11-29")),
            "2026-11-23..2026-11-29"
        );
        assert_eq!(q(Period::ThisMonth, monday), "2026-11-01..2026-11-30");
        assert_eq!(
            q(Period::ThisMonth, d("2026-12-31")),
            "2026-12-01..2026-12-31"
        );
        assert_eq!(q(Period::Last7Days, monday), "2026-11-17..2026-11-23");
        assert_eq!(q(Period::Last30Days, monday), "2026-10-25..2026-11-23");
        for p in Period::ALL {
            assert_eq!(Period::of(&p.range(monday), monday), Some(p));
        }
    }
}
