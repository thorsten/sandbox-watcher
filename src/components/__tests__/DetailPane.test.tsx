import { describe, expect, it, vi } from "vitest";
import { connected, container, entry, NOW, report, session } from "../../test/fixtures";
import { render } from "../../test/render";
import { DetailPane } from "../DetailPane";

const noop = vi.fn<() => void>();

function tabsOf(html: string): string[] {
  return [...html.matchAll(/class="tab[^"]*"[^>]*>([A-Za-z]+)/g)].map((m) => m[1] ?? "");
}

describe("DetailPane", () => {
  it("shows header metadata and sessions", () => {
    const { text } = render(
      <DetailPane
        container={container({ sessions: [session({ command: "claude --resume" })] })}
        runtime={connected()}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />,
    );
    expect(text).toContain("sandbox-web-shop");
    expect(text).toContain("Docker");
    expect(text).toContain("Up 2 hours");
    expect(text).toContain("a1b2c3d4e5f6");
    expect(text).toContain("claude --resume");
    expect(text).toContain("PID 4242");
  });

  it("opens on the Activity tab with all five tabs for a running Docker container", () => {
    const { html } = render(
      <DetailPane
        container={container()}
        runtime={connected()}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />,
    );
    expect(tabsOf(html)).toEqual(["Activity", "Network", "Resources", "Logs", "Mounts"]);
    expect(html).toMatch(/class="tab active"[^>]*>Activity/);
  });

  it("hides Resources for runtimes without stats", () => {
    const { html } = render(
      <DetailPane
        container={container({ runtime: "apple-container" })}
        runtime={connected(["apple-container"])}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />,
    );
    expect(tabsOf(html)).not.toContain("Resources");
  });

  it("limits stopped containers to logs and mounts", () => {
    const { html, text } = render(
      <DetailPane
        container={container({ state: "exited" })}
        runtime={connected()}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />,
    );
    expect(tabsOf(html)).toEqual(["Logs", "Mounts"]);
    expect(text).not.toContain("No active sessions");
    expect(text).not.toContain("Stop");
  });

  it("badges the Network tab with the blocked count", () => {
    const { html } = render(
      <DetailPane
        container={container()}
        runtime={connected()}
        report={report([
          entry(),
          entry({ destination: "b.example" }),
          entry({ status: "allowed" }),
        ])}
        now={NOW}
        onChanged={noop}
      />,
    );
    expect(html).toMatch(/Network<span class="badge badge-err">2<\/span>/);
  });

  it("says when there are no sessions", () => {
    const { text } = render(
      <DetailPane
        container={container()}
        runtime={connected()}
        report={undefined}
        now={NOW}
        onChanged={noop}
      />,
    );
    expect(text).toContain("No active sessions");
  });
});
