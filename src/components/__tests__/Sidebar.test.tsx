import { describe, expect, it, vi } from "vitest";
import { connected, container, entry, NOW, report, session } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { Sidebar } from "../Sidebar";

const noop = vi.fn<() => void>();

describe("Sidebar", () => {
  it("prompts to start a sandbox when there are none", () => {
    const { text } = render(
      <Sidebar
        containers={[]}
        runtime={connected()}
        network={[]}
        selectedId={null}
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(text).toContain("No sandbox containers.");
    expect(text).toContain("sandbox run claude");
  });

  it("groups running containers by project and puts stopped ones last", () => {
    const { html, text } = render(
      <Sidebar
        containers={[
          container({ id: "1", project_slug: "web-shop", name: "sandbox-web-shop" }),
          container({ id: "2", project_slug: "billing", name: "sandbox-billing" }),
          container({ id: "3", project_slug: "docs", name: "sandbox-docs", state: "exited" }),
        ]}
        runtime={connected()}
        network={[]}
        selectedId="1"
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(count(html, 'class="project-group"')).toBe(2);
    expect(text.indexOf("billing")).toBeLessThan(text.indexOf("Stopped"));
    expect(text.indexOf("Stopped")).toBeLessThan(text.indexOf("sandbox-docs"));
    expect(html).toContain("card-stopped");
  });

  it("marks the selected card", () => {
    const { html } = render(
      <Sidebar
        containers={[container({ id: "1" }), container({ id: "2", project_slug: "other" })]}
        runtime={connected()}
        network={[]}
        selectedId="2"
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(count(html, 'aria-pressed="true"')).toBe(1);
    expect(count(html, "card selected")).toBe(1);
  });

  it("shows agent chips with counts, idle state and uptime", () => {
    const { text } = render(
      <Sidebar
        containers={[
          container({
            id: "1",
            sessions: [
              session({ pid: 1 }),
              session({ pid: 2 }),
              session({ pid: 3, agent: "shell" }),
            ],
          }),
          container({ id: "2", project_slug: "idle-one" }),
        ]}
        runtime={connected()}
        network={[]}
        selectedId={null}
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(text).toContain("Claude Code 2");
    expect(text).toContain("Shell");
    expect(text).toContain("idle");
    expect(text).toContain("Up 2 hours");
  });

  it("flags blocked requests, starting and stale containers", () => {
    const { text } = render(
      <Sidebar
        containers={[
          container({ id: "a", config_hash: "old", started_at: "2026-10-30T08:00:00Z" }),
          container({
            id: "b",
            config_hash: "new",
            started_at: "2026-10-30T09:00:00Z",
            readiness: "starting",
          }),
        ]}
        runtime={connected()}
        network={[
          report([entry(), entry({ destination: "other.example" })], { container_id: "a" }),
        ]}
        selectedId={null}
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(text).toContain("2 blocked");
    expect(text).toContain("starting");
    expect(text).toContain("stale");
  });

  it("names the runtime only when several are connected", () => {
    const props = {
      containers: [container({ id: "1", runtime: "podman" as const })],
      network: [],
      selectedId: null,
      onSelect: noop,
      now: NOW,
    };
    expect(render(<Sidebar {...props} runtime={connected(["docker"])} />).text).not.toContain(
      "· Podman",
    );
    expect(render(<Sidebar {...props} runtime={connected(["docker", "podman"])} />).text).toContain(
      "· Podman",
    );
  });
});
