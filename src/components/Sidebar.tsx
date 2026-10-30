import { AGENT_LABELS, formatUptime, groupByProject, staleHashes } from "../format";
import {
  RUNTIME_LABELS,
  type NetworkReport,
  type RuntimeStatus,
  type SandboxContainer,
} from "../types";
import { RuntimeBanner } from "./RuntimeBanner";

interface Props {
  containers: SandboxContainer[];
  runtime: RuntimeStatus;
  network: NetworkReport[] | undefined;
  selectedId: string | null;
  onSelect: (id: string) => void;
  now: Date;
}

export function Sidebar({ containers, runtime, network, selectedId, onSelect, now }: Props) {
  const showRuntime = runtime.status === "connected" && runtime.runtimes.length > 1;
  const stale = staleHashes(containers);
  const blockedFor = (id: string) =>
    network?.find((r) => r.container_id === id)?.entries.filter((e) => e.status === "blocked")
      .length ?? 0;

  const groups = [...groupByProject(containers)];
  const running = groups.filter(([, g]) => g.some((c) => c.state === "running"));
  const stopped = groups.filter(([, g]) => g.every((c) => c.state !== "running"));

  return (
    <aside className="sidebar">
      <RuntimeBanner runtime={runtime} compact />
      {runtime.status === "connected" && containers.length === 0 && (
        <div className="sidebar-empty muted">
          No sandbox containers. Start one with <code>sandbox run claude</code>.
        </div>
      )}
      {running.map(([slug, group]) => (
        <section key={slug} className="project-group">
          <h2 title={slug}>{slug}</h2>
          {group.map((c) => (
            <ContainerCard
              key={c.id}
              container={c}
              selected={c.id === selectedId}
              stale={stale.has(c.id)}
              blocked={blockedFor(c.id)}
              showRuntime={showRuntime}
              now={now}
              onSelect={onSelect}
            />
          ))}
        </section>
      ))}
      {stopped.length > 0 && (
        <section className="project-group stopped-group">
          <h2 className="muted">Stopped</h2>
          {stopped.flatMap(([, group]) =>
            group.map((c) => (
              <ContainerCard
                key={c.id}
                container={c}
                selected={c.id === selectedId}
                stale={false}
                blocked={0}
                showRuntime={showRuntime}
                now={now}
                onSelect={onSelect}
              />
            )),
          )}
        </section>
      )}
    </aside>
  );
}

function ContainerCard({
  container,
  selected,
  stale,
  blocked,
  showRuntime,
  now,
  onSelect,
}: {
  container: SandboxContainer;
  selected: boolean;
  stale: boolean;
  blocked: number;
  showRuntime: boolean;
  now: Date;
  onSelect: (id: string) => void;
}) {
  const isRunning = container.state === "running";
  const starting = isRunning && container.readiness === "starting";
  const dot = !isRunning ? "dot-off" : starting ? "dot-warn" : "dot-ok";
  const agents = new Map<string, number>();
  for (const s of container.sessions) agents.set(s.agent, (agents.get(s.agent) ?? 0) + 1);

  return (
    <button
      type="button"
      className={`card ${selected ? "selected" : ""} ${isRunning ? "" : "card-stopped"}`}
      onClick={() => onSelect(container.id)}
      aria-pressed={selected}
    >
      <div className="card-title">
        <span className={`dot ${dot}`} aria-hidden="true" />
        <strong title={container.name}>{container.name}</strong>
      </div>
      <div className="card-meta muted">
        {isRunning ? formatUptime(container.started_at, now) : container.state}
        {showRuntime && <> · {RUNTIME_LABELS[container.runtime]}</>}
        {container.project_slug !== container.name && isRunning ? null : null}
      </div>
      <div className="card-chips">
        {[...agents].map(([agent, n]) => (
          <span key={agent} className={`chip agent-${agent}`}>
            {AGENT_LABELS[agent as keyof typeof AGENT_LABELS]}
            {n > 1 && <strong>{n}</strong>}
          </span>
        ))}
        {isRunning && container.sessions.length === 0 && <span className="muted small">idle</span>}
        {blocked > 0 && <span className="badge badge-err">{blocked} blocked</span>}
        {starting && <span className="badge badge-warn">starting</span>}
        {stale && <span className="badge badge-warn">stale</span>}
      </div>
    </button>
  );
}
