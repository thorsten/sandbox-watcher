import { useState } from "react";
import { openInEditor } from "../api";
import { changed, commonDir, describeFiles, fileStatus, relativePath } from "../files";
import { formatRelative } from "../network";
import type { FileTouch } from "../types";

interface Props {
  files: FileTouch[];
  /** Container to resolve the editor target; without it rows are plain text. */
  containerId?: string;
  projectDir: string | null;
  now: Date;
  /** Collapse read-only files behind a toggle. Default true. */
  collapseReads?: boolean;
}

export function FilesList({ files, containerId, projectDir, now, collapseReads = true }: Props) {
  const [showReads, setShowReads] = useState(!collapseReads);
  const [error, setError] = useState<string | null>(null);
  if (files.length === 0) return <p className="muted small">No files touched yet.</p>;

  const base = projectDir ?? commonDir(files.map((f) => f.path));
  const changedFiles = files.filter(changed);
  const readFiles = files.filter((f) => !changed(f));
  const shown = showReads ? [...changedFiles, ...readFiles] : changedFiles;

  const open = (path: string) => {
    if (!containerId) return;
    setError(null);
    openInEditor(containerId, path).catch((e: unknown) =>
      setError(e instanceof Error ? e.message : String(e)),
    );
  };

  return (
    <div className="files">
      <div className="files-head muted small">
        {describeFiles(files)}
        {readFiles.length > 0 && collapseReads && (
          <button type="button" className="link" onClick={() => setShowReads((v) => !v)}>
            {showReads ? "Hide read files" : `Show ${readFiles.length} read`}
          </button>
        )}
      </div>
      {shown.length === 0 ? (
        <p className="muted small">Nothing changed yet.</p>
      ) : (
        <ul className="file-list">
          {shown.map((f) => {
            const status = fileStatus(f);
            const label = relativePath(f.path, base);
            return (
              <li key={f.path}>
                <span className={`chip file-${status}`}>{status}</span>
                {containerId && status !== "deleted" ? (
                  <button
                    type="button"
                    className="link file-path"
                    title={`Open ${f.path}`}
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
                  {f.edits > 0 && `${f.edits}× edited`}
                  {f.edits > 0 && f.reads > 0 && " · "}
                  {f.reads > 0 && `${f.reads}× read`}
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
