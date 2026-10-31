import { describe, expect, it } from "vitest";
import { describeInterruption, networkLogsCommand } from "./actions";
import type { SandboxContainer } from "./types";

function container(partial: Partial<SandboxContainer> & { id: string }): SandboxContainer {
  return {
    host: "local",
    key: partial.id,
    last_seen_at: null,
    name: `sandbox-${partial.id}`,
    project_slug: partial.id,
    image: "img",
    state: "running",
    created_at: null,
    started_at: null,
    config_hash: null,
    labels: {},
    runtime: "docker",
    readiness: "ready",
    sessions: [],
    ...partial,
  };
}

describe("describeInterruption", () => {
  it("names sessions per container", () => {
    const lines = describeInterruption([
      container({ id: "a", sessions: [{ pid: 1, command: "claude", agent: "claude-code" }] }),
      container({ id: "b" }),
    ]);
    expect(lines).toEqual([
      "sandbox-a: 1 session will be interrupted: claude (PID 1)",
      "sandbox-b: no active sessions",
    ]);
  });
});

describe("networkLogsCommand", () => {
  it("quotes the project directory", () => {
    expect(networkLogsCommand("/Users/me/it's")).toBe(
      "cd '/Users/me/it'\\''s' && sandbox network logs --status=all",
    );
    expect(networkLogsCommand(null)).toBe("sandbox network logs --status=all");
  });
});
