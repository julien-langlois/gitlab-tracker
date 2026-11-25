//! Where the copied texts point: each MR to its GitLab page, each ticket to the tracker's, so that pasted ids can be clicked.

/// Where the copied texts point: each MR to its GitLab page, each ticket to
/// the tracker's, so that `!123` and `#4567` can be clicked once pasted (a chat
/// that does not render Markdown still turns the URL in it into a link).
pub struct Links {
    pub(super) mrs: std::collections::HashMap<String, String>,
    /// `https://gitlab/group/project/-/merge_requests/`, from any tracked MR:
    /// for the ids that are not tracked.
    pub(super) prefix: Option<String>,
    pub(super) tickets: std::collections::HashMap<String, String>,
    /// Each tracked MR's ticket, for the texts that name it next to the MR.
    pub(super) mr_tickets: std::collections::HashMap<String, String>,
}

impl Links {
    pub fn of(app: &crate::app::App) -> Self {
        let mut mrs = std::collections::HashMap::new();
        let mut prefix = None;
        let mut tickets = std::collections::HashMap::new();
        let mut mr_tickets = std::collections::HashMap::new();
        for mr in app.mrs.iter().filter(|mr| !mr.web_url.is_empty()) {
            mrs.insert(mr.id.clone(), mr.web_url.clone());
            if prefix.is_none() {
                if let Some((base, _)) = mr.web_url.split_once("/-/merge_requests/") {
                    prefix = Some(format!("{base}/-/merge_requests/"));
                }
            }
            // The tickets the MRs name (only those: a `#` number in a title
            // is not always a ticket).
            if let Some(tracker) = &app.tracker {
                let id = mr
                    .linked_ticket
                    .as_ref()
                    .map(|t| t.id.clone())
                    .or_else(|| tracker.detect_ticket_id(&mr.title, &mr.description));
                if let Some(id) = id {
                    let url = mr
                        .linked_ticket
                        .as_ref()
                        .filter(|t| t.id == id && !t.url.is_empty())
                        .map(|t| t.url.clone())
                        .unwrap_or_else(|| tracker.ticket_url(&id));
                    mr_tickets.insert(mr.id.clone(), id.clone());
                    tickets.insert(id, url);
                }
            }
        }
        Self {
            mrs,
            prefix,
            tickets,
            mr_tickets,
        }
    }

    pub(super) fn url(&self, sigil: char, id: &str) -> Option<String> {
        match sigil {
            '!' => self
                .mrs
                .get(id)
                .cloned()
                .or_else(|| self.prefix.as_ref().map(|p| format!("{p}{id}"))),
            _ => self.tickets.get(id).cloned(),
        }
    }

    /// `text` with each `!123` as `[!123](its MR)` and each known `#4567` as
    /// `[#4567](its ticket)`. The first time a line names an MR, its ticket follows
    /// it, `[!123](…) ([#4567](…))`, unless its line already names that
    /// ticket. What is already a link, or part of a word or a path, is left as
    /// it is.
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 64);
        for (n, line) in text.split('\n').enumerate() {
            if n > 0 {
                out.push('\n');
            }
            self.apply_line(line, &mut out);
        }
        out
    }

    pub(super) fn apply_line(&self, line: &str, out: &mut String) {
        let mut named = std::collections::HashSet::new();
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let before = i.checked_sub(1).map(|j| chars[j]);
            let starts = (c == '!' || c == '#')
                && !before.is_some_and(|b| {
                    b.is_alphanumeric() || matches!(b, '[' | '/' | '&' | '_' | '!' | '#')
                });
            if starts {
                let digits: String = chars[i + 1..]
                    .iter()
                    .take_while(|d| d.is_ascii_digit())
                    .collect();
                let end = i + 1 + digits.len();
                let after = chars.get(end);
                let whole = !after.is_some_and(|a| a.is_alphanumeric() || matches!(a, '_' | ']'));
                if !digits.is_empty() && whole {
                    if let Some(url) = self.url(c, &digits) {
                        out.push_str(&format!("[{c}{digits}]({url})"));
                        if c == '!' && named.insert(digits.clone()) {
                            if let Some(ticket) = self.mr_tickets.get(&digits) {
                                let mentioned = mentions(line, ticket);
                                if let (false, Some(url)) = (mentioned, self.tickets.get(ticket)) {
                                    out.push_str(&format!(" ([#{ticket}]({url}))"));
                                }
                            }
                        }
                        i = end;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        }
    }
}

/// Whether `line` names ticket `#id` as a whole number.
pub(super) fn mentions(line: &str, id: &str) -> bool {
    let reference = format!("#{id}");
    line.match_indices(&reference).any(|(at, _)| {
        !line[at + reference.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod links_tests {
    use super::Links;

    fn links() -> Links {
        Links {
            mrs: [(
                "12".to_string(),
                "https://gl/p/-/merge_requests/12".to_string(),
            )]
            .into(),
            prefix: Some("https://gl/p/-/merge_requests/".into()),
            tickets: [("4567".to_string(), "https://rm/issues/4567".to_string())].into(),
            mr_tickets: [("12".to_string(), "4567".to_string())].into(),
        }
    }

    #[test]
    fn mr_and_ticket_numbers_become_links() {
        let l = links();
        assert_eq!(
            l.apply("- !12 Fix #4567 (2 d), !99 × !12"),
            "- [!12](https://gl/p/-/merge_requests/12) Fix [#4567](https://rm/issues/4567) (2 d), \
             [!99](https://gl/p/-/merge_requests/99) × [!12](https://gl/p/-/merge_requests/12)"
        );
    }

    #[test]
    fn an_mr_brings_its_ticket_on_each_line_that_names_it() {
        let l = links();
        assert_eq!(
            l.apply("- !12 Fix the login, !12 again\n- !12"),
            "- [!12](https://gl/p/-/merge_requests/12) ([#4567](https://rm/issues/4567)) \
             Fix the login, [!12](https://gl/p/-/merge_requests/12) again\n\
             - [!12](https://gl/p/-/merge_requests/12) ([#4567](https://rm/issues/4567))"
        );
        // Not when its line names the ticket already.
        assert_eq!(
            l.apply("- Fix (!12) · #4567"),
            "- Fix ([!12](https://gl/p/-/merge_requests/12)) · [#4567](https://rm/issues/4567)"
        );
    }

    #[test]
    fn what_is_not_a_reference_is_left_alone() {
        let l = links();
        for text in [
            "[!12](https://gl/p/-/merge_requests/12)", // already a link
            "#0000 unknown ticket",                    // not a ticket the MRs name
            "page#4567 and a/!12 path",                // part of a word, a path
            "!12a, Hi!, ##4567, !!12",                 // not whole numbers
            "### Heading",                             // Markdown, no number
        ] {
            assert_eq!(l.apply(text), text);
        }
        // A sentence's end is fine.
        assert_eq!(
            l.apply("merge !99."),
            "merge [!99](https://gl/p/-/merge_requests/99)."
        );
    }
}
