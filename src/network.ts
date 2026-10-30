import type { NetworkEntry, NetworkReport } from "./types";

export const RECENT_WINDOW_MS = 5 * 60 * 1000;

/** Blocked requests whose last sighting falls inside the recent window. */
export function recentBlockedCount(
  reports: NetworkReport[],
  now: Date,
  windowMs: number = RECENT_WINDOW_MS,
): number {
  const since = now.getTime() - windowMs;
  let count = 0;
  for (const report of reports) {
    for (const e of report.entries) {
      if (e.status !== "blocked" || !e.last_seen) continue;
      if (Date.parse(e.last_seen) >= since) count += e.count;
    }
  }
  return count;
}

export interface NetworkFilter {
  status: "all" | "blocked";
  search: string;
}

export function filterEntries(entries: NetworkEntry[], filter: NetworkFilter): NetworkEntry[] {
  const needle = filter.search.trim().toLowerCase();
  return entries.filter((e) => {
    if (filter.status === "blocked" && e.status !== "blocked") return false;
    if (!needle) return true;
    return (
      e.destination.toLowerCase().includes(needle) ||
      (e.dst_ip?.includes(needle) ?? false) ||
      String(e.port).includes(needle)
    );
  });
}

export function formatRelative(iso: string | null, now: Date): string {
  if (!iso) return "";
  const diff = Math.max(0, now.getTime() - Date.parse(iso));
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}
