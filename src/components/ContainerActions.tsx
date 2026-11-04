import { useState } from "react";
import { getProjectPath, openInEditor, openProject, openTerminal, stopContainer } from "../api";
import { confirmStopEnabled, copyText, describeInterruption, networkLogsCommand } from "../actions";
import { isRemote } from "../hosts";
import { t, tx } from "../i18n";
import { disableConfirmStop } from "../settingsActions";
import type { ContainerOverview } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";
import { describeError } from "../errors";

interface Props {
  container: ContainerOverview;
  onChanged: () => void;
}

export function ContainerActions({ container, onChanged }: Props) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [copyOpen, setCopyOpen] = useState(false);
  // A greyed container of an unreachable host cannot be stopped.
  const isRunning = container.state === "running" && !container.last_seen_at;
  const remote = isRemote(container);

  const flash = (kind: "ok" | "err", text: string) => {
    setNotice({ kind, text });
    window.setTimeout(() => setNotice(null), kind === "ok" ? 1800 : 6000);
  };

  const run = async (label: string, action: () => Promise<unknown>, okText?: string) => {
    setBusy(label);
    try {
      await action();
      if (okText) flash("ok", okText);
      onChanged();
    } catch (e) {
      flash("err", describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const doStop = () => run("stop", () => stopContainer(container.key), t("actions.stoppedNotice"));

  const requestStop = () => {
    if (confirmStopEnabled()) setConfirming(true);
    else void doStop();
  };

  const copy = async (what: "name" | "path" | "network") => {
    setCopyOpen(false);
    let text: string;
    if (what === "name") text = container.name;
    else {
      const dir = await getProjectPath(container.key).catch(() => null);
      if (what === "path") {
        if (!dir) return flash("err", t("actions.noProjectDir"));
        text = dir;
      } else text = networkLogsCommand(dir, container.host);
    }
    const ok = await copyText(text);
    flash(ok ? "ok" : "err", ok ? t("actions.copied") : t("actions.clipboardUnavailable"));
  };

  return (
    <div className="actions">
      {remote ? (
        <span className="muted" title={t("actions.remoteTitle")}>
          {t("actions.remoteHost")}
        </span>
      ) : (
        <>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void run("terminal", () => openTerminal(container.key))}
            title={t("actions.terminalTitle")}
          >
            {t("actions.terminal")}
          </button>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void run("folder", () => openProject(container.key))}
            title={t("actions.folderTitle")}
          >
            {t("actions.folder")}
          </button>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void run("editor", () => openInEditor(container.key))}
            title={t("actions.editorTitle")}
          >
            {t("actions.editor")}
          </button>
        </>
      )}
      <span className="menu">
        <button type="button" onClick={() => setCopyOpen((v) => !v)} aria-expanded={copyOpen}>
          {t("actions.copy")}
        </button>
        {copyOpen && (
          <ul className="menu-list">
            <li>
              <button type="button" onClick={() => void copy("name")}>
                {t("actions.copyName")}
              </button>
            </li>
            <li>
              <button type="button" onClick={() => void copy("path")}>
                {t("actions.copyPath")}
              </button>
            </li>
            <li>
              <button type="button" onClick={() => void copy("network")}>
                {tx("actions.copyNetwork", { command: <code>sandbox network logs</code> })}
              </button>
            </li>
          </ul>
        )}
      </span>
      {isRunning && (
        <button type="button" className="danger" disabled={busy !== null} onClick={requestStop}>
          {busy === "stop" ? t("stop.stopping") : t("actions.stop")}
        </button>
      )}
      {notice && <span className={`notice notice-${notice.kind}`}>{notice.text}</span>}
      {confirming && (
        <ConfirmDialog
          title={t("actions.stopTitle", { name: container.name })}
          lines={describeInterruption([container])}
          confirmLabel={t("actions.stopContainer")}
          busy={busy === "stop"}
          onCancel={() => setConfirming(false)}
          onConfirm={(dontAsk) => {
            if (dontAsk) void disableConfirmStop();
            setConfirming(false);
            void doStop();
          }}
        />
      )}
    </div>
  );
}
