import type { Language } from "./types";

/**
 * Mark the document with the language it is rendered in, for hyphenation,
 * spell checking in inputs and assistive technology.
 */
export function applyLanguage(
  language: Language,
  root: HTMLElement = document.documentElement,
): void {
  root.lang = language;
}
