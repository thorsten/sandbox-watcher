import type { AgentKind, SandboxContainer } from "./types";

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
export function groupByProject(containers: SandboxContainer[]): Map<string, SandboxContainer[]> {
  const groups = new Map<string, SandboxContainer[]>();
  for (const container of containers) {
    const group = groups.get(container.project_slug);
    if (group) group.push(container);
    else groups.set(container.project_slug, [container]);
  }
  return groups;
}

/** Hashes that differ inside one project mean one container is stale. */
export function staleHashes(containers: SandboxContainer[]): Set<string> {
  const stale = new Set<string>();
  for (const [, group] of groupByProject(containers)) {
    const hashes = new Set(group.map((c) => c.config_hash).filter((h): h is string => !!h));
    if (hashes.size <= 1) continue;
    // The newest running container defines the current hash.
    const current = group
      .filter((c) => c.state === "running" && c.started_at)
      .toSorted(
        (a, b) => Date.parse(b.started_at ?? "") - Date.parse(a.started_at ?? ""),
      )[0]?.config_hash;
    for (const c of group) {
      if (c.config_hash && current && c.config_hash !== current) stale.add(c.id);
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
export function summarize(containers: SandboxContainer[]): Summary {
  const byAgent = new Map<AgentKind, number>();
  let sessions = 0;
  let running = 0;
  for (const c of containers) {
    if (c.state === "running") running++;
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
    stopped: containers.length - running,
    sessions,
    byAgent: ordered,
  };
}
