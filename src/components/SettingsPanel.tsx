import { useState, type ReactNode } from "react";
import { checkForUpdate, saveSettings } from "../api";
import { t, tx, type PlainKey } from "../i18n";
import { RUNTIME_LABELS, type HostStatus, type RuntimeKind, type Settings } from "../types";
import { DiagnosticsSection } from "./DiagnosticsSection";
import { RemoteHostsSection } from "./RemoteHostsSection";
import { describeError } from "../errors";

interface Props {
  settings: Settings;
  /** Connection state of the configured remote hosts. */
  hostStatuses?: HostStatus[];
  /** Section shown first; the runtime banner opens Diagnostics directly. */
  initialSection?: SettingsSection;
  onSaved: (settings: Settings) => void;
  onClose: () => void;
}

export type SettingsSection =
  | "general"
  | "runtime"
  | "actions"
  | "notifications"
  | "history"
  | "diagnostics";
const SECTIONS: SettingsSection[] = [
  "general",
  "runtime",
  "actions",
  "notifications",
  "history",
  "diagnostics",
];
const NAV_LABELS: Record<SettingsSection, PlainKey> = {
  general: "nav.general",
  runtime: "nav.runtime",
  actions: "settings.actions",
  notifications: "settings.notifications",
  history: "nav.history",
  diagnostics: "runtime.diagnostics",
};

const RUNTIMES: RuntimeKind[] = ["docker", "podman", "apple-container"];
const isMac = navigator.platform.toLowerCase().includes("mac");

