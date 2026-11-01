// Light/dark switch. Loaded in <head> so a stored choice applies before the
// first paint. Without a stored choice the page follows the system setting.
// Compiled to theme.js by `pnpm build:site`; the Pages workflow runs that
// before uploading the site.
(() => {
  type Theme = "light" | "dark";
  const key = "theme";
  const root = document.documentElement;
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const themeColor: Record<Theme, string> = { light: "#f7f8fb", dark: "#0a101f" };

  const stored = (): Theme | null => {
    try {
      const v = localStorage.getItem(key);
      return v === "light" || v === "dark" ? v : null;
    } catch {
      return null;
    }
  };
  const effective = (): Theme => stored() ?? (media.matches ? "dark" : "light");

  const apply = () => {
    const choice = stored();
    if (choice) root.setAttribute("data-theme", choice);
    else root.removeAttribute("data-theme");
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute("content", themeColor[effective()]);
    const next = effective() === "dark" ? "light" : "dark";
    for (const button of document.querySelectorAll(".theme-toggle")) {
      button.setAttribute("aria-label", `Switch to ${next} mode`);
      button.setAttribute("title", `Switch to ${next} mode`);
    }
  };

  apply();
  media.addEventListener("change", apply);
  document.addEventListener("DOMContentLoaded", () => {
    apply();
    for (const button of document.querySelectorAll(".theme-toggle")) {
      button.addEventListener("click", () => {
        const next: Theme = effective() === "dark" ? "light" : "dark";
        try {
          // Back to "follow the system" when the choice matches the system again.
          if ((next === "dark") === media.matches) localStorage.removeItem(key);
          else localStorage.setItem(key, next);
        } catch {
          /* private mode: the switch still works for this page view */
          root.setAttribute("data-theme", next);
          return;
        }
        apply();
      });
    }
  });
})();
