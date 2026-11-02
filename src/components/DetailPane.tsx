import { useState } from "react";
import { AGENT_LABELS, formatUptime } from "../format";
import { stateLabel, t } from "../i18n";
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
const TABS: Tab[] = ["activity", "network", "resources", "logs", "mounts"];

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
  const tabs = TABS.filter((id) => {
    if (unreachable) return false;
    if (!isRunning) return id === "mounts" || id === "logs";
    if (id === "resources") return capabilities?.stats !== false;
    if (id === "logs") return capabilities?.logs !== false;
    return true;
  });
  const active = tabs.includes(tab) ? tab : (tabs[0] ?? "mounts");
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
                ? t("detail.unreachable", { when: formatRelative(container.last_seen_at, now) })
                : isRunning
                  ? formatUptime(container.started_at, now)
                  : stateLabel(container.state)}
            </span>
            <span title={t("detail.imageTitle")}>{container.image}</span>
            {container.config_hash && <code title="sandbox.hash">{container.config_hash}</code>}
          </div>
        </div>
        <ContainerActions container={container} onChanged={onChanged} />
      </header>

      {isRunning && (
        <div className="sessions-row">
          {container.sessions.length === 0 ? (
            <span className="muted">{t("detail.noSessions")}</span>
          ) : (
            container.sessions.map((s) => (
              <span key={s.pid} className="session-pill" title={s.command}>
                <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
                <code>{s.command}</code>
                <span className="muted">{t("common.pid", { pid: String(s.pid) })}</span>
              </span>
            ))
          )}
        </div>
      )}

      {unreachable && <p className="muted pad">{t("detail.unreachableHint")}</p>}

      {tabs.length > 0 && (
        <nav className="tabs" aria-label={t("detail.tabsLabel")}>
          {tabs.map((id) => (
            <button
              key={id}
              type="button"
              className={`tab ${active === id ? "active" : ""}`}
              aria-pressed={active === id}
              onClick={() => setTab(id)}
            >
              {t(`tab.${id}`)}
              {id === "network" && blocked > 0 && (
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
