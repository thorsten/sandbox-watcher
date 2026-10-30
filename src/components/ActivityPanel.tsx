import { useEffect, useState } from "react";
import { getActivity } from "../api";
import { AGENT_LABELS } from "../format";
import { formatRelative } from "../network";
import { formatBytes } from "../sparkline";
import type {
  ActivityEvent,
  AgentKind,
  ContainerActivity,
  ProcessInfo,
  SessionProcesses,
} from "../types";

const POLL_MS = 3_000;

function formatElapsed(secs: number): string {
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m ${secs % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

const STATUS_LABELS: Record<string, string> = {
  "??": "new",
  " M": "modified",
  "M ": "staged",
  MM: "modified",
  "A ": "added",
  AM: "added",
  " D": "deleted",
  "D ": "deleted",
  R: "renamed",
};

/** Agents with a running session but no transcript we could read. */
function missingTranscripts(activity: ContainerActivity): AgentKind[] {
  const known: AgentKind[] = ["claude-code", "codex", "open-code", "pi"];
  const have = new Set(activity.agents.map((a) => a.agent));
  const running = new Set(activity.sessions.map((s) => s.session.agent));
  return known.filter((a) => running.has(a) && !have.has(a));
}

export function ActivityPanel({ containerId, now }: { containerId: string; now: Date }) {
  const [activity, setActivity] = useState<ContainerActivity | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const a = await getActivity(containerId);
        if (!cancelled) {
          setActivity(a);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [containerId]);

  if (error) return <p className="network-note warn">{error}</p>;
  if (!activity) return <div className="activity muted">Looking inside the sandbox…</div>;

  return (
    <div className="activity">
      {activity.agents.map((agent) => (
        <section key={agent.source} className="activity-block">
          <h3>
            {AGENT_LABELS[agent.agent]}
            {agent.updated_at && (
              <span className="muted">
                {" "}
                · last activity {formatRelative(agent.updated_at, now)}
              </span>
            )}
          </h3>
          {agent.in_flight ? (
            <div className="in-flight">
              <span className="pulse" aria-hidden="true" />
              <strong>{agent.in_flight.label}</strong>
              <code title={agent.in_flight.detail}>{agent.in_flight.detail}</code>
            </div>
          ) : (
            <div className="muted small">
              Waiting for input or thinking. No tool call in flight.
            </div>
          )}
          <ol className="timeline">
            {agent.events.toReversed().map((e, i) => (
              <EventRow key={`${e.tool_use_id ?? ""}-${e.at ?? ""}-${i}`} event={e} now={now} />
            ))}
          </ol>
        </section>
      ))}

      {missingTranscripts(activity).map((agent) => (
        <p key={agent} className="muted small">
          No {AGENT_LABELS[agent]} transcript found in this container yet. Showing its processes
          only.
        </p>
      ))}

      {activity.sessions.map((s) => (
        <SessionBlock key={s.session.pid} session={s} />
      ))}

      {activity.working_tree && (
        <section className="activity-block">
          <h3>
            Changes in the project
            {activity.working_tree.branch && (
              <span className="muted"> · branch {activity.working_tree.branch}</span>
            )}
          </h3>
          {!activity.working_tree.is_repo && <p className="muted small">Not a git repository.</p>}
          {activity.working_tree.is_repo && activity.working_tree.changes.length === 0 && (
            <p className="muted small">Working tree clean.</p>
          )}
          {activity.working_tree.changes.length > 0 && (
            <>
              <ul className="changes">
                {activity.working_tree.changes.map((c) => (
                  <li key={`${c.status}${c.path}`}>
                    <span
                      className={`chip change-${(STATUS_LABELS[c.status] ?? "other").replace(" ", "")}`}
                    >
                      {STATUS_LABELS[c.status] ?? c.status.trim()}
                    </span>
                    <code>{c.path}</code>
                  </li>
                ))}
              </ul>
              <p className="muted small">
                {activity.working_tree.shortstat}
                {activity.working_tree.truncated ? " · list truncated" : ""}
              </p>
            </>
          )}
        </section>
      )}

      {activity.agents.length === 0 && activity.sessions.length === 0 && (
        <p className="muted">No sessions in this container right now.</p>
      )}
      {activity.errors.length > 0 && (
        <p className="network-note warn">{activity.errors.join("; ")}</p>
      )}
    </div>
  );
}

function EventRow({ event, now }: { event: ActivityEvent; now: Date }) {
  const icon = {
    "user-message": "›",
    "assistant-text": "✦",
    "tool-use": "▶",
    "tool-result": "✓",
    "tool-error": "✗",
  }[event.kind];
  return (
    <li className={`event event-${event.kind}`}>
      <span className="event-icon" aria-hidden="true">
        {icon}
      </span>
      <span className="event-label">{event.label}</span>
      <span className="event-detail" title={event.detail}>
        {event.detail}
      </span>
      <span className="event-time muted">{formatRelative(event.at, now)}</span>
    </li>
  );
}

function SessionBlock({ session }: { session: SessionProcesses }) {
  const s = session.session;
  return (
    <section className="activity-block">
      <h3>
        <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
        <code className="muted">{s.command}</code>
        <span className="muted"> · PID {s.pid}</span>
      </h3>
      {session.running_now.length > 0 ? (
        <div className="running-now">
          <span className="muted">Running now:</span>
          {session.running_now.map((p) => (
            <code key={p.pid} title={p.command}>
              {p.command}
            </code>
          ))}
        </div>
      ) : (
        <div className="muted small">Idle. No child processes.</div>
      )}
      {session.tree.length > 0 && (
        <details>
          <summary className="muted small">Process tree ({session.tree.length})</summary>
          <ul className="proc-tree">
            {session.tree.map((p) => (
              <ProcRow key={p.pid} p={p} />
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

function ProcRow({ p }: { p: ProcessInfo }) {
  return (
    <li style={{ paddingLeft: `${p.depth * 1.1}rem` }}>
      <code title={p.command}>{p.command}</code>
      <span className="muted">
        {" "}
        {formatElapsed(p.elapsed_secs)} · {formatBytes(p.rss_bytes)} · PID {p.pid}
      </span>
    </li>
  );
}
