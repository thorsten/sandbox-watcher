import { useEffect, useState } from "react";
import { getContainerDetails, revealMount } from "../api";
import type { ContainerDetails, MountRole } from "../types";

const ROLE_LABELS: Record<MountRole, string> = {
  project: "project",
  persisted: "persisted",
  runtime: "runtime",
  other: "extra",
};

const ROLE_ORDER: MountRole[] = ["project", "persisted", "other", "runtime"];

export function MountsPanel({ containerKey }: { containerKey: string }) {
  const [details, setDetails] = useState<ContainerDetails | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const remote = containerKey.includes("/");

  useEffect(() => {
    let cancelled = false;
    getContainerDetails(containerKey).then(
      (d) => {
        if (!cancelled) setDetails(d);
      },
      (e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [containerKey]);

  if (error) return <p className="network-note warn">{error}</p>;
  if (details === undefined) return <div className="mounts muted">Inspecting container…</div>;
  if (details === null) return <div className="mounts muted">Container is gone.</div>;

  const mounts = [...details.mounts].toSorted(
    (a, b) => ROLE_ORDER.indexOf(a.role) - ROLE_ORDER.indexOf(b.role),
  );

  return (
    <div className="mounts">
      <table className="network-table">
        <thead>
          <tr>
            <th>Role</th>
            <th>Host path</th>
            <th>In container</th>
            <th>Access</th>
            <th aria-label="Actions" />
          </tr>
        </thead>
        <tbody>
          {mounts.map((m) => (
            <tr key={`${m.source}:${m.destination}`}>
              <td>
                <span className={`chip role-${m.role}`}>{ROLE_LABELS[m.role]}</span>
              </td>
              <td className="path" title={m.source}>
                {m.type === "volume" ? (
                  <span>
                    volume <code>{m.source}</code>
                  </span>
                ) : (
                  <code>{m.host_path ?? m.source}</code>
                )}
              </td>
              <td className="path">
                <code>{m.destination}</code>
              </td>
              <td>{m.read_only ? "read-only" : "writable"}</td>
              <td>
                {m.type === "bind" && !remote && (
                  <button
                    type="button"
                    className="link"
                    onClick={() => void revealMount(containerKey, m.source).catch(() => undefined)}
                  >
                    Show
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      <dl className="kv">
        <dt>Working dir</dt>
        <dd>
          <code>{details.working_dir ?? "–"}</code>
        </dd>
        <dt>User</dt>
        <dd>{details.user ?? "–"}</dd>
        <dt>Ports</dt>
        <dd>
          {details.ports.length === 0
            ? "none"
            : details.ports
                .map((p) =>
                  p.host_port
                    ? `${p.host_port} → ${p.container_port}/${p.protocol}`
                    : `${p.container_port}/${p.protocol} (not published)`,
                )
                .join(", ")}
        </dd>
        <dt>Env names</dt>
        <dd className="env-names">
          {details.env_names.length === 0
            ? "none"
            : details.env_names.map((n) => <code key={n}>{n}</code>)}
          <span className="muted"> Values are never read.</span>
        </dd>
      </dl>
    </div>
  );
}
