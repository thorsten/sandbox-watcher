import type { FileTouch } from "./types";

export type FileStatus = "created" | "edited" | "deleted" | "read";

export function changed(f: FileTouch): boolean {
  return f.edits > 0 || f.created || f.deleted;
}

/** The most significant thing that happened to the file. */
export function fileStatus(f: FileTouch): FileStatus {
  if (f.deleted) return "deleted";
  if (f.created) return "created";
  if (f.edits > 0) return "edited";
  return "read";
}

/** Path relative to the project directory, or unchanged when outside it. */
export function relativePath(path: string, projectDir: string | null): string {
  if (!projectDir) return path;
  const root = projectDir.replace(/[/\\]+$/, "");
  if (path === root) return ".";
  for (const sep of ["/", "\\"]) {
    if (path.startsWith(root + sep)) return path.slice(root.length + 1);
  }
  return path;
}

export interface FileSummary {
  changed: number;
  read: number;
}

export function summarizeFiles(files: FileTouch[]): FileSummary {
  let c = 0;
  for (const f of files) if (changed(f)) c++;
  return { changed: c, read: files.length - c };
}

export function describeFiles(files: FileTouch[]): string {
  const s = summarizeFiles(files);
  if (s.changed === 0 && s.read === 0) return "no files";
  const parts: string[] = [];
  if (s.changed > 0) parts.push(`${s.changed} changed`);
  if (s.read > 0) parts.push(`${s.read} read`);
  return parts.join(" · ");
}

/** Longest common directory of absolute paths, used when no project dir is known. */
export function commonDir(paths: string[]): string | null {
  const first = paths[0];
  if (!first || !first.startsWith("/")) return null;
  let parts = first.split("/").slice(0, -1);
  for (const p of paths.slice(1)) {
    const other = p.split("/");
    let i = 0;
    while (i < parts.length && parts[i] === other[i]) i++;
    parts = parts.slice(0, i);
  }
  const dir = parts.join("/");
  return dir.length > 0 ? dir : null;
}
