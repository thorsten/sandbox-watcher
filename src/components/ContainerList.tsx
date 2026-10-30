import { AGENT_LABELS, formatUptime, groupByProject, staleHashes, summarize } from "../format";
import type { RuntimeStatus, SandboxContainer, Session } from "../types";

interface Props {
  containers: SandboxContainer[];
  runtimeStatus: RuntimeStatus["status"];
  now: Date;
}

export function ContainerList({ containers, runtimeStatus, now }: Props) {
  if (runtimeStatus !== "connected") return null;

  if (containers.length === 0) {
    return (
      <section className="empty">
        <h2>No sandbox containers.</h2>
        <p className="muted">
          Start one with <code>sandbox run claude</code> in a project and it appears here.
        </p>
      </section>
    );
  }

  const stale = staleHashes(containers);
  const summary = summarize(containers);

  return (
    <section>
      <div className="summary">
        <Stat label="Projects" value={summary.projects} />
        <Stat label="Running" value={summary.running} />
        <Stat label="Stopped" value={summary.stopped} />
        <Stat label="Sessions" value={summary.sessions} />
        {summary.byAgent.size > 0 && (
          <div className="agent-chips" aria-label="Sessions per agent">
            {[...summary.byAgent].map(([agent, count]) => (
              <span key={agent} className={`chip agent-${agent}`}>
                {AGENT_LABELS[agent]} <strong>{count}</strong>
              </span>
            ))}
          </div>
        )}
      </div>
      {[...groupByProject(containers)].map(([slug, group]) => (
        <article key={slug} className="project">
          <h2>{slug}</h2>
          <ul className="containers">
            {group.map((c) => (
              <ContainerRow key={c.id} container={c} stale={stale.has(c.id)} now={now} />
            ))}
          </ul>
        </article>
      ))}
    </section>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="stat">
      <span className="stat-value">{value}</span>
      <span className="stat-label">{label}</span>
    </div>
  );
}

function ContainerRow({
  container,
  stale,
  now,
}: {
  container: SandboxContainer;
  stale: boolean;
  now: Date;
}) {
  const isRunning = container.state === "running";
  const starting = isRunning && container.readiness === "starting";
  const dot = !isRunning ? "dot-off" : starting ? "dot-warn" : "dot-ok";
  return (
    <li className={`container ${isRunning ? "" : "container-stopped"}`}>
      <span className={`dot ${dot}`} aria-hidden="true" />
      <div className="container-main">
        <div className="container-title">
          <strong>{container.name}</strong>
          <span className="muted">
            {isRunning ? formatUptime(container.started_at, now) : container.state}
          </span>
          {starting && (
            <span
              className="badge badge-warn"
              title="Entrypoint has not written /tmp/.sandbox-ready yet"
            >
              starting
            </span>
          )}
          {stale && (
            <span
              className="badge badge-warn"
              title="Configuration differs from the newest container of this project"
            >
              stale config
            </span>
          )}
        </div>
        <div className="container-meta muted">
          <span title="Image">{container.image}</span>
          {container.config_hash && <code title="sandbox.hash">{container.config_hash}</code>}
        </div>
        {isRunning && <SessionList sessions={container.sessions} />}
      </div>
    </li>
  );
}

function SessionList({ sessions }: { sessions: Session[] }) {
  if (sessions.length === 0) {
    return <div className="sessions muted">No active sessions</div>;
  }
  return (
    <ul className="sessions">
      {sessions.map((s) => (
        <li key={s.pid} className="session">
          <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
          <code className="session-command" title={s.command}>
            {s.command}
          </code>
          <span className="muted">PID {s.pid}</span>
        </li>
      ))}
    </ul>
  );
}
