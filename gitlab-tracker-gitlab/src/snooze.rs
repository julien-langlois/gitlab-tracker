//! Snoozing an MR: it leaves Today (and the rail's badge) until a moment, or
//! until it changes, and comes back early when it moves — a push, a failed
//! pipeline, a new comment, or when it is merged or closed.
//!
//! Time zones: the moments offered ("tomorrow morning", "Monday") are worked out
//! in the machine's zone, with its daylight saving rules, then kept as an
//! instant in UTC. A snooze set in Paris for "Monday 09:00" is the same instant
//! after a clock change, or on a machine moved to another zone (where it shows
//! in that zone's time). "In 1 hour" is an hour of real time, whatever the
//! clocks do in between.

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, SecondsFormat, TimeZone, Utc, Weekday,
};
use serde::{Deserialize, Serialize};

use crate::i18n::{tc, tf, tl};
use crate::models::{GitlabMrState, PipelineState, TrackedMr};

/// The local hour a "morning" snooze wakes at.
pub const MORNING_HOUR: u32 = 9;

/// What the snooze menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Until {
    /// An hour from now.
    Hour,
    /// Tomorrow at [`MORNING_HOUR`], local time.
    Tomorrow,
    /// The next Monday at [`MORNING_HOUR`], local time.
    NextWeek,
    /// No time: only a change wakes it.
    Change,
}

impl Until {
    pub const ALL: [Until; 4] = [Until::Hour, Until::Tomorrow, Until::NextWeek, Until::Change];

    pub fn label(self) -> &'static str {
        match self {
            Until::Hour => tl("In 1 hour"),
            Until::Tomorrow => tl("Tomorrow morning"),
            Until::NextWeek => tl("Next Monday"),
            Until::Change => tl("Until it changes"),
        }
    }

    /// The instant it wakes, from `now` in its zone; `None` for [`Until::Change`].
    pub fn resolve<Tz: TimeZone>(self, now: &DateTime<Tz>) -> Option<DateTime<Utc>> {
        let zone = now.timezone();
        let today = now.date_naive();
        match self {
            Until::Hour => Some(now.with_timezone(&Utc) + Duration::hours(1)),
            Until::Tomorrow => Some(local_at(&zone, today + Duration::days(1), MORNING_HOUR)),
            Until::NextWeek => {
                // Strictly after today: on a Monday, the next one.
                let days = 7 - i64::from(today.weekday().num_days_from_monday());
                Some(local_at(&zone, today + Duration::days(days), MORNING_HOUR))
            }
            Until::Change => None,
        }
    }
}

/// `hour`:00 on `date` in `zone`, as an instant. An hour skipped by a clock
/// change (spring forward) moves to the first valid time after it; an hour
/// that happens twice (fall back) takes the first.
pub fn local_at<Tz: TimeZone>(zone: &Tz, date: NaiveDate, hour: u32) -> DateTime<Utc> {
    let mut at = date
        .and_hms_opt(hour.min(23), 0, 0)
        .expect("an hour of the day exists");
    // No zone skips more than a few hours; look a day ahead at most.
    for _ in 0..96 {
        match zone.from_local_datetime(&at) {
            LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => {
                return t.with_timezone(&Utc);
            }
            LocalResult::None => at += Duration::minutes(15),
        }
    }
    Utc.from_utc_datetime(&at)
}

/// A snoozed MR: when it wakes, and what it looked like then, so that a change
/// wakes it early.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Snooze {
    /// The instant it wakes, RFC 3339 in UTC; none: only a change wakes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// The MR's head commit when snoozed: a push wakes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    /// Its comments when snoozed: a new one wakes it.
    #[serde(default)]
    pub notes: u32,
    /// Its latest pipeline when snoozed: a newer one that fails wakes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<u64>,
}

/// Why a snooze ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    Time,
    Push,
    Comment,
    PipelineFailed,
    Closed,
}

impl Wake {
    pub fn text(self) -> &'static str {
        match self {
            Wake::Time => tl("its snooze ended"),
            Wake::Push => tl("new commits"),
            Wake::Comment => tl("a new comment"),
            Wake::PipelineFailed => tl("its pipeline failed"),
            Wake::Closed => tl("merged or closed"),
        }
    }
}

impl Snooze {
    /// Snoozes `mr` as it is now, until `until`.
    pub fn new(mr: &TrackedMr, until: Option<DateTime<Utc>>) -> Self {
        Self {
            until: until.map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
            sha: mr.sha.clone(),
            notes: mr.user_notes_count,
            pipeline: mr.pipelines.first().map(|p| p.id),
        }
    }

    /// The instant it wakes, if it has one (an unreadable one wakes it now).
    pub fn until(&self) -> Option<DateTime<Utc>> {
        self.until.as_deref().map(|at| {
            DateTime::parse_from_rfc3339(at)
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or(DateTime::<Utc>::MIN_UTC)
        })
    }

