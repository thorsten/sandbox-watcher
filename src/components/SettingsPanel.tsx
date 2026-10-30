import { useState } from "react";
import { saveSettings } from "../api";
import { RUNTIME_LABELS, type RuntimeKind, type Settings } from "../types";

interface Props {
  settings: Settings;
  onSaved: (settings: Settings) => void;
  onClose: () => void;
}

const RUNTIMES: RuntimeKind[] = ["docker", "podman", "apple-container"];
const isMac = navigator.platform.toLowerCase().includes("mac");

export function SettingsPanel({ settings, onSaved, onClose }: Props) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [error, setError] = useState<string | null>(null);

  const update = (patch: Partial<Settings>) => setDraft((d) => ({ ...d, ...patch }));
  const updateNotify = (patch: Partial<Settings["notifications"]>) =>
    setDraft((d) => ({ ...d, notifications: { ...d.notifications, ...patch } }));

  const save = async () => {
    setStatus("saving");
    try {
      const saved = await saveSettings(draft);
      onSaved(saved);
      setStatus("saved");
      window.setTimeout(() => setStatus("idle"), 1500);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStatus("error");
    }
  };

  const terminalKind = draft.terminal.kind;
  const customCommand = draft.terminal.kind === "custom" ? draft.terminal.command : "";

  return (
    <section className="settings" aria-labelledby="settings-title">
      <header className="settings-header">
        <h2 id="settings-title">Settings</h2>
        <button type="button" onClick={onClose}>
          Close
        </button>
      </header>

      <fieldset>
        <legend>Refresh</legend>
        <label>
          Fallback poll interval
          <input
            type="number"
            min={2}
            max={300}
            value={draft.poll_interval_secs}
            onChange={(e) => update({ poll_interval_secs: Number(e.target.value) })}
          />
          <span className="muted">seconds. Runtime events refresh sooner.</span>
        </label>
        <label>
          Runtime
          <select
            value={draft.runtime_override ?? ""}
            onChange={(e) =>
              update({ runtime_override: (e.target.value || null) as RuntimeKind | null })
            }
          >
            <option value="">Follow sandbox config, else all detected</option>
            {RUNTIMES.map((r) => (
              <option key={r} value={r}>
                Only {RUNTIME_LABELS[r]}
              </option>
            ))}
          </select>
        </label>
      </fieldset>

      <fieldset>
        <legend>Actions</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.confirm_stop}
            onChange={(e) => update({ confirm_stop: e.target.checked })}
          />
          Ask before stopping containers
        </label>
        <label>
          Terminal
          <select
            value={terminalKind}
            onChange={(e) => {
              const kind = e.target.value as Settings["terminal"]["kind"];
              update({
                terminal: kind === "custom" ? { kind, command: customCommand } : { kind },
              });
            }}
          >
            <option value="auto">
              {isMac ? "iTerm2 if installed, else Terminal.app" : "System default"}
            </option>
            {isMac && <option value="iterm">iTerm2</option>}
            {isMac && <option value="terminal-app">Terminal.app</option>}
            <option value="custom">Custom command</option>
          </select>
        </label>
        {terminalKind === "custom" && (
          <label>
            Command
            <input
              type="text"
              value={customCommand}
              placeholder="wezterm start --cwd {dir}"
              onChange={(e) => update({ terminal: { kind: "custom", command: e.target.value } })}
            />
            <span className="muted">
              <code>{"{dir}"}</code> is replaced by the quoted project directory.
            </span>
          </label>
        )}
      </fieldset>

      <fieldset>
        <legend>Notifications</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.notifications.enabled}
            onChange={(e) => updateNotify({ enabled: e.target.checked })}
          />
          Enable notifications
        </label>
        <div className="settings-grid">
          {(
            [
              ["session_started", "Session started"],
              ["session_ended", "Session ended"],
              ["container_started", "Sandbox started"],
              ["container_stopped", "Sandbox stopped"],
              ["container_died", "Sandbox stopped while sessions ran"],
              ["blocked_burst", "Burst of blocked requests"],
              ["stale_container", "Stale container detected"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="check">
              <input
                type="checkbox"
                disabled={!draft.notifications.enabled}
                checked={draft.notifications[key]}
                onChange={(e) => updateNotify({ [key]: e.target.checked })}
              />
              {label}
            </label>
          ))}
        </div>
        <label>
          Blocked burst threshold
          <input
            type="number"
            min={1}
            max={10000}
            value={draft.blocked_burst_threshold}
            onChange={(e) => update({ blocked_burst_threshold: Number(e.target.value) })}
          />
          <span className="muted">requests within</span>
          <input
            type="number"
            min={10}
            max={3600}
            value={draft.blocked_burst_window_secs}
            onChange={(e) => update({ blocked_burst_window_secs: Number(e.target.value) })}
          />
          <span className="muted">seconds</span>
        </label>
      </fieldset>

      <fieldset>
        <legend>Window</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.close_to_tray}
            onChange={(e) => update({ close_to_tray: e.target.checked })}
          />
          Keep running in the tray when the window is closed
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.start_minimized}
            onChange={(e) => update({ start_minimized: e.target.checked })}
          />
          Start hidden in the tray
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.launch_at_login}
            onChange={(e) => update({ launch_at_login: e.target.checked })}
          />
          Launch at login
        </label>
        <label>
          Theme
          <select
            value={draft.theme}
            onChange={(e) => update({ theme: e.target.value as Settings["theme"] })}
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
      </fieldset>

      <footer className="settings-footer">
        {status === "error" && <span className="notice notice-err">{error}</span>}
        {status === "saved" && <span className="notice notice-ok">Saved</span>}
        <button
          type="button"
          className="primary"
          onClick={() => void save()}
          disabled={status === "saving"}
        >
          {status === "saving" ? "Saving…" : "Save"}
        </button>
      </footer>
    </section>
  );
}
