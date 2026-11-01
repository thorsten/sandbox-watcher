import { describe, expect, it } from "vitest";
import { render } from "../../test/render";
import type { DiagnosticsReport } from "../../types";
import { DiagnosticsReportView } from "../DiagnosticsSection";

const report: DiagnosticsReport = {
  generated_at: "2026-11-01T12:00:00Z",
  app_version: "0.8.0",
  environment: {
    os: "linux",
    docker_host: null,
    container_host: null,
    runtime_override: null,
    config_runtime: "docker",
    config_path: "/home/me/.config/sandbox/config.toml",
    binaries: [
      { name: "docker", found: true },
      { name: "podman", found: false },
    ],
  },
  local: [
    {
      kind: "docker",
      candidate: "/var/run/docker.sock",
      outcome: "found",
      endpoint: "/var/run/docker.sock",
    },
    {
      kind: "docker",
      candidate: "connect /var/run/docker.sock",
      outcome: "failed",
      reason: "permission denied",
    },
    { kind: "podman", candidate: "CONTAINER_HOST", outcome: "missing" },
  ],
  hosts: [
    {
      alias: "devbox",
      status: {
        alias: "devbox",
        display_name: "Dev VM",
        state: "unreachable",
        reason: "Permission denied",
        since: "2026-11-01T11:00:00Z",
        runtimes: [],
        probes: ["docker: not found on PATH"],
      },
    },
  ],
};

describe("DiagnosticsReportView", () => {
  it("shows reasons for failed probes and the host probes", () => {
    const { text } = render(<DiagnosticsReportView report={report} />);
    expect(text).toContain("permission denied");
    expect(text).toContain("Dev VM");
    expect(text).toContain("docker: not found on PATH");
    expect(text).toContain("runtime = docker");
  });

  it("puts found probes first", () => {
    const { html } = render(<DiagnosticsReportView report={report} />);
    expect(html.indexOf("/var/run/docker.sock</code>")).toBeLessThan(
      html.indexOf("<code>CONTAINER_HOST</code>"),
    );
  });
});
