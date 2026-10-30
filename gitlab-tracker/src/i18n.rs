//! Translation hook for the text the desktop window shows.
//!
//! Text is written in English and looked up by that English text: [`tl`] for
//! a string, [`tc`] for one whose English has several meanings, [`tf`] for a
//! template with `{name}` placeholders, [`tn`] for a count. Until a
//! [`Translator`] is installed (the GUI installs one, see `gui/i18n.rs`) these
//! return the English itself, so the TUI is unchanged and needs no translation
//! crate.

#![cfg_attr(
    not(feature = "gui"),
    allow(dead_code, reason = "only the GUI installs a translator")
)]

use std::sync::OnceLock;

/// Where translations come from.
pub struct Translator {
    /// `source` in the current language, or `source` itself.
    pub text: for<'a> fn(&'a str) -> &'a str,
    /// `source` in a `context` (`date`…), else as [`Translator::text`] has it.
    pub text_in: for<'a> fn(&str, &'a str) -> &'a str,
    /// A count's wording, `{n}` filled: `one` and `other` are the English forms.
    pub count: fn(u64, &str, &str) -> String,
}

static TRANSLATOR: OnceLock<Translator> = OnceLock::new();

/// Routes [`tl`], [`tf`] and [`tn`] through `translator` from now on. Only the
/// first call counts.
pub fn install(translator: Translator) {
    let _ = TRANSLATOR.set(translator);
}

/// `source` in the current language.
pub fn tl(source: &str) -> &str {
    TRANSLATOR.get().map_or(source, |t| (t.text)(source))
}

/// `source` in a `context`, for English with several meanings: "Merged" as a
/// date (`date`) or as a count.
pub fn tc<'a>(context: &str, source: &'a str) -> &'a str {
    TRANSLATOR
        .get()
        .map_or(source, |t| (t.text_in)(context, source))
}

/// [`tl`] of `template`, with its `{name}` placeholders filled from `args`.
/// Placeholders without an argument stay as written.
pub fn tf(template: &str, args: &[(&str, &str)]) -> String {
    fill(tl(template), args)
}

/// The wording of `n` things: `one` and `other` are the English singular and
/// plural, with `{n}` where the number goes (`"{n} MR"`, `"{n} MRs"`).
pub fn tn(n: u64, one: &str, other: &str) -> String {
    match TRANSLATOR.get() {
        Some(t) => (t.count)(n, one, other),
        None => fill(if n == 1 { one } else { other }, &[("n", &n.to_string())]),
    }
}

/// Fills `{name}` placeholders in one pass, so an inserted value is never read
/// as a template.
fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let value = after.find('}').and_then(|end| {
            let name = &after[..end];
            args.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value, end))
        });
        match value {
            Some((value, end)) => {
                out.push_str(value);
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untranslated_text_is_the_english() {
        assert_eq!(tl("Save"), "Save");
        assert_eq!(
            tf("{n} of {total} · {n}", &[("n", "2"), ("total", "{n}")]),
            "2 of {n} · 2"
        );
        assert_eq!(tf("{unknown} stays", &[]), "{unknown} stays");
        assert_eq!(tn(1, "{n} MR", "{n} MRs"), "1 MR");
        assert_eq!(tn(3, "{n} MR", "{n} MRs"), "3 MRs");
    }
}
