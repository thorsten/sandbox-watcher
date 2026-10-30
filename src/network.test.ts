import { describe, expect, it } from "vitest";
import { filterEntries, formatRelative, recentBlockedCount } from "./network";
import type { NetworkEntry, NetworkReport } from "./types";

const now = new Date("2026-10-30T12:00:00Z");

function entry(partial: Partial<NetworkEntry>): NetworkEntry {
  return {
    container_id: "c",
    destination: "example.com",
    dst_ip: null,
    port: 443,
    count: 1,
    last_seen: "2026-10-30T11:59:00Z",
    status: "blocked",
    ...partial,
  };
}

function report(entries: NetworkEntry[]): NetworkReport {
  return {
    container_id: "c",
    collected_at: now.toISOString(),
    entries,
    unparsed_sources: [],
    failed_sources: [],
  };
}

describe("recentBlockedCount", () => {
  it("sums blocked counts inside the window only", () => {
    const reports = [
      report([
        entry({ count: 3 }),
        entry({ count: 5, last_seen: "2026-10-30T11:00:00Z" }),
        entry({ count: 7, status: "allowed" }),
        entry({ count: 1, last_seen: null }),
      ]),
      report([entry({ count: 2, destination: "other" })]),
    ];
    expect(recentBlockedCount(reports, now)).toBe(5);
  });
});

describe("filterEntries", () => {
  const entries = [
    entry({ destination: "registry.npmjs.org", status: "allowed" }),
    entry({ destination: "evil.example" }),
    entry({ destination: "93.184.216.34", dst_ip: "93.184.216.34", port: 5432 }),
  ];
  it("filters by status and search", () => {
    expect(filterEntries(entries, { status: "blocked", search: "" })).toHaveLength(2);
    expect(filterEntries(entries, { status: "all", search: "NPM" })).toHaveLength(1);
    expect(filterEntries(entries, { status: "all", search: "5432" })).toHaveLength(1);
    expect(filterEntries(entries, { status: "all", search: "  " })).toHaveLength(3);
  });
});

describe("formatRelative", () => {
  it("buckets like uptime", () => {
    expect(formatRelative("2026-10-30T11:59:30Z", now)).toBe("30s ago");
    expect(formatRelative("2026-10-30T11:15:00Z", now)).toBe("45m ago");
    expect(formatRelative("2026-10-29T10:00:00Z", now)).toBe("1d ago");
    expect(formatRelative(null, now)).toBe("");
  });
});
