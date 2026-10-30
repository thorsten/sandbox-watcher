import { describe, expect, it } from "vitest";
import { AGENT_LABELS, formatUptime, groupByProject, staleHashes, summarize } from "./format";
import { container, NOW, session } from "./test/fixtures";

describe("formatUptime edge cases", () => {
  it("handles exact bucket boundaries", () => {
    expect(formatUptime("2026-10-30T11:59:00Z", NOW)).toBe("Up 1 minutes");
    expect(formatUptime("2026-10-30T11:00:00Z", NOW)).toBe("Up 1 hours");
    expect(formatUptime("2026-10-29T12:00:00Z", NOW)).toBe("Up 1 days");
  });
  it("uses the current time by default", () => {
    expect(formatUptime(new Date().toISOString())).toMatch(/^Up \d+ seconds$/);
  });
});

describe("groupByProject", () => {
  it("returns an empty map for no containers", () => {
    expect(groupByProject([]).size).toBe(0);
  });
  it("keeps first-seen project order", () => {
    const groups = groupByProject([
      container({ id: "1", project_slug: "b" }),
      container({ id: "2", project_slug: "a" }),
      container({ id: "3", project_slug: "b" }),
    ]);
    expect([...groups.keys()]).toEqual(["b", "a"]);
  });
});

describe("staleHashes", () => {
  it("ignores containers without hash", () => {
    const stale = staleHashes([
      container({ id: "1", config_hash: null }),
      container({ id: "2", config_hash: "x" }),
    ]);
    expect(stale.size).toBe(0);
  });
  it("needs a running container to decide what is current", () => {
    const stale = staleHashes([
      container({ id: "1", config_hash: "a", state: "exited" }),
      container({ id: "2", config_hash: "b", state: "exited" }),
    ]);
    expect(stale.size).toBe(0);
  });
  it("judges each project independently", () => {
    const stale = staleHashes([
      container({
        id: "1",
        project_slug: "x",
        config_hash: "a",
        started_at: "2026-10-30T08:00:00Z",
      }),
      container({
        id: "2",
        project_slug: "x",
        config_hash: "b",
        started_at: "2026-10-30T09:00:00Z",
      }),
      container({
        id: "3",
        project_slug: "y",
        config_hash: "a",
        started_at: "2026-10-30T07:00:00Z",
      }),
      container({
        id: "4",
        project_slug: "y",
        config_hash: "c",
        started_at: "2026-10-30T06:00:00Z",
      }),
    ]);
    expect([...stale].toSorted()).toEqual(["1", "4"]);
  });
});

describe("summarize", () => {
  it("counts zero for an empty overview", () => {
    const s = summarize([]);
    expect(s).toMatchObject({ projects: 0, running: 0, stopped: 0, sessions: 0 });
    expect(s.byAgent.size).toBe(0);
  });
  it("orders agents by count, ties by label", () => {
    const s = summarize([
      container({
        sessions: [
          session({ pid: 1, agent: "pi" }),
          session({ pid: 2, agent: "codex" }),
          session({ pid: 3, agent: "shell" }),
          session({ pid: 4, agent: "shell" }),
        ],
      }),
    ]);
    expect([...s.byAgent.keys()]).toEqual(["shell", "codex", "pi"]);
  });
  it("has a label for every agent kind", () => {
    for (const label of Object.values(AGENT_LABELS)) expect(label.length).toBeGreaterThan(0);
    expect(AGENT_LABELS["open-code"]).toBe("OpenCode");
  });
});
