import { useState } from "react";
import { AGENT_LABELS, formatUptime } from "../format";
import {
  type Capabilities,
  type NetworkReport,
  RUNTIME_LABELS,
  type RuntimeStatus,
  type SandboxContainer,
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
  container: SandboxContainer;
  runtime: RuntimeStatus;
  report: NetworkReport | undefined;
  now: Date;
  onChanged: () => void;
}

export function DetailPane({ container, runtime, report, now, onChanged }: Props) {
  const [tab, setTab] = useState<Tab>("activity");
  const isRunning = container.state === "running";
  const capabilities: Capabilities | undefined =
    runtime.status === "connected"
      ? runtime.runtimes.find((r) => r.kind === container.runtime)?.capabilities
      : undefined;
  const tabs = TABS.filter((t) => {
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
            <span>{isRunning ? formatUptime(container.started_at, now) : container.state}</span>
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

      <div className="detail-body">
        {active === "activity" && <ActivityPanel containerId={container.id} now={now} />}
        {active === "network" && <NetworkPanel report={report} now={now} />}
        {active === "resources" && <ResourcesPanel containerId={container.id} />}
        {active === "logs" && <LogsPanel containerId={container.id} />}
        {active === "mounts" && <MountsPanel containerId={container.id} />}
      </div>
    </section>
  );
}
