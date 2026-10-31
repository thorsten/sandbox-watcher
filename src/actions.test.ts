import { describe, expect, it } from "vitest";
import { describeInterruption, describeInterruptionByHost, networkLogsCommand } from "./actions";
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

  it("prefixes the network logs command with ssh for remote hosts", () => {
    expect(networkLogsCommand("/home/me/app", "devbox")).toBe(
      "ssh devbox 'cd '\\''/home/me/app'\\'' && sandbox network logs --status=all'",
    );
    expect(networkLogsCommand(null, "devbox")).toBe(
      "ssh devbox 'sandbox network logs --status=all'",
    );
    expect(networkLogsCommand("/x", "local")).toBe("cd '/x' && sandbox network logs --status=all");
  });
});

describe("describeInterruptionByHost", () => {
  const local = container({ id: "a" });
  const remote = container({ id: "b", key: "devbox/b", host: "devbox" });

  it("adds no headings for a single host", () => {
    expect(describeInterruptionByHost([local], [])).toEqual(describeInterruption([local]));
  });

  it("groups by host, local first, using host labels", () => {
    const lines = describeInterruptionByHost(
      [remote, local],
      [
        {
          alias: "devbox",
          display_name: "Dev VM",
          state: "connected",
          runtimes: ["docker"],
          probes: [],
        },
      ],
    );
    expect(lines).toEqual([
      "This machine",
      "sandbox-a: no active sessions",
      "Dev VM",
      "sandbox-b: no active sessions",
    ]);
  });
});
