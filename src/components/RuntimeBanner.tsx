import { hostLabel } from "../hosts";
import { RUNTIME_LABELS, type HostStatus, type RuntimeKind, type RuntimeStatus } from "../types";

/** "Docker" for this machine, "Docker on <host>" for a remote one. */
function runtimeLabel(host: string, kind: RuntimeKind, hosts: HostStatus[]): string {
  const label = RUNTIME_LABELS[kind];
  return host === "local" ? label : `${label} on ${hostLabel(host, hosts)}`;
}

/** One warning per unreachable remote host, with the probe report folded away. */
function HostWarnings({ hosts }: { hosts: HostStatus[] }) {
  const down = hosts.filter((h) => h.state === "unreachable");
  if (down.length === 0) return null;
  return (
    <>
      {down.map((h) => (
        <section key={h.alias} className="banner banner-warn host-warning">
          {h.display_name || h.alias}: {h.state === "unreachable" ? h.reason : ""}
          {h.probes.length > 0 && (
            <details>
              <summary className="muted small">Probes</summary>
              <ul>
                {h.probes.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </details>
          )}
        </section>
      ))}
    </>
  );
}

export function RuntimeBanner({
  runtime,
  hosts = [],
  compact,
  onDiagnostics,
}: {
  runtime: RuntimeStatus;
  hosts?: HostStatus[];
  compact?: boolean;
  onDiagnostics?: () => void;
}) {
  return (
    <>
      <RuntimeStatusBanner
        runtime={runtime}
        hosts={hosts}
        compact={compact}
        onDiagnostics={onDiagnostics}
      />
      <HostWarnings hosts={hosts} />
    </>
  );
}

function RuntimeStatusBanner({
  runtime,
  hosts,
  compact,
  onDiagnostics,
}: {
  runtime: RuntimeStatus;
  hosts: HostStatus[];
  compact?: boolean;
  onDiagnostics?: () => void;
}) {
  if (compact && runtime.status === "connected") {
    return (
      <div className="runtime-compact">
        {runtime.runtimes.map((r) => (
          <span
            key={`${r.host}/${r.kind}`}
            className="runtime-item"
            // A remote endpoint is a local forward socket; the host says more.
            title={`${r.host === "local" ? r.endpoint : hostLabel(r.host, hosts)}${r.version ? `, ${r.version}` : ""}`}
          >
            <span className="dot dot-ok" aria-hidden="true" />
            {runtimeLabel(r.host, r.kind, hosts)}
          </span>
        ))}
        {runtime.failures.map((f) => (
          <span key={`${f.host}/${f.kind}`} className="runtime-item muted" title={f.reason}>
            <span className="dot dot-off" aria-hidden="true" />
            {runtimeLabel(f.host, f.kind, hosts)}
          </span>
        ))}
      </div>
    );
  }
  switch (runtime.status) {
    case "not-detected":
      return (
        <section className="banner banner-warn">
          <strong>No container runtime detected.</strong>
          <p>
            Sandbox Watcher looks for Docker, Podman and Apple <code>container</code> in the same
            order sandbox does. Install one, or set <code>DOCKER_HOST</code> if your socket lives
            somewhere unusual, then press Refresh.
          </p>
          {onDiagnostics && (
            <p>
              <button type="button" onClick={onDiagnostics}>
                Diagnostics
              </button>
            </p>
          )}
        </section>
      );
    case "detected-only":
      return (
        <section className="banner banner-info">
          <strong>Runtime found, but could not connect.</strong>
          <ul>
            {runtime.candidates.map((c) => (
              <li key={`${c.kind}-${c.endpoint}`}>
                {RUNTIME_LABELS[c.kind]} at <code>{c.endpoint}</code>{" "}
                <span className="muted">({c.source})</span>
              </li>
            ))}
          </ul>
          <p className="muted">
            Docker and Podman are supported today. Apple <code>container</code> support arrives with
            milestone M5.
          </p>
          {onDiagnostics && (
            <p>
              <button type="button" onClick={onDiagnostics}>
                Diagnostics
              </button>
            </p>
          )}
        </section>
      );
    case "unavailable":
      return (
        <section className="banner banner-error">
          <strong>Runtime detected but not reachable.</strong>
          <pre>{runtime.reason}</pre>
          <p className="muted">
            Is the daemon running? On Linux, check that your user may access the socket.
          </p>
          {onDiagnostics && (
            <p>
              <button type="button" onClick={onDiagnostics}>
                Diagnostics
              </button>
            </p>
          )}
        </section>
      );
    case "connected":
      return (
        <section className="banner banner-ok">
          <div className="runtime-list">
            {runtime.runtimes.map((r) => (
              <span key={`${r.host}/${r.kind}`} className="runtime-item">
                <span className="dot dot-ok" aria-hidden="true" />
                {runtimeLabel(r.host, r.kind, hosts)}
                {r.version && <span className="muted"> {r.version}</span>}
                {r.host === "local" && <span className="muted"> via {r.endpoint}</span>}
              </span>
            ))}
          </div>
          {runtime.failures.length > 0 && (
            <ul className="runtime-failures muted">
              {runtime.failures.map((f) => (
                <li key={`${f.host}/${f.kind}`}>
                  {runtimeLabel(f.host, f.kind, hosts)} detected but not reachable: {f.reason}
                </li>
              ))}
            </ul>
          )}
        </section>
      );
  }
}
