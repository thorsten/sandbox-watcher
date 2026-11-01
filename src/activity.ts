import { AGENT_LABELS } from "./format";
import { formatBytes } from "./sparkline";
import type { AgentActivity, Attribution, ContainerActivity, Coverage, Session } from "./types";

/** Whose transcript this is, in one line. */
export function describeAttribution(a: Attribution): string {
  if (a.kind === "session") {
    const why = {
      "open-file": "holds the file open",
      "only-session": "only session of this agent",
      "start-time": "started right before the transcript began",
    }[a.evidence];
    return `Session PID ${a.pid} (${why})`;
  }
  return `Agent-level: could be PID ${a.candidates.join(" or ")}`;
}

/** Why the numbers may be off, or null when they cover the whole transcript. */
export function describeCoverage(c: Coverage): string | null {
  const notes: string[] = [];
  if (!c.complete) {
    notes.push(
      c.pending_bytes > 0
        ? `still reading, ${formatBytes(c.pending_bytes)} to go`
        : "still reading the transcript",
    );
  }
  if (c.skipped_bytes > 0) notes.push(`${formatBytes(c.skipped_bytes)} of oversized lines skipped`);
  if (c.restarts > 0) {
    notes.push(
      `file was replaced ${c.restarts === 1 ? "once" : `${c.restarts} times`}; earlier totals kept`,
    );
  }
  return notes.length === 0 ? null : `Totals incomplete: ${notes.join("; ")}`;
}

/** Sessions of a known agent that no transcript could be tied to. */
export function sessionsWithoutTranscript(activity: ContainerActivity): Session[] {
  const covered = new Set<number>();
  for (const a of activity.agents) {
    for (const pid of a.attribution.kind === "session"
      ? [a.attribution.pid]
      : a.attribution.candidates) {
      covered.add(pid);
    }
  }
  return activity.sessions
    .map((s) => s.session)
    .filter((s) => s.agent in AGENT_LABELS && s.agent !== "shell" && s.agent !== "other")
    .filter((s) => !covered.has(s.pid));
}

/** Heading detail for one transcript block. */
export function transcriptTitle(agent: AgentActivity): string {
  const base = AGENT_LABELS[agent.agent];
  return agent.attribution.kind === "session"
    ? `${base} · PID ${agent.attribution.pid}`
    : `${base} · shared by ${agent.attribution.candidates.length} sessions`;
}
