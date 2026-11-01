import { useEffect, useState } from "react";
import { applyAllow, previewAllow } from "../api";
import type { AllowPreview, AllowResult, ConfigTarget } from "../types";

interface Props {
  containerKey: string;
  selections: { host: string; ports: number[] }[];
  onClose: (changed: boolean) => void;
}

export function TargetChoice({
  target,
  selected,
  path,
  unavailable,
  onSelect,
}: {
  target: ConfigTarget;
  selected: ConfigTarget;
  path: string | null;
  unavailable: string | null;
  onSelect: (t: ConfigTarget) => void;
}) {
  const label = target === "global" ? "Global config" : "Project config";
  const fallback = target === "global" ? "not available" : "no .sandbox directory in this project";
  return (
    <label className="check">
      <input
        type="radio"
        name="target"
        checked={selected === target}
        disabled={!path || unavailable !== null}
        onChange={() => onSelect(target)}
      />
      {label} <code className="muted">{path ?? fallback}</code>
      {unavailable && <span className="muted small"> {unavailable}</span>}
    </label>
  );
}

export function AllowDialog({ containerKey, selections, onClose }: Props) {
  const [preview, setPreview] = useState<AllowPreview | null>(null);
  const [target, setTarget] = useState<ConfigTarget>("global");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AllowResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    previewAllow(containerKey, selections).then(
      (p) => {
        if (cancelled) return;
        setPreview(p);
        const globalUsable = p.global_path && !p.global_unavailable;
        const projectUsable = p.project_path && !p.project_unavailable;
        if (!globalUsable && projectUsable) setTarget("project");
      },
      (e) => !cancelled && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [containerKey, selections]);

  const already = preview
    ? target === "global"
      ? preview.already_in_global
      : preview.already_in_project
    : [];
  const toAdd = preview ? preview.entries.filter((e) => !already.includes(e)) : [];
  const path = preview ? (target === "global" ? preview.global_path : preview.project_path) : null;
  const unavailable = preview
    ? target === "global"
      ? preview.global_unavailable
      : preview.project_unavailable
    : null;
  const alias = containerKey.includes("/") ? containerKey.split("/")[0] : null;

  const apply = async () => {
    setBusy(true);
    try {
      const hash = target === "global" ? preview?.global_hash : preview?.project_hash;
      setResult(await applyAllow(containerKey, target, toAdd, hash ?? null));
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (message.startsWith("changed:")) {
        setError(
          `${message.slice("changed:".length).trim()}. The preview was refreshed; check the entries and try again.`,
        );
        try {
          setPreview(await previewAllow(containerKey, selections));
        } catch (e2) {
          setError(e2 instanceof Error ? e2.message : String(e2));
        }
      } else {
        setError(
          `${message} If the write may have gone through, close and open the dialog again to see the current config.`,
        );
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog open className="confirm allow-dialog" aria-labelledby="allow-title">
      <h3 id="allow-title">Allow blocked destinations{alias && ` on ${alias}`}</h3>
      {error && <p className="network-note warn">{error}</p>}
      {!preview && !error && <p className="muted">Checking configuration…</p>}

      {preview && !result && (
        <>
          <p className="muted small">
            This appends entries to sandbox's <code>allow_network</code> list, the same edit{" "}
            <code>sandbox network allow</code> makes. It takes effect for new containers; restart
            the sandbox afterwards.
          </p>
          <fieldset>
            <legend>Write to</legend>
            <TargetChoice
              target="global"
              selected={target}
              path={preview.global_path}
              unavailable={preview.global_unavailable}
              onSelect={setTarget}
            />
            <TargetChoice
              target="project"
              selected={target}
              path={preview.project_path}
              unavailable={preview.project_unavailable}
              onSelect={setTarget}
            />
          </fieldset>
          <p>Entries to add:</p>
          {toAdd.length === 0 ? (
            <p className="muted">Everything selected is already configured here.</p>
          ) : (
            <pre>{toAdd.map((e) => `  "${e}",`).join("\n")}</pre>
          )}
          {already.length > 0 && (
            <p className="muted small">Already present: {already.join(", ")}</p>
          )}
          <div className="confirm-actions">
            <button type="button" onClick={() => onClose(false)} disabled={busy}>
              Cancel
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => void apply()}
              disabled={busy || toAdd.length === 0 || !path || unavailable !== null}
            >
              {busy ? "Writing…" : `Add ${toAdd.length} to ${target} config`}
            </button>
          </div>
        </>
      )}

      {result && (
        <>
          <p>
            Added {result.added.length} entr{result.added.length === 1 ? "y" : "ies"} to{" "}
            <code>{result.path}</code>.
          </p>
          {result.added.length > 0 && <pre>{result.added.map((e) => `  "${e}",`).join("\n")}</pre>}
          <p className="muted small">
            Running containers keep their current policy. Stop the sandbox and start it again to
            apply.
          </p>
          <div className="confirm-actions">
            <button type="button" className="primary" onClick={() => onClose(true)}>
              Done
            </button>
          </div>
        </>
      )}
    </dialog>
  );
}