    /// Whether, and why, `mr` should wake at `now`.
    pub fn wake(&self, mr: &TrackedMr, now: DateTime<Utc>) -> Option<Wake> {
        // Not loaded yet: nothing to compare with.
        if mr.title.is_empty() || mr.author == "Loading" {
            return None;
        }
        if mr.state != GitlabMrState::Opened {
            return Some(Wake::Closed);
        }
        if self.sha.is_some() && mr.sha.is_some() && mr.sha != self.sha {
            return Some(Wake::Push);
        }
        if let Some(p) = mr.pipelines.first() {
            if Some(p.id) != self.pipeline && p.status == PipelineState::Failed {
                return Some(Wake::PipelineFailed);
            }
        }
        if mr.user_notes_count > self.notes {
            return Some(Wake::Comment);
        }
        if self.until().is_some_and(|at| at <= now) {
            return Some(Wake::Time);
        }
        None
    }

    /// "until 11:42", "until tomorrow 09:00", "until Mon 09:00", "until it
    /// changes": the wake-up in `now`'s zone.
    pub fn text<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        match self.until() {
            Some(at) => tf("until {when}", &[("when", &when_text(at, now))]),
            None => tl("until it changes").to_string(),
        }
    }
}

/// `at` as `now`'s zone reads it, as short as it stays clear: the time today,
/// "tomorrow" and the time, the weekday within a week, else the date.
pub fn when_text<Tz: TimeZone>(at: DateTime<Utc>, now: &DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = at.with_timezone(&now.timezone());
    let time = local.format("%H:%M").to_string();
    let days = (local.date_naive() - now.date_naive()).num_days();
    match days {
        0 => time,
        1 => tf("tomorrow {time}", &[("time", &time)]),
        2..=6 => format!("{} {time}", weekday_short(local.weekday())),
        _ => format!("{} {time}", local.format("%d/%m")),
    }
}

pub fn weekday_short(day: Weekday) -> &'static str {
    tc(
        "date",
        match day {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        },
    )
}

