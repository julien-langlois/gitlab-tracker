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
