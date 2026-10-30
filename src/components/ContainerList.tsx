import { useState } from "react";
import { AGENT_LABELS, formatUptime, groupByProject, staleHashes, summarize } from "../format";
import { recentBlockedCount } from "../network";
import {
  type Capabilities,
  type NetworkReport,
  RUNTIME_LABELS,
  type RuntimeKind,
  type RuntimeStatus,
  type SandboxContainer,
  type Session,
} from "../types";
import { LogsPanel } from "./LogsPanel";
import { MountsPanel } from "./MountsPanel";
import { NetworkPanel } from "./NetworkPanel";
import { ResourcesPanel } from "./ResourcesPanel";

type Tab = "network" | "resources" | "logs" | "mounts";
const TABS: { id: Tab; label: string }[] = [
  { id: "network", label: "Network" },
  { id: "resources", label: "Resources" },
  { id: "logs", label: "Logs" },
  { id: "mounts", label: "Mounts" },
];

interface Props {
  containers: SandboxContainer[];
  runtime: RuntimeStatus;
  /** Undefined until the first network collection finished. */
  network: NetworkReport[] | undefined;
  now: Date;
}

export function ContainerList({ containers, runtime, network, now }: Props) {
  if (runtime.status !== "connected") return null;
  const capabilities = new Map<RuntimeKind, Capabilities>(
    runtime.runtimes.map((r) => [r.kind, r.capabilities]),
  );
  const showRuntime = runtime.runtimes.length > 1;

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
  const recentBlocked = network ? recentBlockedCount(network, now) : 0;
  const reportFor = (id: string) => network?.find((r) => r.container_id === id);

  return (
    <section>
      <div className="summary">
        <Stat label="Projects" value={summary.projects} />
        <Stat label="Running" value={summary.running} />
        <Stat label="Stopped" value={summary.stopped} />
        <Stat label="Sessions" value={summary.sessions} />
        <Stat
          label="Blocked, last 5 min"
          value={recentBlocked}
          tone={recentBlocked > 0 ? "warn" : undefined}
        />
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
              <ContainerRow
                key={c.id}
                container={c}
                stale={stale.has(c.id)}
                report={reportFor(c.id)}
                capabilities={capabilities.get(c.runtime)}
                showRuntime={showRuntime}
                now={now}
              />
            ))}
          </ul>
        </article>
      ))}
    </section>
  );
}

function Stat({ label, value, tone }: { label: string; value: number; tone?: "warn" }) {
  return (
    <div className={`stat ${tone ? `stat-${tone}` : ""}`}>
      <span className="stat-value">{value}</span>
      <span className="stat-label">{label}</span>
    </div>
  );
}

function ContainerRow({
  container,
  stale,
  report,
  capabilities,
  showRuntime,
  now,
}: {
  container: SandboxContainer;
  stale: boolean;
  report: NetworkReport | undefined;
  capabilities: Capabilities | undefined;
  showRuntime: boolean;
  now: Date;
}) {
  const tabs = TABS.filter((t) => {
    if (t.id === "resources") return capabilities?.stats !== false;
    if (t.id === "logs") return capabilities?.logs !== false;
    return true;
  });
  const [tab, setTab] = useState<Tab | null>(null);
  const isRunning = container.state === "running";
  const blocked = report?.entries.filter((e) => e.status === "blocked").length ?? 0;
  const starting = isRunning && container.readiness === "starting";
  const dot = !isRunning ? "dot-off" : starting ? "dot-warn" : "dot-ok";
  return (
    <li className={`container ${isRunning ? "" : "container-stopped"}`}>
      <span className={`dot ${dot}`} aria-hidden="true" />
      <div className="container-main">
        <div className="container-title">
          <strong>{container.name}</strong>
          {showRuntime && <span className="chip">{RUNTIME_LABELS[container.runtime]}</span>}
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
        {isRunning && (
          <div className="tabs" aria-label="Container details">
            {tabs.map((t) => (
              <button
                key={t.id}
                type="button"
                className={`tab ${tab === t.id ? "active" : ""}`}
                aria-pressed={tab === t.id}
                onClick={() => setTab((current) => (current === t.id ? null : t.id))}
              >
                {t.label}
                {t.id === "network" && blocked > 0 && (
                  <span className="badge badge-err">{blocked} blocked</span>
                )}
              </button>
            ))}
          </div>
        )}
        {isRunning && tab === "network" && <NetworkPanel report={report} now={now} />}
        {isRunning && tab === "resources" && <ResourcesPanel containerId={container.id} />}
        {isRunning && tab === "logs" && <LogsPanel containerId={container.id} />}
        {isRunning && tab === "mounts" && <MountsPanel containerId={container.id} />}
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
