import { describe, expect, it, vi } from "vitest";
import { container, session } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { ActivityPanel } from "../ActivityPanel";
import { AllowDialog } from "../AllowDialog";
import { ConfirmDialog } from "../ConfirmDialog";
import { ContainerActions } from "../ContainerActions";
import { HistoryView } from "../HistoryView";
import { LogsPanel } from "../LogsPanel";
import { MountsPanel } from "../MountsPanel";
import { ResourcesPanel } from "../ResourcesPanel";
import { Sparkline } from "../Sparkline";
import { StopAllButton } from "../StopAllButton";
import { UpdateBanner } from "../UpdateBanner";

const noop = vi.fn<() => void>();
const now = new Date("2026-10-30T12:00:00Z");

describe("ConfirmDialog", () => {
  it("lists every line and both buttons", () => {
    const { text } = render(
      <ConfirmDialog
        title="Stop it?"
        lines={["one", "two"]}
        confirmLabel="Stop now"
        onConfirm={noop}
        onCancel={noop}
      />,
    );
    expect(text).toContain("Stop it?");
    expect(text).toContain("one two");
    expect(text).toContain("Don't ask again");
    expect(text).toContain("Cancel Stop now");
  });
  it("disables buttons while busy", () => {
    const { html, text } = render(
      <ConfirmDialog
        title="t"
        lines={[]}
        confirmLabel="Go"
        busy
        onConfirm={noop}
        onCancel={noop}
      />,
    );
    expect(count(html, "disabled")).toBe(2);
    expect(text).toContain("Stopping…");
  });
});

describe("ContainerActions", () => {
  it("offers Stop only for running containers", () => {
    expect(render(<ContainerActions container={container()} onChanged={noop} />).text).toContain(
      "Stop",
    );
    expect(
      render(<ContainerActions container={container({ state: "exited" })} onChanged={noop} />).text,
    ).not.toContain("Stop");
  });
  it("always offers terminal, folder and copy", () => {
    const { text } = render(
      <ContainerActions container={container({ state: "exited" })} onChanged={noop} />,
    );
    expect(text).toContain("Terminal");
    expect(text).toContain("Folder");
    expect(text).toContain("Copy");
  });
});

describe("StopAllButton", () => {
  it("renders nothing without running containers", () => {
    expect(render(<StopAllButton running={[]} onChanged={noop} />).html).toBe("");
  });
  it("shows the number of running containers", () => {
    const { text } = render(
      <StopAllButton running={[container({ id: "a" }), container({ id: "b" })]} onChanged={noop} />,
    );
    expect(text).toBe("Stop all (2)");
  });
});

describe("Sparkline", () => {
  it("shows a placeholder until there are two samples", () => {
    const { text } = render(
      <Sparkline title="CPU" values={[5]} max={100} format={String} current="5%" />,
    );
    expect(text).toContain("collecting…");
  });
  it("draws line and area paths and labels the current value", () => {
    const { html } = render(
      <Sparkline title="CPU" values={[1, 50, 20]} max={100} format={String} current="20%" />,
    );
    expect(count(html, "<path")).toBe(2);
    expect(html).toContain('aria-label="CPU, latest 20%"');
  });
});

describe("UpdateBanner", () => {
  const update = {
    version: "0.3.0",
    current_version: "0.2.0",
    notes:
      "### Features\n\n- **core:** self-update ([abcdef1](https://x/commit/abcdef1))\n- faster refresh",
    date: null,
  };
  it("announces the version and summarises the notes", () => {
    const { text } = render(<UpdateBanner update={update} progress={null} onDismiss={noop} />);
    expect(text).toContain("Sandbox Watcher 0.3.0 is available");
    expect(text).toContain("you have 0.2.0");
    expect(text).toContain("core: self-update");
    expect(text).toContain("faster refresh");
    expect(text).not.toContain("abcdef1");
    expect(text).toContain("Later Install and restart");
  });
  it("omits the notes list when there are none", () => {
    const { html } = render(
      <UpdateBanner update={{ ...update, notes: null }} progress={null} onDismiss={noop} />,
    );
    expect(html).not.toContain("update-notes");
  });
});

describe("panels before their data arrives", () => {
  it("show loading states", () => {
    expect(render(<ActivityPanel containerKey="c1" now={now} />).text).toContain(
      "Looking inside the sandbox",
    );
    expect(render(<MountsPanel containerKey="c1" />).text).toContain("Inspecting container");
    expect(render(<ResourcesPanel containerKey="c1" />).text).toContain(
      "Collecting resource samples",
    );
    expect(render(<LogsPanel containerKey="c1" />).text).toContain("Reading container log");
    expect(
      render(
        <AllowDialog
          containerKey="c1"
          selections={[{ host: "a.b", ports: [443] }]}
          onClose={noop}
        />,
      ).text,
    ).toContain("Checking configuration");
  });
  it("render the history toolbar with ranges and filters", () => {
    const { text } = render(<HistoryView onClose={noop} />);
    expect(text).toContain("History");
    expect(text).toContain("Today 7 days 30 days All");
    expect(text).toContain("All projects");
    expect(text).toContain("All agents");
    expect(text).toContain("Claude Code");
  });
});

describe("fixtures", () => {
  it("build a running container with a session", () => {
    const c = container({ sessions: [session()] });
    expect(c.state).toBe("running");
    expect(c.sessions[0]?.agent).toBe("claude-code");
  });
});
