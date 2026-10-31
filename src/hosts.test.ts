import { describe, expect, it } from "vitest";
import { filterByHost, hostLabel, hostsInUse, isRemote, pickSelected } from "./hosts";
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

  describe("pickSelected", () => {
    const greyed = container({
      id: "g",
      key: "devbox/g",
      host: "devbox",
      last_seen_at: "2026-10-30T11:00:00Z",
    });
    const stopped = container({ id: "s", state: "exited" });

    it("keeps the selected container while it is visible", () => {
      expect(pickSelected([local, remote], "devbox/b")).toBe(remote);
      expect(pickSelected([greyed, local], "devbox/g")).toBe(greyed);
    });

    it("skips greyed containers when picking one automatically", () => {
      expect(pickSelected([greyed, local], null)).toBe(local);
      expect(pickSelected([greyed, stopped], null)).toBe(stopped);
    });

    it("falls back to the first visible container when the filter hides the selection", () => {
      const visible = filterByHost([local, remote], "devbox");
      expect(pickSelected(visible, "a")).toBe(remote);
    });

    it("shows a greyed container only when nothing else is left", () => {
      expect(pickSelected([greyed], null)).toBe(greyed);
      expect(pickSelected([], "a")).toBeNull();
    });
  });
});
