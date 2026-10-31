import { useState } from "react";
import { getProjectPath, openInEditor, openProject, openTerminal, stopContainer } from "../api";
import { confirmStopEnabled, copyText, describeInterruption, networkLogsCommand } from "../actions";
import { isRemote } from "../hosts";
import { disableConfirmStop } from "../settingsActions";
import type { SandboxContainer } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

interface Props {
  container: SandboxContainer;
  onChanged: () => void;
}

export function ContainerActions({ container, onChanged }: Props) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [copyOpen, setCopyOpen] = useState(false);
  const isRunning = container.state === "running";
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
      flash("err", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const doStop = () => run("stop", () => stopContainer(container.key), "Stopped");

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
        if (!dir) return flash("err", "No bind-mounted project directory");
        text = dir;
      } else text = networkLogsCommand(dir, container.host);
    }
    const ok = await copyText(text);
    flash(ok ? "ok" : "err", ok ? "Copied" : "Clipboard unavailable");
  };

  return (
    <div className="actions">
      {remote ? (
        <span className="muted" title="Not available for remote hosts">
          remote host
        </span>
      ) : (
        <>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void run("terminal", () => openTerminal(container.key))}
            title="Open a terminal in the project directory"
          >
            Terminal
          </button>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void run("folder", () => openProject(container.key))}
            title="Open the project directory"
          >
            Folder
          </button>
          <button
            type="button"
            disabled={busy !== null}
            onClick={() => void run("editor", () => openInEditor(container.key))}
            title="Open the project in your editor"
          >
            Editor
          </button>
        </>
      )}
      <span className="menu">
        <button type="button" onClick={() => setCopyOpen((v) => !v)} aria-expanded={copyOpen}>
          Copy ▾
        </button>
        {copyOpen && (
          <ul className="menu-list">
            <li>
              <button type="button" onClick={() => void copy("name")}>
                Container name
              </button>
            </li>
            <li>
              <button type="button" onClick={() => void copy("path")}>
                Project path
              </button>
            </li>
            <li>
              <button type="button" onClick={() => void copy("network")}>
                `sandbox network logs` command
              </button>
            </li>
          </ul>
        )}
      </span>
      {isRunning && (
        <button type="button" className="danger" disabled={busy !== null} onClick={requestStop}>
          {busy === "stop" ? "Stopping…" : "Stop"}
        </button>
      )}
      {notice && <span className={`notice notice-${notice.kind}`}>{notice.text}</span>}
      {confirming && (
        <ConfirmDialog
          title={`Stop ${container.name}?`}
          lines={describeInterruption([container])}
          confirmLabel="Stop container"
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
