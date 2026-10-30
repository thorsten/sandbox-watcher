/** Percentage for the download bar, or null when the size is unknown. */
export function progressPercent(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((downloaded / total) * 100)));
}

/** First lines of release notes, without Markdown headings and commit links. */
export function summarizeNotes(notes: string | null, maxLines = 6): string[] {
  if (!notes) return [];
  return notes
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("- "))
    .map((l) =>
      l
        .slice(2)
        .replace(/\s*\(\[[0-9a-f]{7}\]\([^)]*\)\)\s*$/, "")
        .replace(/\*\*([^*]+):\*\*\s*/, "$1: "),
    )
    .slice(0, maxLines);
}