/// Ends the snoozes of `mrs` that should wake at `now`: the MRs woken, with why.
pub fn wake_due(mrs: &mut [TrackedMr], now: DateTime<Utc>) -> Vec<(String, String, Wake)> {
    let mut woken = Vec::new();
    for mr in mrs.iter_mut() {
        let Some(why) = mr.snooze.as_ref().and_then(|s| s.wake(mr, now)) else {
            continue;
        };
        mr.snooze = None;
        woken.push((mr.id.clone(), mr.title.clone(), why));
    }
    woken
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Pipeline;
    use chrono::FixedOffset;
    use chrono_tz::Europe::Paris;
    use chrono_tz::{America::New_York, Pacific::Auckland};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn open_mr() -> TrackedMr {
        let mut mr = TrackedMr::placeholder("7".into(), "Fix login".into(), None);
        mr.data.author = "Thomas <@thomas_db>".into();
        mr.data.state = GitlabMrState::Opened;
        mr.data.sha = Some("aaa".into());
        mr.data.user_notes_count = 2;
        mr.data.pipelines = vec![Pipeline {
            id: 10,
            status: PipelineState::Success,
            created_at: None,
            updated_at: None,
            sha: Some("aaa".into()),
            jobs: vec![],
            needs_read: false,
        }];
        mr
    }

    #[test]
    fn tomorrow_morning_is_nine_local_on_both_sides_of_a_clock_change() {
        // Saturday 28 March 2026, 22:00 in Paris (UTC+1); the clocks go
        // forward that night: Sunday 09:00 is UTC+2, so 07:00 UTC.
        let now = Paris.with_ymd_and_hms(2026, 3, 28, 22, 0, 0).unwrap();
        assert_eq!(
            Until::Tomorrow.resolve(&now),
            Some(utc("2026-03-29T07:00:00Z"))
        );
        // The night they go back (25 October 2026): 09:00 is UTC+1 again.
        let now = Paris.with_ymd_and_hms(2026, 10, 24, 22, 0, 0).unwrap();
        assert_eq!(
            Until::Tomorrow.resolve(&now),
            Some(utc("2026-10-25T08:00:00Z"))
        );
    }

    #[test]
    fn an_hour_is_an_hour_of_real_time_across_a_clock_change() {
        // 01:30 in Paris the night clocks go forward: an hour later reads 03:30.
        let now = Paris.with_ymd_and_hms(2026, 3, 29, 1, 30, 0).unwrap();
        let at = Until::Hour.resolve(&now).unwrap();
        assert_eq!(at - now.with_timezone(&Utc), Duration::hours(1));
        assert_eq!(
            at.with_timezone(&Paris).format("%H:%M").to_string(),
            "03:30"
        );
    }

    #[test]
    fn a_skipped_hour_moves_forward_and_a_doubled_one_takes_the_first() {
        // 02:00 does not exist in Paris on 29 March 2026: 03:00 (UTC+2).
        let date = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        assert_eq!(local_at(&Paris, date, 2), utc("2026-03-29T01:00:00Z"));
        // 02:00 happens twice on 25 October 2026: the first, still UTC+2.
        let date = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        assert_eq!(local_at(&Paris, date, 2), utc("2026-10-25T00:00:00Z"));
    }

    #[test]
    fn next_monday_is_strictly_after_today_in_the_local_calendar() {
        // Monday 16 November 2026 at 08:00 in Paris: the Monday after.
        let now = Paris.with_ymd_and_hms(2026, 11, 16, 8, 0, 0).unwrap();
        assert_eq!(
            Until::NextWeek.resolve(&now),
            Some(utc("2026-11-23T08:00:00Z"))
        );
        // Sunday 23:30 in Auckland is already Sunday 10:30 UTC: the local
        // calendar decides, so the next morning (Monday) is the answer.
        let now = Auckland.with_ymd_and_hms(2026, 11, 22, 23, 30, 0).unwrap();
        let at = Until::NextWeek.resolve(&now).unwrap();
        assert_eq!(
            at.with_timezone(&Auckland)
                .format("%a %d %H:%M")
                .to_string(),
            "Mon 23 09:00"
        );
        // Sunday 20:00 in New York is Monday 01:00 UTC: still Monday 23
        // locally, not the one after.
        let now = New_York.with_ymd_and_hms(2026, 11, 22, 20, 0, 0).unwrap();
        let at = Until::NextWeek.resolve(&now).unwrap();
        assert_eq!(
            at.with_timezone(&New_York)
                .format("%a %d %H:%M")
                .to_string(),
            "Mon 23 09:00"
        );
    }

    #[test]
    fn the_wake_up_reads_in_the_zone_of_the_machine_it_is_shown_on() {
        let mr = open_mr();
        let snooze = Snooze::new(&mr, Some(utc("2026-11-18T08:00:00Z")));
        // Stored in UTC, whatever the zone it was set in.
        assert_eq!(snooze.until.as_deref(), Some("2026-11-18T08:00:00Z"));
        let paris = Paris.with_ymd_and_hms(2026, 11, 17, 15, 0, 0).unwrap();
        assert_eq!(when_text(snooze.until().unwrap(), &paris), "tomorrow 09:00");
        // The same instant on a machine in New York: 03:00 the next day.
        let ny = paris.with_timezone(&New_York);
        assert_eq!(when_text(snooze.until().unwrap(), &ny), "tomorrow 03:00");
        // A fixed offset works the same.
        let plus5 = paris.with_timezone(&FixedOffset::east_opt(5 * 3600).unwrap());
        assert_eq!(when_text(snooze.until().unwrap(), &plus5), "tomorrow 13:00");
    }

    #[test]
    fn a_change_wakes_it_before_its_time() {
        let now = utc("2026-11-17T10:00:00Z");
        let later = Some(utc("2026-11-20T08:00:00Z"));
        let base = open_mr();
        let snooze = Snooze::new(&base, later);
        assert_eq!(snooze.wake(&base, now), None);

        let mut mr = base.clone();
        mr.data.sha = Some("bbb".into());
        assert_eq!(snooze.wake(&mr, now), Some(Wake::Push));

        let mut mr = base.clone();
        mr.data.user_notes_count = 3;
        assert_eq!(snooze.wake(&mr, now), Some(Wake::Comment));

        // A newer pipeline wakes it only if it fails.
        let mut mr = base.clone();
        mr.data.pipelines[0].id = 11;
        assert_eq!(snooze.wake(&mr, now), None);
        mr.data.pipelines[0].status = PipelineState::Failed;
        assert_eq!(snooze.wake(&mr, now), Some(Wake::PipelineFailed));

        let mut mr = base.clone();
        mr.data.state = GitlabMrState::Merged;
        assert_eq!(snooze.wake(&mr, now), Some(Wake::Closed));

        // Its time comes.
        assert_eq!(
            snooze.wake(&base, utc("2026-11-20T08:00:00Z")),
            Some(Wake::Time)
        );
        // Until it changes: no time ever wakes it.
        let forever = Snooze::new(&base, None);
        assert_eq!(forever.wake(&base, utc("2030-01-01T00:00:00Z")), None);
    }

    #[test]
    fn wake_due_clears_the_snoozes_that_end() {
        let mut a = open_mr();
        a.snooze = Some(Snooze::new(&a, Some(utc("2026-11-17T09:00:00Z"))));
        let mut b = open_mr();
        b.id = "8".into();
        b.snooze = Some(Snooze::new(&b, None));
        let mut mrs = vec![a, b];
        let woken = wake_due(&mut mrs, utc("2026-11-17T10:00:00Z"));
        assert_eq!(woken.len(), 1);
        assert_eq!((woken[0].0.as_str(), woken[0].2), ("7", Wake::Time));
        assert!(mrs[0].snooze.is_none() && mrs[1].snooze.is_some());
    }

    #[test]
    fn an_old_state_file_without_snoozes_still_reads() {
        let snooze: Snooze = serde_json::from_str(r#"{"until":"2026-11-18T08:00:00Z"}"#).unwrap();
        assert_eq!(snooze.notes, 0);
        assert!(snooze.sha.is_none());
    }
}
