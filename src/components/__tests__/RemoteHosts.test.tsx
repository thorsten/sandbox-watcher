import { describe, expect, it, vi } from "vitest";
import { connected, container, NOW } from "../../test/fixtures";
import { render } from "../../test/render";
import { ContainerActions } from "../ContainerActions";
import { RemoteHostsSection, validAlias } from "../RemoteHostsSection";
import { RuntimeBanner } from "../RuntimeBanner";
import { Sidebar } from "../Sidebar";
import { StopAllButton } from "../StopAllButton";
import type { HostStatus } from "../../types";

const noop = vi.fn<() => void>();

describe("remote hosts in the UI", () => {
  const remote = container({ id: "b", key: "devbox/b", host: "devbox" });

  it("hides host-side actions for remote containers but keeps stop", () => {
    const { html } = render(<ContainerActions container={remote} onChanged={noop} />);
    expect(html).not.toContain(">Terminal<");
    expect(html).not.toContain(">Folder<");
    expect(html).not.toContain(">Editor<");
    expect(html).toContain("Stop");
    expect(html).toContain("remote host");
    expect(html).toContain("Not available for remote hosts");
  });

  it("keeps host-side actions for local containers", () => {
    const { html } = render(<ContainerActions container={container()} onChanged={noop} />);
    expect(html).toContain(">Terminal<");
  });

  it("shows a host badge and last seen for unreachable hosts", () => {
    const greyed = { ...remote, last_seen_at: "2026-10-30T11:55:00Z" };
    const { html, text } = render(
      <Sidebar
        containers={[container({ id: "a" }), greyed]}
        runtime={connected()}
        hosts={[]}
        network={[]}
        selectedId={null}
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(html).toContain("host-badge");
    expect(html).toContain("unreachable");
    expect(text).toContain("devbox");
    expect(text).toContain("last seen 5m ago");
    expect(html).toContain("<select");
  });

  it("shows no host filter with a single host", () => {
    const { html } = render(
      <Sidebar
        containers={[container({ id: "a" })]}
        runtime={connected()}
        hosts={[]}
        network={[]}
        selectedId={null}
        onSelect={noop}
        now={NOW}
      />,
    );
    expect(html).not.toContain("<select");
    expect(html).not.toContain("host-badge");
  });

  it("renders the settings section with an empty list", () => {
    const { html } = render(<RemoteHostsSection hosts={[]} statuses={[]} onChange={noop} />);
    expect(html).toContain("Remote hosts");
    expect(html).toContain("SSH alias");
    expect(html).toContain("Test connection");
  });

  it("rejects option-like aliases", () => {
    expect(validAlias("-oProxyCommand=x")).toBe(false);
    expect(validAlias("local")).toBe(false);
    expect(validAlias("devbox")).toBe(true);
  });

  it("lists unreachable hosts with their probes in the banner", () => {
    const hosts: HostStatus[] = [
      {
        alias: "devbox",
        display_name: "Dev VM",
        state: "unreachable",
        reason: "ssh: connection refused",
        since: "2026-10-30T11:55:00Z",
        runtimes: [],
        probes: ["ssh -V ok", "docker.sock missing"],
      },
    ];
    const { text, html } = render(<RuntimeBanner runtime={connected()} hosts={hosts} />);
    expect(text).toContain("Dev VM: ssh: connection refused");
    expect(html).toContain("<details");
    expect(text).toContain("docker.sock missing");
  });

  it("groups stop-all sessions by host", () => {
    const { text } = render(
      <StopAllButton running={[remote, container({ id: "a" })]} hosts={[]} onChanged={noop} />,
    );
    expect(text).toContain("Stop all (2)");
  });
});
