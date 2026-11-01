import { useCallback } from "react";
import { getActivity } from "../api";
import {
  describeAttribution,
  describeCoverage,
  sessionsWithoutTranscript,
  transcriptTitle,
} from "../activity";
import { AGENT_LABELS } from "../format";
import { FilesList } from "./FilesList";
import { TurnsSection } from "./TurnsSection";
import { formatRelative } from "../network";
import { formatBytes } from "../sparkline";
import { formatCost, formatTokens, totalTokens } from "../usageFormat";
import { usePolling } from "../usePolling";
import type { ActivityEvent, ProcessInfo, SessionProcesses, Usage } from "../types";

export const POLL_MS = 3_000;

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

export function ActivityPanel({ containerKey, now }: { containerKey: string; now: Date }) {
  const load = useCallback(() => getActivity(containerKey), [containerKey]);
  const { data: activity, error } = usePolling(load, POLL_MS);

  // A failed refresh keeps the last activity on screen; the note says the data may be stale.
  const note = error !== null ? <p className="network-note warn">{error}</p> : null;
  if (!activity) return note ?? <div className="activity muted">Looking inside the sandbox…</div>;

  return (
    <div className="activity">
      {note}
      {activity.agents.map((agent) => (
        <section key={agent.source} className="activity-block">
          <h3>
            {transcriptTitle(agent)}
            {agent.updated_at && (
              <span className="muted">
                {" "}
                · last activity {formatRelative(agent.updated_at, now)}
              </span>
            )}
          </h3>
          <p className="muted small" title={agent.source}>
            {describeAttribution(agent.attribution)}
          </p>
          {describeCoverage(agent.coverage) && (
            <p className="network-note warn">{describeCoverage(agent.coverage)}</p>
          )}
          {agent.usage && <UsageLine usage={agent.usage} label="Whole transcript" />}
          {agent.session_usage && (
            <UsageLine
              usage={agent.session_usage}
              label="This session"
              note="The transcript is older than the session; history counts this part only."
            />
          )}
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
          <h4 className="files-title">Files</h4>
          <FilesList
            files={agent.files}
            containerKey={containerKey}
            projectDir={activity.project_dir}
            now={now}
          />
          <TurnsSection
            containerKey={containerKey}
            agent={agent.agent}
            pid={agent.attribution.kind === "session" ? agent.attribution.pid : null}
            projectDir={activity.project_dir}
            now={now}
          />
        </section>
      ))}

      {sessionsWithoutTranscript(activity).map((s) => (
        <p key={s.pid} className="muted small">
          No transcript written by {AGENT_LABELS[s.agent]} PID {s.pid} yet. Showing its processes
          only.
        </p>
      ))}

      {activity.sessions.map((s) => (
        <SessionBlock key={s.session.pid} session={s} now={now} />
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

function UsageLine({ usage, label, note }: { usage: Usage; label: string; note?: string }) {
  return (
    <div className="usage-line muted" title={note}>
      <span className="small">{label}:</span>
      <span title="Input, output, cache read, cache write tokens">
        {formatTokens(totalTokens(usage))} tokens
        <span className="small">
          {" "}
          (in {formatTokens(usage.input_tokens)} · out {formatTokens(usage.output_tokens)}
          {usage.cache_read_tokens > 0 && <> · cache {formatTokens(usage.cache_read_tokens)}</>})
        </span>
      </span>
      <span
        title={
          usage.cost_source === "estimated"
            ? "Estimated from the price table in Settings"
            : usage.cost_source === "reported"
              ? "Reported by the agent"
              : "No price for this model; edit the price table in Settings"
        }
      >
        {formatCost(usage.cost_usd, usage.cost_source)}
      </span>
      {usage.models.length > 0 && <code>{usage.models.join(", ")}</code>}
      <span className="small">{usage.turns} turns</span>
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

function SessionBlock({ session, now }: { session: SessionProcesses; now: Date }) {
  const s = session.session;
  return (
    <section className="activity-block">
      <h3>
        <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
        <code className="muted">{s.command}</code>
        <span className="muted"> · PID {s.pid}</span>
        {session.started_at && (
          <span className="muted"> · started {formatRelative(session.started_at, now)}</span>
        )}
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
