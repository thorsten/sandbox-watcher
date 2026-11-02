import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { LanguageProvider, setLanguage } from "../i18n";
import type { Language } from "../types";

/**
 * Render to HTML and strip tags for text assertions. `language` renders in
 * that language and restores English afterwards.
 */
export function render(
  element: ReactElement,
  { language = "en" }: { language?: Language } = {},
): { html: string; text: string } {
  let html: string;
  try {
    html = renderToStaticMarkup(<LanguageProvider language={language}>{element}</LanguageProvider>);
  } finally {
    setLanguage("en");
  }
  const text = html
    .replace(/<[^>]+>/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/\s+/g, " ")
    .trim();
  return { html, text };
}

/** Count occurrences of a substring in the markup. */
export function count(html: string, needle: string): number {
  return html.split(needle).length - 1;
}
