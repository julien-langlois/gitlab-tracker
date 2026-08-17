use std::collections::HashSet;

use chrono::{DateTime, Utc};

pub const RELEVANCE_THRESHOLD: f64 = 0.70;

/// Formats an ISO 8601 timestamp string into a human-readable relative date label.
///
/// Returns labels such as "à l'instant", "il y a 5 min", "Hier", "Il y a 3 jours", etc.
/// Falls back to a compact absolute date ("2024-06-01 14:32") when the timestamp
/// cannot be parsed or when the difference exceeds 30 days.
pub fn format_relative_date(iso: &str) -> String {
    let Ok(dt) = iso.parse::<DateTime<Utc>>() else {
        // Graceful fallback: show a compact absolute date (drop sub-seconds and TZ).
        return iso.get(..19).unwrap_or(iso).replace('T', " ");
    };

    let now = Utc::now();
    let diff = now.signed_duration_since(dt);
    let secs = diff.num_seconds();
    let minutes = diff.num_minutes();
    let hours = diff.num_hours();
    let days = diff.num_days();

    // Handle future dates (e.g. milestone due dates in the future).
    if secs < 0 {
        let future_diff = dt.signed_duration_since(now);
        let future_days = future_diff.num_days();
        let future_hours = future_diff.num_hours();
        let future_minutes = future_diff.num_minutes();
        let future_secs = future_diff.num_seconds();

        let future_months = future_days / 30;
        let future_years = future_days / 365;

        return match (future_secs, future_minutes, future_hours, future_days) {
            (s, _, _, _) if s < 60 => "just now".to_string(),
            (_, m, _, _) if m < 60 => format!("in {} min", m),
            (_, _, h, _) if h < 24 => format!("in {}h", h),
            (_, _, _, 1) => "tomorrow".to_string(),
            (_, _, _, d) if d < 7 => format!("in {} days", d),
            (_, _, _, d) if d < 14 => "next week".to_string(),
            (_, _, _, d) if d < 30 => format!("in {} weeks", d / 7),
            (_, _, _, d) if d < 60 => "in about a month".to_string(),
            _ if future_years >= 1 => format!(
                "in {} year{}",
                future_years,
                if future_years > 1 { "s" } else { "" }
            ),
            _ => format!("in {} months", future_months),
        };
    }

    let months = days / 30;
    let years = days / 365;

    match (secs, minutes, hours, days) {
        (s, _, _, _) if s < 60 => "just now".to_string(),
        (_, m, _, _) if m < 60 => format!("{} min ago", m),
        (_, _, h, _) if h < 24 => format!("{}h ago", h),
        (_, _, _, 1) => "yesterday".to_string(),
        (_, _, _, d) if d < 7 => format!("{} days ago", d),
        (_, _, _, d) if d < 14 => "last week".to_string(),
        (_, _, _, d) if d < 30 => format!("{} weeks ago", d / 7),
        (_, _, _, d) if d < 60 => "about a month ago".to_string(),
        _ if years >= 1 => format!("{} year{} ago", years, if years > 1 { "s" } else { "" }),
        _ => format!("{} months ago", months),
    }
}

pub fn calculate_relevance(mr_title: &str, commit_msg: &str) -> f64 {
    let extract_keywords = |text: &str| -> HashSet<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphabetic())
            .map(|s| s.to_string())
            .filter(|s| s.len() > 2 && s != "feat" && s != "fix" && s != "refactor" && s != "chore")
            .collect()
    };
    let mr_words = extract_keywords(mr_title);
    let commit_words = extract_keywords(commit_msg);
    if mr_words.is_empty() || commit_words.is_empty() {
        return 0.0;
    }
    let common_words = mr_words.intersection(&commit_words).count();
    let total_keywords_to_match = mr_words.len().min(commit_words.len());
    if total_keywords_to_match == 0 {
        return 0.0;
    }
    (common_words as f64) / (total_keywords_to_match as f64)
}

/// Parses a human-readable duration string into a number of hours (f32).
///
/// Accepted formats (case-insensitive):
/// - `"1h30"`, `"1h30m"` → 1.5
/// - `"90m"`, `"90"` (bare number treated as minutes) → 1.5
/// - `"1.5h"`, `"1,5h"` → 1.5
/// - `"2h"` → 2.0
///
/// Returns `Err` with a human-readable message when the format is not recognised
/// or when the result is zero / negative.
pub fn parse_duration_to_hours(input: &str) -> Result<f32, String> {
    let s = input.trim().to_lowercase().replace(',', ".");

    // Pattern: "1h30m" or "1h30" — hours and optional minutes
    if let Some(h_pos) = s.find('h') {
        let hours_part = &s[..h_pos];
        let minutes_part = s[h_pos + 1..].trim_end_matches('m');

        let hours: f32 = hours_part
            .parse()
            .map_err(|_| format!("Invalid hours in \"{}\"", input))?;

        let minutes: f32 = if minutes_part.is_empty() {
            0.0
        } else {
            minutes_part
                .parse()
                .map_err(|_| format!("Invalid minutes in \"{}\"", input))?
        };

        let total = hours + minutes / 60.0;
        if total <= 0.0 {
            return Err("Duration must be greater than zero".into());
        }
        return Ok(total);
    }

    // Pattern: "90m" — plain minutes
    if let Some(stripped) = s.strip_suffix('m') {
        let minutes: f32 = stripped
            .parse()
            .map_err(|_| format!("Invalid minutes in \"{}\"", input))?;
        let total = minutes / 60.0;
        if total <= 0.0 {
            return Err("Duration must be greater than zero".into());
        }
        return Ok(total);
    }

    // Pattern: bare number — treated as minutes
    if let Ok(minutes) = s.parse::<f32>() {
        if minutes <= 0.0 {
            return Err("Duration must be greater than zero".into());
        }
        return Ok(minutes / 60.0);
    }

    Err(format!(
        "Unrecognised format \"{}\". Try: 1h30, 90m, 1.5h",
        input
    ))
}
