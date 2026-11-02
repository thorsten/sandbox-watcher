# ADR 0003: Localisation

Status: accepted, 2026-11-02

## Context

The app was English only. It should speak German as well, and a third
language should be a matter of writing one dictionary per side, not of
touching every component. Text lives in three places: the React webview,
the Tauri shell (tray, native menu) and the core (notifications). The
language the user wants is either explicit or "whatever the system uses".

## Decision

1. **Language is a setting with a system default.** `Settings.language`
   is `system | en | de`. The pure resolver `i18n::Language::resolve` in
   core turns it into a concrete `Language`, taking the system locale the
   shell reads with `sys-locale`, English as fallback.
2. **The Rust side resolves, the webview asks.** The webview calls
   `resolved_language` after every settings load and save and never reads
   `navigator.language`: in a WKWebView that follows the bundle's declared
   localisations, not the user's system language, so the two sides would
   disagree. One resolver keeps UI, tray and notifications in step.
3. **Typed dictionaries, no i18n library.** `src/i18n/en.ts` is the
   source of truth; `de.ts` is declared as its type, so a missing or extra
   key fails `tsc`. `t()` interpolates `{param}`, picks plural forms with
   `Intl.PluralRules`, formats numbers and dates for the language; `tx()`
   does the same with React nodes for sentences that wrap a `<code>`. The
   language is one module-level value, set by `LanguageProvider`; App
   re-renders the whole tree on a change, so components call `t` directly.
4. **One function per text on the Rust side.** `i18n::text` holds the
   tray, menu and notification texts with an exhaustive `match` over
   `Language`, so a new variant does not compile until every text has it.
5. **German addresses the user as "du".** Session, Turn and Prompt stay
   untranslated; Container, Sandbox, Host and Mount as well.

## Consequences

- Ids that double as CSS classes or wire values (container states, file
  statuses, mount roles, git change kinds) stay English; only their labels
  are translated.
- Product names, commands, log lines, config lines and the diagnostics
  plain text stay as they are. Errors from Tauri commands are still
  English strings; translating them needs a typed error (see
  `docs/I18N.md`, Step 6).
- Adding a language: a `Language` and a `LanguageChoice` variant, a
  dictionary typed as `Dictionary`, an entry in `DICTIONARIES` and
  `LOCALES`, an option in the Settings select, and the contract fixtures.
- The German guard test renders every component in German and fails on a
  list of English words, so a string that never got a key is caught.
