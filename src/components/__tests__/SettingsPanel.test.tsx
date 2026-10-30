import { describe, expect, it, vi } from "vitest";
import { settings } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { SettingsPanel } from "../SettingsPanel";

const noop = vi.fn<() => void>();

describe("SettingsPanel", () => {
  it("renders every section", () => {
    const { text } = render(<SettingsPanel settings={settings()} onSaved={noop} onClose={noop} />);
    for (const legend of [
      "Refresh",
      "Actions",
      "Notifications",
      "History and cost",
      "Updates",
      "Window",
    ]) {
      expect(text).toContain(legend);
    }
    expect(text).toContain("Check now");
    expect(text).toContain("Save");
  });

  it("reflects the current values", () => {
    const { html } = render(
      <SettingsPanel
        settings={settings({
          poll_interval_secs: 42,
          history_retention_days: 7,
          check_for_updates: false,
        })}
        onSaved={noop}
        onClose={noop}
      />,
    );
    expect(html).toContain('value="42"');
    expect(html).toContain('value="7"');
    expect(html).toMatch(/<input type="checkbox"\/>Check for new releases/);
  });

  it("disables individual notification toggles when notifications are off", () => {
    const base = settings();
    const off = settings({ notifications: { ...base.notifications, enabled: false } });
    const { html } = render(<SettingsPanel settings={off} onSaved={noop} onClose={noop} />);
    expect(count(html, 'disabled=""')).toBe(7);
  });

  it("shows the custom terminal command field only for custom", () => {
    const auto = render(<SettingsPanel settings={settings()} onSaved={noop} onClose={noop} />);
    expect(auto.html).not.toContain("{dir}");
    const custom = render(
      <SettingsPanel
        settings={settings({ terminal: { kind: "custom", command: "wezterm start --cwd {dir}" } })}
        onSaved={noop}
        onClose={noop}
      />,
    );
    expect(custom.html).toContain('value="wezterm start --cwd {dir}"');
  });

  it("prefills the price table as JSON", () => {
    const { text } = render(<SettingsPanel settings={settings()} onSaved={noop} onClose={noop} />);
    expect(text).toContain('"claude-sonnet-4": {');
    expect(text).toContain('"output": 15');
  });
});
