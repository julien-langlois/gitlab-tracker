use regex::Regex;

/// Attempts to extract a Redmine ticket ID from the MR title and/or description.
///
/// Patterns are evaluated in order; the first match wins. The title is always
/// checked before the description so a reference in the title takes priority.
///
/// Each pattern must contain exactly one capture group that matches the raw
/// ticket ID string (numeric for Redmine, but kept as `String` for genericity).
///
/// Returns `None` when no pattern matches either source.
pub fn detect_ticket_id(title: &str, description: &str, patterns: &[String]) -> Option<String> {
    for source in [title, description] {
        for pattern in patterns {
            match Regex::new(pattern) {
                Ok(re) => {
                    if let Some(caps) = re.captures(source) {
                        if let Some(m) = caps.get(1) {
                            return Some(m.as_str().to_string());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(pattern = %pattern, error = %e, "Invalid ticket detection regex — skipped");
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_pattern() -> Vec<String> {
        vec![r"#(\d+)".to_string()]
    }

    #[test]
    fn detects_plain_hash_in_title() {
        assert_eq!(
            detect_ticket_id("fix: closes #1234", "", &hash_pattern()),
            Some("1234".to_string())
        );
    }

    #[test]
    fn prefers_title_over_description() {
        assert_eq!(
            detect_ticket_id("#10 my fix", "refs #20", &hash_pattern()),
            Some("10".to_string())
        );
    }

    #[test]
    fn falls_back_to_description() {
        assert_eq!(
            detect_ticket_id("my boring MR", "refs #42", &hash_pattern()),
            Some("42".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_match() {
        assert_eq!(
            detect_ticket_id("my boring MR", "no ticket here", &hash_pattern()),
            None
        );
    }

    #[test]
    fn skips_invalid_regex_gracefully() {
        let bad_patterns = vec!["[invalid".to_string(), r"#(\d+)".to_string()];
        // Should still return a match from the valid second pattern.
        assert_eq!(
            detect_ticket_id("#99", "", &bad_patterns),
            Some("99".to_string())
        );
    }
}
