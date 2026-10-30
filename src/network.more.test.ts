import { describe, expect, it } from "vitest";
import { filterEntries, formatRelative, RECENT_WINDOW_MS, recentBlockedCount } from "./network";
import { entry, NOW, report } from "./test/fixtures";

describe("recentBlockedCount", () => {
  it("is zero without reports", () => {
    expect(recentBlockedCount([], NOW)).toBe(0);
  });
  it("respects a custom window", () => {
    const r = [report([entry({ count: 4, last_seen: "2026-10-30T11:50:00Z" })])];
    expect(recentBlockedCount(r, NOW)).toBe(0);
    expect(recentBlockedCount(r, NOW, 15 * 60 * 1000)).toBe(4);
  });
  it("counts an entry exactly at the window edge", () => {
    const edge = new Date(NOW.getTime() - RECENT_WINDOW_MS).toISOString();
    expect(recentBlockedCount([report([entry({ count: 2, last_seen: edge })])], NOW)).toBe(2);
  });
});

describe("filterEntries", () => {
  const entries = [
    entry({ destination: "registry.npmjs.org", status: "allowed" }),
    entry({ destination: "api.Example.com" }),
    entry({ destination: "10.0.0.5", dst_ip: "10.0.0.5", port: 5432 }),
  ];
  it("matches case-insensitively", () => {
    expect(filterEntries(entries, { status: "all", search: "EXAMPLE" })).toHaveLength(1);
  });
  it("matches by IP", () => {
    expect(filterEntries(entries, { status: "all", search: "10.0" })).toHaveLength(1);
  });
  it("combines status and search", () => {
    expect(filterEntries(entries, { status: "blocked", search: "npm" })).toHaveLength(0);
  });
  it("does not mutate the input", () => {
    const copy = [...entries];
    filterEntries(entries, { status: "blocked", search: "" });
    expect(entries).toEqual(copy);
  });
});

describe("formatRelative", () => {
  it("clamps future timestamps to zero", () => {
    expect(formatRelative("2026-10-30T12:05:00Z", NOW)).toBe("0s ago");
  });
  it("uses hours below a day", () => {
    expect(formatRelative("2026-10-30T09:00:00Z", NOW)).toBe("3h ago");
  });
});
