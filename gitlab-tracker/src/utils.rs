use chrono::{DateTime, Utc};

/// Returns `true` when a GitLab display string belongs to the configured username.
///
/// GitLab user fields are rendered as `Full Name (@username)` in this application.
/// Matching on `@username` keeps the check stable when the display name changes.
pub fn matches_gitlab_username(display: &str, username: &str) -> bool {
    display.contains(format!("@{}", username).as_str())
}

/// Computes a fuzzy match score between a `query` and a `haystack` string.
///
/// The algorithm looks for all query characters in order inside `haystack`
/// (subsequence match) and scores the result based on three criteria:
///
/// 1. **Exact substring bonus** — the full query appears verbatim → highest score.
/// 2. **Prefix bonus** — every matched character is immediately followed by the next
///    one (consecutive run starting early in `haystack`).
/// 3. **Proximity penalty** — gaps between matched characters reduce the score.
///
/// Returns `None` when `query` is empty (caller should treat as "always matches")
/// or when no subsequence match is found (caller should treat as "no match").
/// Returns `Some(score)` in `[0.0, 1.0]` otherwise — higher is more relevant.
pub fn fuzzy_score(query: &str, haystack: &str) -> Option<f64> {
    if query.is_empty() {
        return None; // Caller decides: empty query always matches.
    }

    let q = query.to_lowercase();
    let h = haystack.to_lowercase();

    // Fast path: exact substring → maximum relevance.
    if h.contains(q.as_str()) {
        // Boost score based on how early the match appears (earlier = more relevant).
        let pos = h.find(q.as_str()).unwrap_or(0);
        let position_bonus = 1.0 - (pos as f64 / h.len() as f64).min(1.0);
        return Some(0.8 + 0.2 * position_bonus);
    }

    // Subsequence match: find each query char in order inside haystack.
    let q_chars: Vec<char> = q.chars().collect();
    let h_chars: Vec<char> = h.chars().collect();

    let mut q_idx = 0;
    let mut match_positions: Vec<usize> = Vec::with_capacity(q_chars.len());

    for (h_idx, &hc) in h_chars.iter().enumerate() {
        if q_idx < q_chars.len() && hc == q_chars[q_idx] {
            match_positions.push(h_idx);
            q_idx += 1;
        }
    }

    // All query characters must be present as a subsequence.
    if q_idx < q_chars.len() {
        return None;
    }

    // Score: reward consecutive runs, penalise large gaps.
    let total_span = match_positions.last().unwrap() - match_positions.first().unwrap() + 1;
    let matched = match_positions.len() as f64;
    let span = total_span as f64;

    // Compactness: ratio of matched chars to the span they occupy.
    let compactness = matched / span;

    // Position bonus: earlier first match → higher relevance.
    let first_pos = *match_positions.first().unwrap() as f64;
    let position_bonus = 1.0 - (first_pos / h_chars.len() as f64).min(1.0);

    // Combine: compactness weighted 70%, position 30%, capped at 0.79
    // so subsequence matches always rank below exact substring matches.
    let score = (0.7 * compactness + 0.3 * position_bonus) * 0.79;
    Some(score.clamp(0.0, 0.79))
}

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
