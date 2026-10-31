import { describe, expect, it } from "vitest";
import { filterByHost, hostLabel, hostsInUse, isRemote } from "./hosts";
import { container } from "./test/fixtures";
import type { HostStatus } from "./types";

const devbox: HostStatus = {
  alias: "devbox",
  display_name: "Dev VM",
  state: "connected",
  runtimes: ["docker"],
  probes: [],
};

describe("hosts", () => {
  const local = container({ id: "a" });
  const remote = container({ id: "b", key: "devbox/b", host: "devbox" });

  it("tells remote containers apart", () => {
    expect(isRemote(local)).toBe(false);
    expect(isRemote(remote)).toBe(true);
  });

  it("labels hosts with their display name", () => {
    expect(hostLabel("devbox", [devbox])).toBe("Dev VM");
    expect(hostLabel("other", [devbox])).toBe("other");
  });

  it("filters and lists hosts, local first", () => {
    expect(filterByHost([local, remote], "devbox")).toEqual([remote]);
    expect(filterByHost([local, remote], "all")).toHaveLength(2);
    expect(hostsInUse([remote, local])).toEqual(["local", "devbox"]);
  });
});
