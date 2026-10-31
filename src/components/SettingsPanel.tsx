import { useState } from "react";
import { checkForUpdate, saveSettings } from "../api";
import { RUNTIME_LABELS, type HostStatus, type RuntimeKind, type Settings } from "../types";
import { RemoteHostsSection } from "./RemoteHostsSection";

interface Props {
  settings: Settings;
  /** Connection state of the configured remote hosts. */
  hostStatuses?: HostStatus[];
  onSaved: (settings: Settings) => void;
  onClose: () => void;
}

const RUNTIMES: RuntimeKind[] = ["docker", "podman", "apple-container"];
const isMac = navigator.platform.toLowerCase().includes("mac");

export function SettingsPanel({ settings, hostStatuses = [], onSaved, onClose }: Props) {
  const [draft, setDraft] = useState<Settings>(settings);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [error, setError] = useState<string | null>(null);
  const [pricesText, setPricesText] = useState(() => JSON.stringify(settings.prices, null, 2));
  const [pricesError, setPricesError] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);

  const checkNow = async () => {
    setUpdateStatus("Checking…");
    try {
      const found = await checkForUpdate();
      setUpdateStatus(found ? `Version ${found.version} is available.` : "You are up to date.");
    } catch (e) {
      setUpdateStatus(`Check failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

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
        <legend>Editor</legend>
        <label>
          Open files and projects with
          <select
            value={draft.editor.kind}
            onChange={(e) => {
              const kind = e.target.value as Settings["editor"]["kind"];
              update({
                editor:
                  kind === "custom"
                    ? { kind, command: draft.editor.kind === "custom" ? draft.editor.command : "" }
                    : { kind },
              });
            }}
          >
            <option value="system">System default app</option>
            <option value="vs-code">Visual Studio Code</option>
            <option value="cursor">Cursor</option>
            <option value="zed">Zed</option>
            <option value="jet-brains">JetBrains IDE (PhpStorm, IntelliJ IDEA, WebStorm, …)</option>
            <option value="custom">Custom command</option>
          </select>
        </label>
        {draft.editor.kind === "custom" && (
          <label>
            Command
            <input
              type="text"
              value={draft.editor.command}
              placeholder="subl {path}"
              onChange={(e) => update({ editor: { kind: "custom", command: e.target.value } })}
            />
            <span className="muted">
              <code>{"{path}"}</code> is replaced by the quoted file or folder.
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
        <legend>History and cost</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.history_enabled}
            onChange={(e) => update({ history_enabled: e.target.checked })}
          />
          Record sessions and containers locally
        </label>
        <label>
          Keep history for
          <input
            type="number"
            min={1}
            max={3650}
            value={draft.history_retention_days}
            onChange={(e) => update({ history_retention_days: Number(e.target.value) })}
          />
          <span className="muted">days</span>
        </label>
        <label className="stacked">
          Model prices, USD per million tokens, matched by model id prefix
          <textarea
            rows={8}
            value={pricesText}
            onChange={(e) => setPricesText(e.target.value)}
            onBlur={() => {
              try {
                const parsed = JSON.parse(pricesText) as Settings["prices"];
                update({ prices: parsed });
                setPricesError(null);
              } catch (err) {
                setPricesError(err instanceof Error ? err.message : String(err));
              }
            }}
            spellCheck={false}
          />
          {pricesError && <span className="notice notice-err">{pricesError}</span>}
          <span className="muted small">
            Pi and OpenCode report cost themselves; Claude Code and Codex are estimated from this
            table. Unknown models show tokens only.
          </span>
        </label>
      </fieldset>

      <fieldset>
        <legend>Updates</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.check_for_updates}
            onChange={(e) => update({ check_for_updates: e.target.checked })}
          />
          Check for new releases on start and every six hours
        </label>
        <div className="settings-row">
          <button type="button" onClick={() => void checkNow()}>
            Check now
          </button>
          {updateStatus && <span className="muted small">{updateStatus}</span>}
        </div>
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

      <RemoteHostsSection
        hosts={draft.remote_hosts}
        statuses={hostStatuses}
        onChange={(remote_hosts) => update({ remote_hosts })}
      />

      <footer className="settings-footer">
        {status === "error" && <span className="notice notice-err">{error}</span>}
        {status === "saved" && <span className="notice notice-ok">Saved</span>}
        <button
          type="button"
          className="primary"
          onClick={() => void save()}
          disabled={status === "saving" || pricesError !== null}
        >
          {status === "saving" ? "Saving…" : "Save"}
        </button>
      </footer>
    </section>
  );
}