export function SettingsPanel({
  settings,
  hostStatuses = [],
  initialSection = "general",
  onSaved,
  onClose,
}: Props) {
  const [section, setSection] = useState<SettingsSection>(initialSection);
  const [draft, setDraft] = useState<Settings>(settings);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [error, setError] = useState<string | null>(null);
  const [pricesText, setPricesText] = useState(() => JSON.stringify(settings.prices, null, 2));
  const [pricesError, setPricesError] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);

  const checkNow = async () => {
    setUpdateStatus(t("settings.checking"));
    try {
      const found = await checkForUpdate();
      setUpdateStatus(
        found ? t("settings.updateAvailable", { version: found.version }) : t("settings.upToDate"),
      );
    } catch (e) {
      setUpdateStatus(t("settings.checkFailed", { error: describeError(e) }));
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
      setError(describeError(e));
      setStatus("error");
    }
  };

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);
  const terminalKind = draft.terminal.kind;
  const customCommand = draft.terminal.kind === "custom" ? draft.terminal.command : "";

  const sections: Record<SettingsSection, ReactNode> = {
    general: (
      <>
        <fieldset>
          <legend>{t("settings.appearance")}</legend>
          <label>
            {t("settings.language")}
            <select
              value={draft.language}
              onChange={(e) => update({ language: e.target.value as Settings["language"] })}
            >
              <option value="system">{t("language.system")}</option>
              <option value="en">English</option>
              <option value="de">Deutsch</option>
            </select>
          </label>
          <label>
            {t("settings.theme")}
            <select
              value={draft.theme}
              onChange={(e) => update({ theme: e.target.value as Settings["theme"] })}
            >
              <option value="system">{t("theme.system")}</option>
              <option value="light">{t("theme.light")}</option>
              <option value="dark">{t("theme.dark")}</option>
            </select>
          </label>
        </fieldset>
        <fieldset>
          <legend>{t("settings.window")}</legend>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.close_to_tray}
              onChange={(e) => update({ close_to_tray: e.target.checked })}
            />
            {t("settings.closeToTray")}
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.start_minimized}
              onChange={(e) => update({ start_minimized: e.target.checked })}
            />
            {t("settings.startMinimized")}
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.launch_at_login}
              onChange={(e) => update({ launch_at_login: e.target.checked })}
            />
            {t("settings.launchAtLogin")}
          </label>
        </fieldset>
        <fieldset>
          <legend>{t("settings.updates")}</legend>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.check_for_updates}
              onChange={(e) => update({ check_for_updates: e.target.checked })}
            />
            {t("settings.checkUpdates")}
          </label>
          <div className="settings-row">
            <button type="button" onClick={() => void checkNow()}>
              {t("settings.checkNow")}
            </button>
            {updateStatus && <span className="muted small">{updateStatus}</span>}
          </div>
        </fieldset>
      </>
    ),
    runtime: (
      <>
        <fieldset>
          <legend>{t("settings.refresh")}</legend>
          <label>
            {t("settings.pollInterval")}
            <input
              type="number"
              min={2}
              max={300}
              value={draft.poll_interval_secs}
              onChange={(e) => update({ poll_interval_secs: Number(e.target.value) })}
            />
            <span className="muted">{t("settings.pollHint")}</span>
          </label>
          <label>
            {t("settings.networkInterval")}
            <input
              type="number"
              min={2}
              max={600}
              value={draft.network_poll_interval_secs}
              onChange={(e) => update({ network_poll_interval_secs: Number(e.target.value) })}
            />
            <span className="muted">{t("settings.networkHint")}</span>
          </label>
          <label>
            {t("settings.runtime")}
            <select
              value={draft.runtime_override ?? ""}
              onChange={(e) =>
                update({ runtime_override: (e.target.value || null) as RuntimeKind | null })
              }
            >
              <option value="">{t("settings.runtimeFollow")}</option>
              {RUNTIMES.map((r) => (
                <option key={r} value={r}>
                  {t("settings.runtimeOnly", { runtime: RUNTIME_LABELS[r] })}
                </option>
              ))}
            </select>
          </label>
        </fieldset>
        <RemoteHostsSection
          hosts={draft.remote_hosts}
          statuses={hostStatuses}
          onChange={(remote_hosts) => update({ remote_hosts })}
        />
      </>
    ),
    actions: (
      <>
        <fieldset>
          <legend>{t("settings.actions")}</legend>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.confirm_stop}
              onChange={(e) => update({ confirm_stop: e.target.checked })}
            />
            {t("settings.confirmStop")}
          </label>
          <label>
            {t("settings.terminal")}
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
                {isMac ? t("settings.terminalAutoMac") : t("settings.terminalAuto")}
              </option>
              {isMac && <option value="iterm">iTerm2</option>}
              {isMac && <option value="terminal-app">Terminal.app</option>}
              <option value="custom">{t("settings.customCommand")}</option>
            </select>
          </label>
          {terminalKind === "custom" && (
            <label>
              {t("settings.command")}
              <input
                type="text"
                value={customCommand}
                placeholder="wezterm start --cwd {dir}"
                onChange={(e) => update({ terminal: { kind: "custom", command: e.target.value } })}
              />
              <span className="muted">
                {tx("settings.dirHint", { placeholder: <code>{"{dir}"}</code> })}
              </span>
            </label>
          )}
        </fieldset>
        <fieldset>
          <legend>{t("settings.editor")}</legend>
          <label>
            {t("settings.openWith")}
            <select
              value={draft.editor.kind}
              onChange={(e) => {
                const kind = e.target.value as Settings["editor"]["kind"];
                update({
                  editor:
                    kind === "custom"
                      ? {
                          kind,
                          command: draft.editor.kind === "custom" ? draft.editor.command : "",
                        }
                      : { kind },
                });
              }}
            >
              <option value="system">{t("settings.editorSystem")}</option>
              <option value="vs-code">Visual Studio Code</option>
              <option value="cursor">Cursor</option>
              <option value="zed">Zed</option>
              <option value="jet-brains">
                JetBrains IDE (PhpStorm, IntelliJ IDEA, WebStorm, …)
              </option>
              <option value="custom">{t("settings.customCommand")}</option>
            </select>
          </label>
          {draft.editor.kind === "custom" && (
            <label>
              {t("settings.command")}
              <input
                type="text"
                value={draft.editor.command}
                placeholder="subl {path}"
                onChange={(e) => update({ editor: { kind: "custom", command: e.target.value } })}
              />
              <span className="muted">
                {tx("settings.pathHint", { placeholder: <code>{"{path}"}</code> })}
              </span>
            </label>
          )}
        </fieldset>
      </>
    ),
    notifications: (
      <fieldset>
        <legend>{t("settings.notifications")}</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.notifications.enabled}
            onChange={(e) => updateNotify({ enabled: e.target.checked })}
          />
          {t("settings.enableNotifications")}
        </label>
        <div className="settings-grid">
          {(
            [
              "session_started",
              "session_ended",
              "container_started",
              "container_stopped",
              "container_died",
              "blocked_burst",
              "stale_container",
            ] as const
          ).map((key) => (
            <label key={key} className="check">
              <input
                type="checkbox"
                disabled={!draft.notifications.enabled}
                checked={draft.notifications[key]}
                onChange={(e) => updateNotify({ [key]: e.target.checked })}
              />
              {t(`notify.${key}`)}
            </label>
          ))}
        </div>
        <label>
          {t("settings.burstThreshold")}
          <input
            type="number"
            min={1}
            max={10000}
            value={draft.blocked_burst_threshold}
            onChange={(e) => update({ blocked_burst_threshold: Number(e.target.value) })}
          />
          <span className="muted">{t("settings.requestsWithin")}</span>
          <input
            type="number"
            min={10}
            max={3600}
            value={draft.blocked_burst_window_secs}
            onChange={(e) => update({ blocked_burst_window_secs: Number(e.target.value) })}
          />
          <span className="muted">{t("settings.seconds")}</span>
        </label>
      </fieldset>
    ),
    history: (
      <fieldset>
        <legend>{t("settings.history")}</legend>
        <label className="check">
          <input
            type="checkbox"
            checked={draft.history_enabled}
            onChange={(e) => update({ history_enabled: e.target.checked })}
          />
          {t("settings.recordHistory")}
        </label>
        <label>
          {t("settings.keepFor")}
          <input
            type="number"
            min={1}
            max={3650}
            value={draft.history_retention_days}
            onChange={(e) => update({ history_retention_days: Number(e.target.value) })}
          />
          <span className="muted">{t("settings.days")}</span>
        </label>
        <label className="stacked">
          {t("settings.prices")}
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
                setPricesError(describeError(err));
              }
            }}
            spellCheck={false}
          />
          {pricesError && <span className="notice notice-err">{pricesError}</span>}
          <span className="muted small">{t("settings.pricesHint")}</span>
        </label>
      </fieldset>
    ),
    diagnostics: <DiagnosticsSection />,
  };

  return (
    <section className="settings" aria-labelledby="settings-title">
      <header className="settings-header">
        <h2 id="settings-title">{t("app.settings")}</h2>
        <button
          type="button"
          className="close"
          onClick={onClose}
          title={t("settings.close")}
          aria-label={t("settings.close")}
        >
          ×
        </button>
      </header>

      <div className="settings-body">
        <nav className="settings-nav" aria-label={t("settings.navLabel")}>
          {SECTIONS.map((id) => (
            <button
              key={id}
              type="button"
              className={section === id ? "active" : ""}
              aria-current={section === id ? "page" : undefined}
              onClick={() => setSection(id)}
            >
              {t(NAV_LABELS[id])}
            </button>
          ))}
        </nav>
        <div className="settings-content">{sections[section]}</div>
      </div>

      <footer className="settings-footer">
        {status === "error" && <span className="notice notice-err">{error}</span>}
        {status === "saved" && <span className="notice notice-ok">{t("settings.saved")}</span>}
        {status === "idle" && dirty && <span className="muted small">{t("settings.unsaved")}</span>}
        <button
          type="button"
          className="primary"
          onClick={() => void save()}
          disabled={status === "saving" || pricesError !== null}
        >
          {status === "saving" ? t("settings.saving") : t("settings.save")}
        </button>
      </footer>
    </section>
  );
}
