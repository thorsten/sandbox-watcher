import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it } from "vitest";
import { connected } from "../../test/fixtures";
import { render } from "../../test/render";
import type { HostStatus, RuntimeStatus } from "../../types";
import { RuntimeBanner } from "../RuntimeBanner";

const devbox: HostStatus = {
  alias: "devbox",
  display_name: "Dev VM",
  state: "connected",
  runtimes: ["docker"],
  probes: [],
};

/**
 * Expand function components (these use no hooks) and collect the keys of
 * every list of siblings, since static rendering does not warn on duplicates.
 */
function siblingKeys(node: ReactNode): string[][] {
  if (Array.isArray(node)) {
    // Unkeyed siblings are static JSX children, not a rendered list.
    const keys = node.flatMap((e) => (isValidElement(e) && e.key !== null ? [e.key] : []));
    return [keys, ...node.flatMap(siblingKeys)];
  }
  if (!isValidElement(node)) return [];
  const element = node as ReactElement<{ children?: ReactNode }>;
  if (typeof element.type === "function") {
    const expand = element.type as (props: unknown) => ReactNode;
    return siblingKeys(expand(element.props));
  }
  return siblingKeys(element.props.children);
}

/** Local Docker plus Docker on `devbox` through a forwarded socket. */
function localAndRemote(): RuntimeStatus {
  const runtime = connected(["docker"]);
  if (runtime.status !== "connected") throw new Error("fixture");
  const local = runtime.runtimes[0];
  if (!local) throw new Error("fixture");
  runtime.runtimes.push({ ...local, host: "devbox", endpoint: "/tmp/sw-501-0/f0.sock" });
  runtime.failures = [
    { host: "devbox", kind: "podman", endpoint: "", reason: "no answer within 10s" },
    { host: "local", kind: "podman", endpoint: "", reason: "machine stopped" },
  ];
  return runtime;
}

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
    runtime.failures = [{ host: "local", kind: "podman", endpoint: "", reason: "machine stopped" }];
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

  it("names the host of remote runtimes and hides their forward socket", () => {
    const full = render(<RuntimeBanner runtime={localAndRemote()} hosts={[devbox]} />);
    const compact = render(<RuntimeBanner runtime={localAndRemote()} hosts={[devbox]} compact />);

    expect(full.text).toContain("Docker 27.0.1 via /var/run/docker.sock");
    expect(full.text).toContain("Docker on Dev VM 27.0.1");
    expect(full.text).toContain(
      "Podman on Dev VM detected but not reachable: no answer within 10s",
    );
    expect(full.text).toContain("Podman detected but not reachable: machine stopped");
    expect(compact.text).toBe("Docker Docker on Dev VM Podman on Dev VM Podman");
    for (const { html } of [full, compact]) expect(html).not.toContain("/tmp/sw-");
    for (const compactMode of [false, true]) {
      const tree = (
        <RuntimeBanner runtime={localAndRemote()} hosts={[devbox]} compact={compactMode} />
      );
      for (const keys of siblingKeys(tree)) expect(new Set(keys).size).toBe(keys.length);
    }
  });
});
