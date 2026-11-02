import { useState } from "react";
import { installUpdate } from "../api";
import { t } from "../i18n";
import type { UpdateInfo } from "../types";
import { progressPercent, summarizeNotes } from "../updateFormat";

interface Props {
  update: UpdateInfo;
  progress: { downloaded: number; total: number | null } | null;
  onDismiss: () => void;
}

export function UpdateBanner({ update, progress, onDismiss }: Props) {
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const notes = summarizeNotes(update.notes);
  const percent = progress ? progressPercent(progress.downloaded, progress.total) : null;

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      await installUpdate();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setInstalling(false);
    }
  };

  return (
    <section className="update-banner" aria-live="polite">
      <div className="update-head">
        <strong>{t("update.available", { version: update.version })}</strong>
        <span className="muted"> · {t("update.current", { version: update.current_version })}</span>
      </div>
      {notes.length > 0 && (
        <ul className="update-notes">
          {notes.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      {installing && (
        <div className="update-progress">
          <progress className="update-progress-bar" max={100} value={percent ?? undefined} />
          <span className="muted small">
            {percent === null
              ? t("update.downloading")
              : t("update.downloadingPercent", { percent })}{" "}
            {t("update.restarts")}
          </span>
        </div>
      )}
      {error && <p className="notice notice-err">{error}</p>}
      <div className="update-actions">
        <button type="button" onClick={onDismiss} disabled={installing}>
          {t("update.later")}
        </button>
        <button
          type="button"
          className="primary"
          onClick={() => void install()}
          disabled={installing}
        >
          {installing ? t("update.installing") : t("update.install")}
        </button>
      </div>
    </section>
  );
}
