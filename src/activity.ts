import { AGENT_LABELS } from "./format";
import { t } from "./i18n";
import { formatBytes } from "./sparkline";
import type { AgentActivity, Attribution, ContainerActivity, Coverage, Session } from "./types";

/** Whose transcript this is, in one line. */
export function describeAttribution(a: Attribution): string {
  if (a.kind === "session") {
    const why = {
      "open-file": t("attribution.openFile"),
      "only-session": t("attribution.onlySession"),
      "start-time": t("attribution.startTime"),
    }[a.evidence];
    return t("attribution.session", { pid: String(a.pid), why });
  }
  return t("attribution.agent", { candidates: a.candidates.join(t("common.or")) });
}

/** Why the numbers may be off, or null when they cover the whole transcript. */
export function describeCoverage(c: Coverage): string | null {
  const notes: string[] = [];
  if (!c.complete) {
    notes.push(
      c.pending_bytes > 0
        ? t("coverage.stillReadingBytes", { bytes: formatBytes(c.pending_bytes) })
        : t("coverage.stillReading"),
    );
  }
  if (c.skipped_bytes > 0) {
    notes.push(t("coverage.skipped", { bytes: formatBytes(c.skipped_bytes) }));
  }
  if (c.restarts > 0) notes.push(t("coverage.replaced", { n: c.restarts }));
  return notes.length === 0 ? null : t("coverage.incomplete", { notes: notes.join("; ") });
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
    ? t("transcript.pid", { agent: base, pid: String(agent.attribution.pid) })
    : t("transcript.shared", { agent: base, n: agent.attribution.candidates.length });
}
