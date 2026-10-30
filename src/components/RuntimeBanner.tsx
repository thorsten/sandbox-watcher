import type { RuntimeStatus } from "../types";

export function RuntimeBanner({ runtime }: { runtime: RuntimeStatus }) {
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
        </section>
      );
    case "detected-only":
      return (
        <section className="banner banner-info">
          <strong>Runtime found, adapter not wired yet.</strong>
          <ul>
            {runtime.candidates.map((c) => (
              <li key={`${c.kind}-${c.endpoint}`}>
                <code>{c.kind}</code> at <code>{c.endpoint}</code>{" "}
                <span className="muted">({c.source})</span>
              </li>
            ))}
          </ul>
          <p className="muted">
            Listing containers arrives with milestone M1. Run with{" "}
            <code>SANDBOX_WATCHER_MOCK=1</code> to see demo data.
          </p>
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
        </section>
      );
    case "connected":
      return (
        <section className="banner banner-ok">
          <span className="dot dot-ok" aria-hidden="true" />
          Connected to <code>{runtime.info.kind}</code>
          {runtime.info.version && <span className="muted"> {runtime.info.version}</span>}
          <span className="muted"> via {runtime.info.endpoint}</span>
        </section>
      );
  }
}
