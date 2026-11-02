/**
 * Translation for the webview. The language is one module-level value so
 * that pure helpers (`formatUptime`, `describeStopReport`, ...) and
 * components agree; `LanguageProvider` sets it for the tree it wraps. The
 * value itself comes from the shell (`resolvedLanguage`), never from
 * `navigator.language`.
 */
import { Fragment, createContext, useContext, type ReactNode } from "react";
import type { ContainerState, Language } from "../types";
import { de } from "./de";
import type { Dictionary, Key, Params, PlainKey, PluralForms, PluralKey } from "./dictionary";
import { en } from "./en";

export type { Key, PlainKey, PluralKey } from "./dictionary";

const DICTIONARIES: Record<Language, Dictionary> = { en, de };

/** BCP 47 tags for `Intl`; the UI language alone has no region. */
const LOCALES: Record<Language, string> = { en: "en-US", de: "de-DE" };

let current: Language = "en";

export function getLanguage(): Language {
  return current;
}

export function setLanguage(language: Language): void {
  current = language;
}

/** The `Intl` locale of the current language. */
export function locale(): string {
  return LOCALES[current];
}

const pluralRules = new Map<Language, Intl.PluralRules>();
const numberFormats = new Map<string, Intl.NumberFormat>();

function plural(language: Language): Intl.PluralRules {
  let rules = pluralRules.get(language);
  if (!rules) {
    rules = new Intl.PluralRules(LOCALES[language]);
    pluralRules.set(language, rules);
  }
  return rules;
}

/** A cached `Intl.NumberFormat` for the current language. */
export function numberFormat(options: Intl.NumberFormatOptions = {}): Intl.NumberFormat {
  const id = `${current}:${JSON.stringify(options)}`;
  let format = numberFormats.get(id);
  if (!format) {
    format = new Intl.NumberFormat(LOCALES[current], options);
    numberFormats.set(id, format);
  }
  return format;
}

export function formatNumber(n: number, options?: Intl.NumberFormatOptions): string {
  return numberFormat(options).format(n);
}

function interpolate(template: string, params: Params | undefined): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = params[name];
    if (value === undefined) return match;
    return typeof value === "number" ? formatNumber(value) : value;
  });
}

/**
 * The text for `key` in the current language. Plural keys pick their form
 * from `params.n`; numbers are formatted for the language.
 */
export function t(key: PlainKey, params?: Params): string;
export function t(key: PluralKey, params: Params & { n: number }): string;
export function t(key: Key, params?: Params): string {
  const entry = DICTIONARIES[current][key] ?? en[key];
  if (typeof entry === "string") return interpolate(entry, params);
  const forms: PluralForms = entry;
  const n = typeof params?.n === "number" ? params.n : 0;
  const form = plural(current).select(n) === "one" ? forms.one : forms.other;
  return interpolate(form, params);
}

/**
 * Like `t`, but placeholders become React nodes, for sentences that wrap a
 * `<code>` or a link. Plural keys take `n` from `parts`. Placeholders
 * without a part are left as they are; numbers are formatted.
 */
export function tx(key: Key, parts: Record<string, ReactNode>): ReactNode {
  const entry = DICTIONARIES[current][key] ?? en[key];
  let template: string;
  if (typeof entry === "string") template = entry;
  else {
    const n = typeof parts.n === "number" ? parts.n : 0;
    template = plural(current).select(n) === "one" ? entry.one : entry.other;
  }
  return (
    <>
      {template.split(/(\{\w+\})/).map((piece, i) => {
        const name = /^\{(\w+)\}$/.exec(piece)?.[1];
        if (name === undefined || !(name in parts)) return piece;
        const part = parts[name];
        return <Fragment key={i}>{typeof part === "number" ? formatNumber(part) : part}</Fragment>;
      })}
    </>
  );
}

/** A container state as a word. */
export function stateLabel(state: ContainerState): string {
  return t(`state.${state}`);
}

const LanguageContext = createContext<Language>("en");

/**
 * Sets the language for the tree below. Assigning the module value during
 * render keeps `t` and the pure helpers called by children in step. App
 * re-renders the whole tree on a change, so components call `t` directly;
 * a memoised component would read `useLanguage()` to re-render too.
 */
export function LanguageProvider({
  language,
  children,
}: {
  language: Language;
  children: ReactNode;
}) {
  setLanguage(language);
  return <LanguageContext.Provider value={language}>{children}</LanguageContext.Provider>;
}

export function useLanguage(): Language {
  return useContext(LanguageContext);
}

const dateTimeFormats = new Map<string, Intl.DateTimeFormat>();

function dateTimeFormat(options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const id = `${current}:${JSON.stringify(options)}`;
  let format = dateTimeFormats.get(id);
  if (!format) {
    format = new Intl.DateTimeFormat(LOCALES[current], options);
    dateTimeFormats.set(id, format);
  }
  return format;
}

/** Date and time of an ISO timestamp, e.g. "Oct 30, 2026, 12:00 PM" or "30.10.2026, 12:00". */
export function formatDateTime(iso: string | Date): string {
  const date = iso instanceof Date ? iso : new Date(iso);
  if (Number.isNaN(date.getTime())) return t("common.unknown");
  return dateTimeFormat({ dateStyle: "medium", timeStyle: "short" }).format(date);
}

/** Time of day with seconds, for "refreshed at". */
export function formatTime(date: Date): string {
  return dateTimeFormat({ timeStyle: "medium" }).format(date);
}
