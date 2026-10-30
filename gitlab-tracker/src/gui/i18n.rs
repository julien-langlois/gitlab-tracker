//! The window's translations, on `bevel-i18n`, behind [`crate::i18n`].
//!
//! The catalogs are `gitlab-tracker/i18n/<locale>.tsv`: one row per line,
//! `context \t English \t translation`, with `@plural` rows (`one|other` →
//! the language's forms) for counts. A string missing from a catalog shows in
//! English. The TUI and the core stay in English: their text goes through
//! [`crate::i18n`], which only translates once [`install`] ran.

use std::sync::{LazyLock, PoisonError, RwLock};

use bevel_i18n::{Embedded, I18n, Lang};

/// The embedded catalogs: (locale, shared catalog, the app's catalog).
static CATALOGS: &Embedded = &[
    ("de", "", include_str!("../../i18n/de.tsv")),
    ("es", "", include_str!("../../i18n/es.tsv")),
    ("fr", "", include_str!("../../i18n/fr.tsv")),
    ("pt-br", "", include_str!("../../i18n/pt-br.tsv")),
    ("zh-hans", "", include_str!("../../i18n/zh-hans.tsv")),
];

static I18N: LazyLock<I18n> = LazyLock::new(|| {
    let offered: Vec<&str> = CATALOGS.iter().map(|(code, _, _)| *code).collect();
    let (i18n, problems) = I18n::new(CATALOGS, &offered);
    for problem in problems {
        tracing::warn!("translation catalog: {problem}");
    }
    i18n
});

/// The language the window is drawn in. Global rather than per thread: the
/// notifications and messages are also written on the runtime's threads.
static CURRENT: RwLock<Lang> = RwLock::new(Lang::EN);

fn current() -> Lang {
    *CURRENT.read().unwrap_or_else(PoisonError::into_inner)
}

/// Routes [`crate::i18n`] through the catalogs.
pub fn install() {
    crate::i18n::install(crate::i18n::Translator {
        text: |source| I18N.t(current(), source),
        text_in: |context, source| I18N.tc(current(), context, source),
        count: |n, one, other| I18N.trn(current(), n, one, other),
    });
}

/// Draws the window in the language of the `language` setting: a code such as
/// `fr`, or `None` (or an unknown code) to follow the system.
pub fn set_language(language: Option<&str>) {
    let lang = language
        .and_then(|code| I18N.from_code(code))
        .unwrap_or_else(|| I18N.system_lang());
    *CURRENT.write().unwrap_or_else(PoisonError::into_inner) = lang;
}

/// The languages to choose from: (code, name in itself), English first.
pub fn languages() -> Vec<(&'static str, &'static str)> {
    I18N.languages()
        .into_iter()
        .map(|lang| (I18N.code(lang), I18N.name(lang)))
        .collect()
}

/// The name of the language the system asks for, as [`languages`] writes it.
pub fn system_language_name() -> &'static str {
    I18N.name(I18N.system_lang())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_have_no_bad_rows() {
        assert_eq!(I18N.check_all(), Vec::<String>::new());
    }

    #[test]
    fn every_language_has_a_name() {
        for (code, name) in languages() {
            assert_ne!(code, name, "{code} needs an `@id language.name` row");
        }
    }
}
