import { useState } from "react";
import { openInEditor } from "../api";
import {
  changed,
  commonDir,
  describeFiles,
  fileStatus,
  fileStatusLabel,
  relativePath,
} from "../files";
import { t } from "../i18n";
import { formatRelative } from "../network";
import type { FileTouch } from "../types";
import { describeError } from "../errors";

interface Props {
  files: FileTouch[];
  /** Container to resolve the editor target; without it rows are plain text. */
  containerKey?: string;
  projectDir: string | null;
  now: Date;
  /** Collapse read-only files behind a toggle. Default true. */
  collapseReads?: boolean;
}

export function FilesList({ files, containerKey, projectDir, now, collapseReads = true }: Props) {
  const [showReads, setShowReads] = useState(!collapseReads);
  const [error, setError] = useState<string | null>(null);
  const canOpen = !!containerKey && !containerKey.includes("/");
  if (files.length === 0) return <p className="muted small">{t("files.noneYet")}</p>;

  const base = projectDir ?? commonDir(files.map((f) => f.path));
  const changedFiles = files.filter(changed);
  const readFiles = files.filter((f) => !changed(f));
  const shown = showReads ? [...changedFiles, ...readFiles] : changedFiles;

  const open = (path: string) => {
    if (!containerKey || !canOpen) return;
    setError(null);
    openInEditor(containerKey, path).catch((e: unknown) => setError(describeError(e)));
  };

  return (
    <div className="files">
      <div className="files-head muted small">
        {describeFiles(files)}
        {readFiles.length > 0 && collapseReads && (
          <button type="button" className="link" onClick={() => setShowReads((v) => !v)}>
            {showReads ? t("files.hideRead") : t("files.showRead", { n: readFiles.length })}
          </button>
        )}
      </div>
      {shown.length === 0 ? (
        <p className="muted small">{t("files.nothingChanged")}</p>
      ) : (
        <ul className="file-list">
          {shown.map((f) => {
            const status = fileStatus(f);
            const label = relativePath(f.path, base);
            return (
              <li key={f.path}>
                <span className={`chip file-${status}`}>{fileStatusLabel(status)}</span>
                {canOpen && status !== "deleted" ? (
                  <button
                    type="button"
                    className="link file-path"
                    title={t("files.openTitle", { path: f.path })}
                    onClick={() => open(f.path)}
                  >
                    {label}
                  </button>
                ) : (
                  <code className="file-path" title={f.path}>
                    {label}
                  </code>
                )}
                <span className="muted small file-meta">
                  {f.edits > 0 && t("files.editedCount", { n: f.edits })}
                  {f.edits > 0 && f.reads > 0 && " · "}
                  {f.reads > 0 && t("files.readCount", { n: f.reads })}
                  {f.last_at && ` · ${formatRelative(f.last_at, now)}`}
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {error && <p className="notice notice-err">{error}</p>}
    </div>
  );
}
