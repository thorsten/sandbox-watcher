import { describe, expect, it } from "vitest";
import { entry, NOW, report } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { NetworkPanel } from "../NetworkPanel";

describe("NetworkPanel", () => {
  it("shows a loading state before the first report", () => {
    expect(render(<NetworkPanel containerId="c1" report={undefined} now={NOW} />).text).toContain(
      "Reading network logs",
    );
  });

  it("renders a row per entry with status, count and age", () => {
    const { text, html } = render(
      <NetworkPanel
        containerId="c1"
        report={report([
          entry({ destination: "evil.example", count: 3 }),
          entry({
            destination: "registry.npmjs.org",
            status: "allowed",
            last_seen: "2026-10-30T11:58:00Z",
          }),
        ])}
        now={NOW}
      />,
    );
    expect(count(html, "<tr>")).toBe(3);
    expect(text).toContain("evil.example");
    expect(text).toContain("blocked");
    expect(text).toContain("allowed");
    expect(text).toContain("2m ago");
    expect(text).toContain("All 2");
    expect(text).toContain("Blocked 1");
  });

  it("labels DNS-only rows and shows the resolved IP", () => {
    const { text } = render(
      <NetworkPanel
        containerId="c1"
        report={report([
          entry({ destination: "dns.only", port: 0 }),
          entry({ destination: "example.com", dst_ip: "93.184.216.34" }),
        ])}
        now={NOW}
      />,
    );
    expect(text).toContain("DNS");
    expect(text).toContain("93.184.216.34");
  });

  it("offers checkboxes only for blocked host names", () => {
    const { html } = render(
      <NetworkPanel
        containerId="c1"
        report={report([
          entry({ destination: "blocked.example" }),
          entry({ destination: "10.0.0.5", dst_ip: "10.0.0.5" }),
          entry({ destination: "ok.example", status: "allowed" }),
        ])}
        now={NOW}
      />,
    );
    expect(count(html, 'type="checkbox"')).toBe(1);
    expect(html).toContain('aria-label="Select blocked.example"');
  });

  it("warns about failed and unparsed sources", () => {
    const { text } = render(
      <NetworkPanel
        containerId="c1"
        report={report([], { failed_sources: ["dns"], unparsed_sources: ["proxy-access"] })}
        now={NOW}
      />,
    );
    expect(text).toContain("Could not read DNS log");
    expect(text).toContain("proxy log had content in a format this version does not understand");
    expect(text).toContain("No matching network activity.");
  });
});
