import { describe, expect, it } from "vitest";
import {
  describeInterruption,
  describeInterruptionByHost,
  describeStopReport,
  networkLogsCommand,
} from "./actions";
import type { HostStatus, ContainerOverview } from "./types";

function container(partial: Partial<ContainerOverview> & { id: string }): ContainerOverview {
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

const ok = (container_id: string, name: string) => ({ container_id, name, ok: true, error: null });

describe("describeStopReport", () => {
  const hosts: HostStatus[] = [
    {
      state: "connected",
      alias: "devbox",
      display_name: "Dev box",
      runtimes: ["docker"],
      probes: [],
    },
  ];

  it("counts a clean stop", () => {
    expect(describeStopReport({ results: [ok("a", "sandbox-a")], failures: [] })).toBe(
      "Stopped 1 container",
    );
    expect(describeStopReport({ results: [], failures: [] })).toBe("Stopped 0 containers");
  });

  it("names failed containers and hosts that could not be listed", () => {
    const text = describeStopReport(
      {
        results: [
          ok("a", "sandbox-a"),
          { container_id: "devbox/b", name: "sandbox-b", ok: false, error: "no answer" },
        ],
        failures: [
          { host: "devbox", runtime: "podman", error: "connection refused" },
          { host: "local", runtime: "docker", error: "500 broken" },
        ],
      },
      hosts,
    );
    expect(text).toBe(
      "stopped 1; 1 failed: devbox: sandbox-b (no answer); could not list Podman containers on Dev box: connection refused; could not list Docker containers on this machine: 500 broken",
    );
  });

  it("never reports success when a listing failed", () => {
    const text = describeStopReport({
      results: [],
      failures: [{ host: "local", runtime: "docker", error: "down" }],
    });
    expect(text).not.toContain("Stopped");
    expect(text).toContain("could not list Docker containers on this machine: down");
  });
});
