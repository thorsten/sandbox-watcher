import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

/** Render to HTML and strip tags for text assertions. */
export function render(element: ReactElement): { html: string; text: string } {
  const html = renderToStaticMarkup(element);
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
