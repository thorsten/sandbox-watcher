import { useEffect, useState } from "react";
import { applyAllow, previewAllow } from "../api";
import type { AllowPreview, AllowResult, ConfigTarget } from "../types";

interface Props {
  containerKey: string;
  selections: { host: string; ports: number[] }[];
  onClose: (changed: boolean) => void;
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
        if (p.project_path && !p.global_path) setTarget("project");
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

  const apply = async () => {
    setBusy(true);
    try {
      setResult(await applyAllow(containerKey, target, toAdd));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog open className="confirm allow-dialog" aria-labelledby="allow-title">
      <h3 id="allow-title">Allow blocked destinations</h3>
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
            <label className="check">
              <input
                type="radio"
                name="target"
                checked={target === "global"}
                disabled={!preview.global_path}
                onChange={() => setTarget("global")}
              />
              Global config <code className="muted">{preview.global_path ?? "not available"}</code>
            </label>
            <label className="check">
              <input
                type="radio"
                name="target"
                checked={target === "project"}
                disabled={!preview.project_path}
                onChange={() => setTarget("project")}
              />
              Project config{" "}
              <code className="muted">
                {preview.project_path ?? "no .sandbox directory in this project"}
              </code>
            </label>
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
              disabled={busy || toAdd.length === 0 || !path}
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
