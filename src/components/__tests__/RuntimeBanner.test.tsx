import { describe, expect, it } from "vitest";
import { connected } from "../../test/fixtures";
import { render } from "../../test/render";
import { RuntimeBanner } from "../RuntimeBanner";

describe("RuntimeBanner", () => {
  it("explains a missing runtime", () => {
    const { text } = render(<RuntimeBanner runtime={{ status: "not-detected" }} />);
    expect(text).toContain("No container runtime detected.");
    expect(text).toContain("DOCKER_HOST");
  });

  it("lists detected but unconnected runtimes", () => {
    const { text } = render(
      <RuntimeBanner
        runtime={{
          status: "detected-only",
          candidates: [{ kind: "podman", endpoint: "/run/podman.sock", source: "rootless socket" }],
        }}
      />,
    );
    expect(text).toContain("could not connect");
    expect(text).toContain("Podman at /run/podman.sock");
    expect(text).toContain("rootless socket");
  });

  it("shows the reason when unreachable", () => {
    const { text, html } = render(
      <RuntimeBanner
        runtime={{ status: "unavailable", candidates: [], reason: "connection refused" }}
      />,
    );
    expect(html).toContain("banner-error");
    expect(text).toContain("connection refused");
  });

  it("lists every connected runtime with version and endpoint", () => {
    const { text } = render(<RuntimeBanner runtime={connected(["docker", "podman"])} />);
    expect(text).toContain("Docker 27.0.1 via /var/run/docker.sock");
    expect(text).toContain("Podman");
  });

  it("mentions failures next to connected runtimes", () => {
    const runtime = connected(["docker"]);
    if (runtime.status !== "connected") throw new Error("fixture");
    runtime.failures = [{ kind: "podman", endpoint: "", reason: "machine stopped" }];
    const { text } = render(<RuntimeBanner runtime={runtime} />);
    expect(text).toContain("Podman detected but not reachable: machine stopped");
  });

  it("renders a compact variant for the sidebar", () => {
    const { html, text } = render(
      <RuntimeBanner runtime={connected(["docker", "apple-container"])} compact />,
    );
    expect(html).toContain("runtime-compact");
    expect(text).toBe("Docker Apple container");
  });
});
