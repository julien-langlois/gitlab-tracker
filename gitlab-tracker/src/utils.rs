use std::collections::HashSet;

pub const RELEVANCE_THRESHOLD: f64 = 0.70;

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
