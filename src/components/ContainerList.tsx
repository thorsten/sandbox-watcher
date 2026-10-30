import { formatUptime, groupByProject, staleHashes } from "../format";
import type { RuntimeStatus, SandboxContainer } from "../types";

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
  const running = containers.filter((c) => c.state === "running").length;

  return (
    <section>
      <div className="summary">
        <Stat label="Projects" value={groupByProject(containers).size} />
        <Stat label="Running" value={running} />
        <Stat label="Stopped" value={containers.length - running} />
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
  return (
    <li className={`container ${isRunning ? "" : "container-stopped"}`}>
      <span className={`dot ${isRunning ? "dot-ok" : "dot-off"}`} aria-hidden="true" />
      <div className="container-main">
        <div className="container-title">
          <strong>{container.name}</strong>
          <span className="muted">
            {isRunning ? formatUptime(container.started_at, now) : container.state}
          </span>
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
      </div>
    </li>
  );
}
