import { useEffect, useState } from "react";
import { applyAllow, previewAllow } from "../api";
import { t, tx } from "../i18n";
import type { AllowPreview, AllowResult, ConfigTarget } from "../types";
import { describeError, isCommandError } from "../errors";

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
  const label = target === "global" ? t("allow.global") : t("allow.project");
  const fallback = target === "global" ? t("allow.globalMissing") : t("allow.projectMissing");
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
      (e) => !cancelled && setError(describeError(e)),
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
      if (isCommandError(e, "config-changed")) {
        setError(t("allow.changedRetry", { message: describeError(e) }));
        try {
          setPreview(await previewAllow(containerKey, selections));
        } catch (e2) {
          setError(describeError(e2));
        }
      } else {
        setError(t("allow.maybeWritten", { message: describeError(e) }));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog open className="confirm allow-dialog" aria-labelledby="allow-title">
      <h3 id="allow-title">{alias ? t("allow.titleOn", { host: alias }) : t("allow.title")}</h3>
      {error && <p className="network-note warn">{error}</p>}
      {!preview && !error && <p className="muted">{t("allow.checking")}</p>}

      {preview && !result && (
        <>
          <p className="muted small">
            {tx("allow.intro", {
              list: <code>allow_network</code>,
              command: <code>sandbox network allow</code>,
            })}
          </p>
          <fieldset>
            <legend>{t("allow.writeTo")}</legend>
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
          <p>{t("allow.entries")}</p>
          {toAdd.length === 0 ? (
            <p className="muted">{t("allow.allPresent")}</p>
          ) : (
            <pre>{toAdd.map((e) => `  "${e}",`).join("\n")}</pre>
          )}
          {already.length > 0 && (
            <p className="muted small">{t("allow.alreadyPresent", { list: already.join(", ") })}</p>
          )}
          <div className="confirm-actions">
            <button type="button" onClick={() => onClose(false)} disabled={busy}>
              {t("confirm.cancel")}
            </button>
            <button
              type="button"
              className="primary"
              onClick={() => void apply()}
              disabled={busy || toAdd.length === 0 || !path || unavailable !== null}
            >
              {busy
                ? t("allow.writing")
                : target === "global"
                  ? t("allow.addGlobal", { n: toAdd.length })
                  : t("allow.addProject", { n: toAdd.length })}
            </button>
          </div>
        </>
      )}

      {result && (
        <>
          <p>{tx("allow.added", { n: result.added.length, path: <code>{result.path}</code> })}</p>
          {result.added.length > 0 && <pre>{result.added.map((e) => `  "${e}",`).join("\n")}</pre>}
          <p className="muted small">{t("allow.restartHint")}</p>
          <div className="confirm-actions">
            <button type="button" className="primary" onClick={() => onClose(true)}>
              {t("allow.done")}
            </button>
          </div>
        </>
      )}
    </dialog>
  );
}
