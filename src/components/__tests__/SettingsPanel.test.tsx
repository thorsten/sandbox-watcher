import { describe, expect, it, vi } from "vitest";
import { settings } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { SettingsPanel, type SettingsSection } from "../SettingsPanel";

const noop = vi.fn<() => void>();

function panel(section: SettingsSection, partial: Parameters<typeof settings>[0] = {}) {
  return render(
    <SettingsPanel
      settings={settings(partial)}
      initialSection={section}
      onSaved={noop}
      onClose={noop}
    />,
  );
}

describe("SettingsPanel", () => {
  it("lists every section and shows General first", () => {
    const { text, html } = render(
      <SettingsPanel settings={settings()} onSaved={noop} onClose={noop} />,
    );
    for (const name of [
      "General",
      "Runtime",
      "Actions",
      "Notifications",
      "History",
      "Diagnostics",
    ]) {
      expect(text).toContain(name);
    }
    expect(html).toContain('class="active" aria-current="page"');
    expect(text).toContain("Appearance");
    expect(text).toContain("Window");
    expect(text).toContain("Check now");
    expect(text).not.toContain("Run diagnostics");
    expect(text).toContain("Save");
    expect(html).toContain('class="close"');
    expect(html).toContain('aria-label="Close"');
  });

  it("opens the requested section", () => {
    const legends: [SettingsSection, string[]][] = [
      ["general", ["Appearance", "Window", "Updates"]],
      ["runtime", ["Refresh", "Remote hosts"]],
      ["actions", ["Ask before stopping containers", "Editor"]],
      ["notifications", ["Enable notifications"]],
      ["history", ["History and cost"]],
      ["diagnostics", ["Run diagnostics"]],
    ];
    for (const [section, expected] of legends) {
      const { text } = panel(section);
      for (const legend of expected) expect(`${section}: ${text}`).toContain(legend);
    }
  });

  it("reflects the current values", () => {
    expect(panel("runtime", { poll_interval_secs: 42 }).html).toContain('value="42"');
    expect(panel("history", { history_retention_days: 7 }).html).toContain('value="7"');
    expect(panel("general", { check_for_updates: false }).html).toMatch(
      /<input type="checkbox"\/>Check for new releases/,
    );
  });

  it("disables individual notification toggles when notifications are off", () => {
    const base = settings();
    const { html } = panel("notifications", {
      notifications: { ...base.notifications, enabled: false },
    });
    expect(count(html, 'disabled=""')).toBe(7);
  });

  it("shows the custom terminal command field only for custom", () => {
    expect(panel("actions").html).not.toContain("{dir}");
    const custom = panel("actions", {
      terminal: { kind: "custom", command: "wezterm start --cwd {dir}" },
    });
    expect(custom.html).toContain('value="wezterm start --cwd {dir}"');
  });

  it("prefills the price table as JSON", () => {
    const { text } = panel("history");
    expect(text).toContain('"claude-sonnet-4": {');
    expect(text).toContain('"output": 15');
  });

  it("offers the editors and shows the command field only for custom", () => {
    const system = panel("actions");
    for (const name of ["Visual Studio Code", "Cursor", "Zed", "JetBrains IDE"]) {
      expect(system.text).toContain(name);
    }
    expect(system.html).not.toContain("subl {path}");
    const custom = panel("actions", { editor: { kind: "custom", command: "subl {path}" } });
    expect(custom.html).toContain('value="subl {path}"');
  });

  it("offers the languages with the saved one selected", () => {
    const { html, text } = panel("general", { language: "de" });
    expect(text).toContain("Language");
    for (const name of ["System", "English", "Deutsch"]) expect(text).toContain(name);
    expect(html).toContain('<option value="de" selected="">Deutsch</option>');
  });
});
