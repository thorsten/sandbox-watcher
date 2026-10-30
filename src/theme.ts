import type { Theme } from "./types";

/** Apply the theme by attribute; CSS variables switch on `data-theme`. */
export function applyTheme(theme: Theme, root: HTMLElement = document.documentElement): void {
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
}
