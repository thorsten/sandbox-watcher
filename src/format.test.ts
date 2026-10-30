import { describe, expect, it } from "vitest";
import { formatUptime, groupByProject, staleHashes, summarize } from "./format";
import type { SandboxContainer } from "./types";

const now = new Date("2026-10-30T12:00:00Z");

function container(partial: Partial<SandboxContainer> & { id: string }): SandboxContainer {
  return {
    name: `sandbox-${partial.project_slug ?? "p"}`,
    project_slug: "p",
    image: "img",
    state: "running",
    created_at: null,
    started_at: null,
    config_hash: null,
    labels: {},
    readiness: "unknown",
    sessions: [],
    ...partial,
  };
}

describe("formatUptime", () => {
  it("matches sandbox status buckets", () => {
    expect(formatUptime(null, now)).toBe("unknown");
    expect(formatUptime("2026-10-30T11:59:30Z", now)).toBe("Up 30 seconds");
    expect(formatUptime("2026-10-30T11:15:00Z", now)).toBe("Up 45 minutes");
    expect(formatUptime("2026-10-30T07:00:00Z", now)).toBe("Up 5 hours");
    expect(formatUptime("2026-10-27T12:00:00Z", now)).toBe("Up 3 days");
  });

  it("never goes negative", () => {
    expect(formatUptime("2026-10-30T13:00:00Z", now)).toBe("Up 0 seconds");
    expect(formatUptime("garbage", now)).toBe("unknown");
  });
});

describe("groupByProject", () => {
  it("keeps order inside groups", () => {
    const groups = groupByProject([
      container({ id: "a", project_slug: "x" }),
      container({ id: "b", project_slug: "y" }),
      container({ id: "c", project_slug: "x" }),
    ]);
    expect([...groups.keys()]).toEqual(["x", "y"]);
    expect(groups.get("x")?.map((c) => c.id)).toEqual(["a", "c"]);
  });
});

describe("staleHashes", () => {
  it("flags containers whose hash differs from the newest running one", () => {
    const stale = staleHashes([
      container({
        id: "old",
        project_slug: "x",
        config_hash: "aaa",
        started_at: "2026-10-30T08:00:00Z",
      }),
      container({
        id: "new",
        project_slug: "x",
        config_hash: "bbb",
        started_at: "2026-10-30T10:00:00Z",
      }),
      container({
        id: "other",
        project_slug: "y",
        config_hash: "aaa",
        started_at: "2026-10-30T09:00:00Z",
      }),
    ]);
    expect([...stale]).toEqual(["old"]);
  });

  it("ignores projects with a single hash", () => {
    expect(
      staleHashes([
        container({ id: "a", config_hash: "aaa" }),
        container({ id: "b", config_hash: "aaa" }),
      ]).size,
    ).toBe(0);
  });
});

describe("summarize", () => {
  it("counts sessions per agent, most frequent first", () => {
    const summary = summarize([
      container({
        id: "a",
        project_slug: "x",
        sessions: [
          { pid: 1, command: "claude", agent: "claude-code" },
          { pid: 2, command: "zsh", agent: "shell" },
        ],
      }),
      container({
        id: "b",
        project_slug: "y",
        state: "exited",
        sessions: [],
      }),
      container({
        id: "c",
        project_slug: "y",
        sessions: [{ pid: 3, command: "claude", agent: "claude-code" }],
      }),
    ]);
    expect(summary.projects).toBe(2);
    expect(summary.running).toBe(2);
    expect(summary.stopped).toBe(1);
    expect(summary.sessions).toBe(3);
    expect([...summary.byAgent]).toEqual([
      ["claude-code", 2],
      ["shell", 1],
    ]);
  });
});
