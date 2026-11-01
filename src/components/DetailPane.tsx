import { useState } from "react";
import { AGENT_LABELS, formatUptime } from "../format";
import { formatRelative } from "../network";
import {
  type Capabilities,
  type NetworkReport,
  RUNTIME_LABELS,
  type RuntimeStatus,
  type ContainerOverview,
} from "../types";
import { ActivityPanel } from "./ActivityPanel";
import { ContainerActions } from "./ContainerActions";
import { LogsPanel } from "./LogsPanel";
import { MountsPanel } from "./MountsPanel";
import { NetworkPanel } from "./NetworkPanel";
import { ResourcesPanel } from "./ResourcesPanel";

type Tab = "activity" | "network" | "resources" | "logs" | "mounts";
const TABS: { id: Tab; label: string }[] = [
  { id: "activity", label: "Activity" },
  { id: "network", label: "Network" },
  { id: "resources", label: "Resources" },
  { id: "logs", label: "Logs" },
  { id: "mounts", label: "Mounts" },
];

interface Props {
  container: ContainerOverview;
  runtime: RuntimeStatus;
  report: NetworkReport | undefined;
  now: Date;
  onChanged: () => void;
}

export function DetailPane({ container, runtime, report, now, onChanged }: Props) {
  const [tab, setTab] = useState<Tab>("activity");
  // Greyed: the host is unreachable and this is its last snapshot.
  const unreachable = container.last_seen_at !== null;
  const isRunning = container.state === "running" && !unreachable;
  const capabilities: Capabilities | undefined =
    runtime.status === "connected"
      ? runtime.runtimes.find((r) => r.host === container.host && r.kind === container.runtime)
          ?.capabilities
      : undefined;
  const tabs = TABS.filter((t) => {
    if (unreachable) return false;
    if (!isRunning) return t.id === "mounts" || t.id === "logs";
    if (t.id === "resources") return capabilities?.stats !== false;
    if (t.id === "logs") return capabilities?.logs !== false;
    return true;
  });
  const active = tabs.some((t) => t.id === tab) ? tab : (tabs[0]?.id ?? "mounts");
  const blocked = report?.entries.filter((e) => e.status === "blocked").length ?? 0;

  return (
    <section className="detail">
      <header className="detail-header">
        <div>
          <h2>
            <span
              className={`dot ${isRunning ? (container.readiness === "starting" ? "dot-warn" : "dot-ok") : "dot-off"}`}
              aria-hidden="true"
            />
            {container.name}
          </h2>
          <div className="detail-meta muted">
            <span>{container.project_slug}</span>
            <span>{RUNTIME_LABELS[container.runtime]}</span>
            <span>
              {unreachable
                ? `Host unreachable — last seen ${formatRelative(container.last_seen_at, now)}`
                : isRunning
                  ? formatUptime(container.started_at, now)
                  : container.state}
            </span>
            <span title="Image">{container.image}</span>
            {container.config_hash && <code title="sandbox.hash">{container.config_hash}</code>}
          </div>
        </div>
        <ContainerActions container={container} onChanged={onChanged} />
      </header>

      {isRunning && (
        <div className="sessions-row">
          {container.sessions.length === 0 ? (
            <span className="muted">No active sessions</span>
          ) : (
            container.sessions.map((s) => (
              <span key={s.pid} className="session-pill" title={s.command}>
                <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
                <code>{s.command}</code>
                <span className="muted">PID {s.pid}</span>
              </span>
            ))
          )}
        </div>
      )}

      {unreachable && (
        <p className="muted pad">Details come back when the host is reachable again.</p>
      )}

      {tabs.length > 0 && (
        <nav className="tabs" aria-label="Container details">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              className={`tab ${active === t.id ? "active" : ""}`}
              aria-pressed={active === t.id}
              onClick={() => setTab(t.id)}
            >
              {t.label}
              {t.id === "network" && blocked > 0 && (
                <span className="badge badge-err">{blocked}</span>
              )}
            </button>
          ))}
        </nav>
      )}

      {!unreachable && (
        <div className="detail-body">
          {active === "activity" && <ActivityPanel containerKey={container.key} now={now} />}
          {active === "network" && (
            <NetworkPanel
              containerKey={container.key}
              report={report}
              now={now}
              onChanged={onChanged}
            />
          )}
          {active === "resources" && <ResourcesPanel containerKey={container.key} />}
          {active === "logs" && <LogsPanel containerKey={container.key} />}
          {active === "mounts" && <MountsPanel containerKey={container.key} />}
        </div>
      )}
    </section>
  );
}
