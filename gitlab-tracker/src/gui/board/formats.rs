//! Dates, durations, percentages and names as the pages write them.

use super::*;

/// A GitLab timestamp (RFC 3339) as UTC.
pub fn parse_time(at: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// [`parse_time`] of an optional timestamp.
pub fn time_of(at: Option<&str>) -> Option<DateTime<Utc>> {
    at.and_then(parse_time)
}

/// Days (fractional) from `from` to `to`.
pub fn days_between(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    (to - from).num_minutes() as f64 / 1440.0
}

/// A short duration: "12 min", "3 h", "1.5 d", "12 d".
pub fn duration_text(days: f64) -> String {
    if days < 1.0 / 24.0 {
        let minutes = (days * 1440.0).round().max(1.0) as u64;
        tf("{n} min", &[("n", &minutes.to_string())])
    } else if days >= 365.0 {
        tf("{n} y", &[("n", &format!("{:.0}", (days / 365.0).floor()))])
    } else if days >= 60.0 {
        tf("{n} mo", &[("n", &format!("{:.0}", (days / 30.0).floor()))])
    } else {
        crate::next_step::days_text(days)
    }
}

/// "3 h ago"-like: [`duration_text`] of the time since `at`.
pub fn ago(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    tf(
        "{time} ago",
        &[("time", &duration_text(days_between(at, now).max(0.0)))],
    )
}

/// A whole number with a `%` sign.
pub fn percent(fraction: f64) -> String {
    format!("{:.0} %", fraction * 100.0)
}

/// A duration in seconds as "7 min 40" or "48 s".
pub fn seconds_text(seconds: f64) -> String {
    let s = seconds.round() as u64;
    if s < 60 {
        tf("{n} s", &[("n", &s.to_string())])
    } else {
        tf(
            "{m} min {s}",
            &[
                ("m", &(s / 60).to_string()),
                ("s", &format!("{:02}", s % 60)),
            ],
        )
    }
}

/// "Open in GitLab" for the URL, else nothing.
pub fn open_url(ui: &Ui, url: &str) {
    if !url.is_empty() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

/// "a person" → their first name: "Thomas Dubosc (@thomas_db)" → "Thomas".
pub fn first_name(display: &str) -> &str {
    crate::next_step::first_name(display)
}

pub use gitlab_tracker_gitlab::is_bot;

/// A muted caption under a figure or a list, small.
pub fn caption(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.add(egui::Label::new(muted(theme, text).size(theme.text_size - 1.5)).wrap());
}

/// Translated "—" placeholder for a missing value.
pub fn dash() -> &'static str {
    tl("—")
}
