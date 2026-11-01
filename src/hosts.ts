import type { HostStatus, ContainerOverview } from "./types";

export function isRemote(c: ContainerOverview): boolean {
  return c.host !== "local";
}

/** Display name of a host, falling back to its alias. */
export function hostLabel(host: string, hosts: HostStatus[]): string {
  return hosts.find((h) => h.alias === host)?.display_name || host;
}

export function filterByHost(cs: ContainerOverview[], host: string): ContainerOverview[] {
  return host === "all" ? cs : cs.filter((c) => c.host === host);
}

/**
 * The container the detail pane shows: the selected one while it is
 * visible, else the first running and then the first other container of a
 * reachable host. Greyed containers of unreachable hosts are only picked
 * automatically when nothing else is visible.
 */
export function pickSelected(
  visible: ContainerOverview[],
  selectedId: string | null,
): ContainerOverview | null {
  const live = visible.filter((c) => !c.last_seen_at);
  return (
    visible.find((c) => c.key === selectedId) ??
    live.find((c) => c.state === "running") ??
    live[0] ??
    visible[0] ??
    null
  );
}

/** Hosts that have containers: local first, then aliases sorted. */
export function hostsInUse(cs: ContainerOverview[]): string[] {
  const remote = [...new Set(cs.map((c) => c.host).filter((h) => h !== "local"))].toSorted();
  return cs.some((c) => c.host === "local") ? ["local", ...remote] : remote;
}
