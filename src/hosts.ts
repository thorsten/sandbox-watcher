import type { HostStatus, SandboxContainer } from "./types";

export function isRemote(c: SandboxContainer): boolean {
  return c.host !== "local";
}

/** Display name of a host, falling back to its alias. */
export function hostLabel(host: string, hosts: HostStatus[]): string {
  return hosts.find((h) => h.alias === host)?.display_name || host;
}

export function filterByHost(cs: SandboxContainer[], host: string): SandboxContainer[] {
  return host === "all" ? cs : cs.filter((c) => c.host === host);
}

/** Hosts that have containers: local first, then aliases sorted. */
export function hostsInUse(cs: SandboxContainer[]): string[] {
  const remote = [...new Set(cs.map((c) => c.host).filter((h) => h !== "local"))].toSorted();
  return cs.some((c) => c.host === "local") ? ["local", ...remote] : remote;
}
