import type { AgentKind, ContainerOverview, StopResult } from "./types";

/** Same buckets as `sandbox status` (container-status.ts, getContainerUptime). */
export function formatUptime(startedAt: string | null, now: Date = new Date()): string {
  if (!startedAt) return "unknown";
  const started = Date.parse(startedAt);
  if (Number.isNaN(started)) return "unknown";
  const seconds = Math.max(0, Math.floor((now.getTime() - started) / 1000));
  if (seconds < 60) return `Up ${seconds} seconds`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `Up ${minutes} minutes`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Up ${hours} hours`;
  return `Up ${Math.floor(hours / 24)} days`;
}

/** Group containers by project slug, keeping the incoming order inside a group. */
export function groupByProject(containers: ContainerOverview[]): Map<string, ContainerOverview[]> {
  const groups = new Map<string, ContainerOverview[]>();
  for (const container of containers) {
    const group = groups.get(container.project_slug);
    if (group) group.push(container);
    else groups.set(container.project_slug, [container]);
  }
  return groups;
}

/** Hashes that differ inside one project on one host mean one container is stale. Returns container keys. */
export function staleHashes(containers: ContainerOverview[]): Set<string> {
  const stale = new Set<string>();
  const groups = new Map<string, ContainerOverview[]>();
  for (const c of containers) {
    const id = `${c.host}\u0000${c.project_slug}`;
    const group = groups.get(id);
    if (group) group.push(c);
    else groups.set(id, [c]);
  }
  for (const group of groups.values()) {
    const hashes = new Set(group.map((c) => c.config_hash).filter((h): h is string => !!h));
    if (hashes.size <= 1) continue;
    // The newest running container defines the current hash.
    const current = group
      .filter((c) => c.state === "running" && c.started_at)
      .toSorted(
        (a, b) => Date.parse(b.started_at ?? "") - Date.parse(a.started_at ?? ""),
      )[0]?.config_hash;
    for (const c of group) {
      if (c.config_hash && current && c.config_hash !== current) stale.add(c.key);
    }
  }
  return stale;
}

export const AGENT_LABELS: Record<AgentKind, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  "open-code": "OpenCode",
  pi: "Pi",
  shell: "Shell",
  other: "Other",
};

export interface Summary {
  projects: number;
  running: number;
  stopped: number;
  sessions: number;
  byAgent: Map<AgentKind, number>;
}

/** Totals for the dashboard header. Agents are ordered by count, then label. */
export function summarize(containers: ContainerOverview[]): Summary {
  const byAgent = new Map<AgentKind, number>();
  let sessions = 0;
  let running = 0;
  for (const c of containers) {
    if (c.state === "running" && !c.last_seen_at) running++;
    // Greyed containers (unreachable host) are cached, not live.
    if (c.last_seen_at) continue;
    for (const s of c.sessions) {
      sessions++;
      byAgent.set(s.agent, (byAgent.get(s.agent) ?? 0) + 1);
    }
  }
  const ordered = new Map(
    [...byAgent].toSorted(
      ([a, x], [b, y]) => y - x || AGENT_LABELS[a].localeCompare(AGENT_LABELS[b]),
    ),
  );
  return {
    projects: groupByProject(containers).size,
    running,
    stopped: containers.filter((c) => c.state !== "running").length,
    sessions,
    byAgent: ordered,
  };
}

/** Name of a stop result, prefixed with the host alias for remote containers (key `alias/id`). */
export function stopResultLabel(result: Pick<StopResult, "container_id" | "name">): string {
  const slash = result.container_id.indexOf("/");
  return slash < 0 ? result.name : `${result.container_id.slice(0, slash)}: ${result.name}`;
}
