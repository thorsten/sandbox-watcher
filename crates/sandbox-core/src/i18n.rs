//! Languages the app speaks. The setting (`LanguageChoice`) may follow the
//! system; this module resolves it to a concrete `Language` once, on the
//! Rust side, so the webview, the tray and the notifications agree. The
//! webview must not use `navigator.language`: in a WKWebView it follows the
//! bundle's declared localisations, not the user's system language.

use serde::{Deserialize, Serialize};

use crate::settings::LanguageChoice;

/// A language with a complete translation. Adding one: a variant here, a
/// `LanguageChoice` variant, a dictionary in `src/i18n/`, and an option in
/// the Settings panel. The exhaustive `match`es then point to every text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    En,
    De,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::En, Language::De];

    /// BCP 47 primary subtag, also the `lang` attribute of the document.
    pub fn tag(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::De => "de",
        }
    }

    /// The language for a locale tag such as `de-DE`, `de_AT.UTF-8`, `en-US`
    /// or `C`. Only the primary subtag counts; unknown ones give `None`.
    pub fn from_locale(locale: &str) -> Option<Language> {
        let primary = locale
            .trim()
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        Language::ALL.into_iter().find(|l| l.tag() == primary)
    }

    /// The language to use for `choice`. `System` takes `system_locale` (as
    /// the OS reports it, `None` when unknown) and falls back to English.
    pub fn resolve(choice: LanguageChoice, system_locale: Option<&str>) -> Language {
        match choice {
            LanguageChoice::En => Language::En,
            LanguageChoice::De => Language::De,
            LanguageChoice::System => system_locale
                .and_then(Language::from_locale)
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_choice_ignores_the_system() {
        assert_eq!(
            Language::resolve(LanguageChoice::De, Some("en-US")),
            Language::De
        );
        assert_eq!(
            Language::resolve(LanguageChoice::En, Some("de-DE")),
            Language::En
        );
        assert_eq!(Language::resolve(LanguageChoice::De, None), Language::De);
    }

    #[test]
    fn system_follows_the_primary_subtag() {
        for tag in ["de", "de-DE", "de_AT.UTF-8", "DE-CH", "de@euro", " de-DE "] {
            assert_eq!(
                Language::resolve(LanguageChoice::System, Some(tag)),
                Language::De,
                "{tag}"
            );
        }
        for tag in ["en", "en-GB", "en_US.UTF-8"] {
            assert_eq!(
                Language::resolve(LanguageChoice::System, Some(tag)),
                Language::En,
                "{tag}"
            );
        }
    }

    #[test]
    fn unknown_or_missing_system_locale_falls_back_to_english() {
        for tag in ["fr-FR", "C", "POSIX", "", "deutsch"] {
            assert_eq!(
                Language::resolve(LanguageChoice::System, Some(tag)),
                Language::En,
                "{tag:?}"
            );
        }
        assert_eq!(
            Language::resolve(LanguageChoice::System, None),
            Language::En
        );
    }

    #[test]
    fn tags_are_the_wire_values() {
        for lang in Language::ALL {
            assert_eq!(
                serde_json::to_value(lang).unwrap(),
                serde_json::Value::String(lang.tag().to_string())
            );
            assert_eq!(Language::from_locale(lang.tag()), Some(lang));
        }
    }
}
