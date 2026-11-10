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

/// Texts of the tray, the native menu and the notifications. One function
/// per text with an exhaustive `match`, so a new `Language` variant does
/// not compile until every text has it. Webview texts live in `src/i18n/`.
pub mod text {
    use super::Language::{self, De, En};

    fn plural(n: usize, one: &str, other: &str) -> String {
        format!("{n} {}", if n == 1 { one } else { other })
    }

    /// "1 session" / "2 sessions".
    pub fn sessions(l: Language, n: usize) -> String {
        match l {
            En => plural(n, "session", "sessions"),
            De => plural(n, "Session", "Sessions"),
        }
    }

    /// Fallback name of a session whose agent is unknown and whose command is empty.
    pub fn session_fallback(l: Language) -> &'static str {
        match l {
            En => "session",
            De => "Session",
        }
    }

    // Tray.

    pub fn tray_show(l: Language) -> &'static str {
        match l {
            En => "Show Sandbox Watcher",
            De => "Sandbox Watcher anzeigen",
        }
    }

    pub fn tray_empty(l: Language) -> &'static str {
        match l {
            En => "No running sandboxes",
            De => "Keine laufenden Sandboxes",
        }
    }

    pub fn tray_project_idle(l: Language, name: &str) -> String {
        match l {
            En => format!("{name}: idle"),
            De => format!("{name}: inaktiv"),
        }
    }

    pub fn tray_project_sessions(l: Language, name: &str, n: usize) -> String {
        format!("{name}: {}", sessions(l, n))
    }

    pub fn tray_project_containers(l: Language, name: &str, containers: usize, n: usize) -> String {
        match l {
            En => format!("{name}: {containers} containers, {}", sessions(l, n)),
            De => format!("{name}: {containers} Container, {}", sessions(l, n)),
        }
    }

    pub fn tray_stop_all(l: Language) -> &'static str {
        match l {
            En => "Stop all sandboxes",
            De => "Alle Sandboxes stoppen",
        }
    }

    pub fn tray_settings(l: Language) -> &'static str {
        match l {
            En => "Settings…",
            De => "Einstellungen…",
        }
    }

    pub fn tray_quit(l: Language) -> &'static str {
        match l {
            En => "Quit Sandbox Watcher",
            De => "Sandbox Watcher beenden",
        }
    }

    pub fn tray_tooltip(l: Language, running: usize, n: usize) -> String {
        match l {
            En => format!("Sandbox Watcher: {running} running, {}", sessions(l, n)),
            De => format!("Sandbox Watcher: {running} laufend, {}", sessions(l, n)),
        }
    }

    // Native application menu. `None` keeps Tauri's own English label.

    pub fn menu_window(l: Language) -> &'static str {
        match l {
            En => "Window",
            De => "Fenster",
        }
    }

    pub fn menu_help(l: Language) -> &'static str {
        match l {
            En => "Help",
            De => "Hilfe",
        }
    }

    pub fn menu_file(l: Language) -> &'static str {
        match l {
            En => "File",
            De => "Datei",
        }
    }

    pub fn menu_edit(l: Language) -> &'static str {
        match l {
            En => "Edit",
            De => "Bearbeiten",
        }
    }

    pub fn menu_about(l: Language, product: &str) -> String {
        match l {
            En => format!("About {product}"),
            De => format!("Über {product}"),
        }
    }

    pub fn menu_hide(l: Language, product: &str) -> String {
        match l {
            En => format!("Hide {product}"),
            De => format!("{product} ausblenden"),
        }
    }

    pub fn menu_quit(l: Language, product: &str) -> String {
        match l {
            En => format!("Quit {product}"),
            De => format!("{product} beenden"),
        }
    }

    pub fn menu_check_updates(l: Language) -> &'static str {
        match l {
            En => "Check for Updates…",
            De => "Nach Updates suchen…",
        }
    }

    /// Labels of Tauri's predefined items, keyed by their builder name.
    pub fn menu_predefined(l: Language, item: &str) -> Option<&'static str> {
        match l {
            En => None,
            De => Some(match item {
                "minimize" => "Minimieren",
                "maximize" => "Maximieren",
                "close_window" => "Fenster schließen",
                "services" => "Dienste",
                "hide_others" => "Andere ausblenden",
                "show_all" => "Alle einblenden",
                "quit" => "Beenden",
                "undo" => "Widerrufen",
                "redo" => "Wiederholen",
                "cut" => "Ausschneiden",
                "copy" => "Kopieren",
                "paste" => "Einsetzen",
                "select_all" => "Alles auswählen",
                _ => return None,
            }),
        }
    }

    // Notifications: titles name the project, bodies the container.

    pub fn container_started_title(l: Language, slug: &str) -> String {
        match l {
            En => format!("Sandbox started: {slug}"),
            De => format!("Sandbox gestartet: {slug}"),
        }
    }

    pub fn container_running_body(l: Language, name: &str) -> String {
        match l {
            En => format!("{name} is running"),
            De => format!("{name} läuft"),
        }
    }

    pub fn container_running_again_body(l: Language, name: &str) -> String {
        match l {
            En => format!("{name} is running again"),
            De => format!("{name} läuft wieder"),
        }
    }

    pub fn container_died_title(l: Language, slug: &str) -> String {
        match l {
            En => format!("Sandbox stopped unexpectedly: {slug}"),
            De => format!("Sandbox unerwartet gestoppt: {slug}"),
        }
    }

    pub fn container_died_body(l: Language, name: &str, n: usize) -> String {
        match l {
            En => format!("{name} stopped while {} ran", sessions(l, n)),
            De => format!(
                "{name} wurde gestoppt, während {} {}",
                sessions(l, n),
                if n == 1 { "lief" } else { "liefen" }
            ),
        }
    }

    pub fn container_stopped_title(l: Language, slug: &str) -> String {
        match l {
            En => format!("Sandbox stopped: {slug}"),
            De => format!("Sandbox gestoppt: {slug}"),
        }
    }

    pub fn container_stopped_body(l: Language, name: &str) -> String {
        match l {
            En => format!("{name} is no longer running"),
            De => format!("{name} läuft nicht mehr"),
        }
    }

    pub fn session_started_title(l: Language, agent: &str, slug: &str) -> String {
        match l {
            En => format!("{agent} started in {slug}"),
            De => format!("{agent} in {slug} gestartet"),
        }
    }

    pub fn session_ended_title(l: Language, agent: &str, slug: &str) -> String {
        match l {
            En => format!("{agent} ended in {slug}"),
            De => format!("{agent} in {slug} beendet"),
        }
    }

    pub fn session_body(l: Language, name: &str, pid: u32) -> String {
        match l {
            En | De => format!("{name}, PID {pid}"),
        }
    }

    pub fn stale_title(l: Language, slug: &str) -> String {
        match l {
            En => format!("Stale sandbox: {slug}"),
            De => format!("Veraltete Sandbox: {slug}"),
        }
    }

    pub fn stale_body(l: Language, name: &str) -> String {
        match l {
            En => format!(
                "{name} was created from an older configuration than the newest container of this project"
            ),
            De => format!(
                "{name} wurde aus einer älteren Konfiguration erstellt als der neueste Container dieses Projekts"
            ),
        }
    }

    pub fn burst_title(l: Language, slug: &str) -> String {
        match l {
            En => format!("Blocked requests: {slug}"),
            De => format!("Blockierte Verbindungen: {slug}"),
        }
    }

    pub fn burst_body(l: Language, n: u64, name: &str) -> String {
        match l {
            En => format!("{n} blocked connections recently from {name}"),
            De => format!("{n} blockierte Verbindungen kürzlich von {name}"),
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
    fn texts_pluralise_in_both_languages() {
        assert_eq!(text::sessions(Language::En, 1), "1 session");
        assert_eq!(text::sessions(Language::En, 2), "2 sessions");
        assert_eq!(text::sessions(Language::De, 1), "1 Session");
        assert_eq!(text::sessions(Language::De, 0), "0 Sessions");
        assert_eq!(
            text::container_died_body(Language::De, "sandbox-web", 1),
            "sandbox-web wurde gestoppt, während 1 Session lief"
        );
        assert_eq!(
            text::container_died_body(Language::En, "sandbox-web", 3),
            "sandbox-web stopped while 3 sessions ran"
        );
        assert_eq!(
            text::tray_tooltip(Language::De, 2, 1),
            "Sandbox Watcher: 2 laufend, 1 Session"
        );
        assert_eq!(text::menu_predefined(Language::En, "copy"), None);
        assert_eq!(
            text::menu_predefined(Language::De, "copy"),
            Some("Kopieren")
        );
        assert_eq!(text::menu_predefined(Language::De, "unknown"), None);
        assert_eq!(
            text::menu_check_updates(Language::De),
            "Nach Updates suchen…"
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
