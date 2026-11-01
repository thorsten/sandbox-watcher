import { useEffect, useRef, useState } from "react";
import { diagnosticsText, runDiagnostics } from "../api";
import { RUNTIME_LABELS, type DiagnosticsReport, type Probe } from "../types";

function outcomeText(p: Probe): { label: string; detail: string } {
  switch (p.outcome) {
    case "found":
      return { label: "found", detail: p.endpoint };
    case "missing":
      return { label: "missing", detail: "" };
    case "skipped":
      return { label: "skipped", detail: p.reason };
    case "failed":
      return { label: "failed", detail: p.reason };
  }
}

const ORDER = { found: 0, failed: 1, skipped: 2, missing: 3 } as const;

export function DiagnosticsReportView({ report }: { report: DiagnosticsReport }) {
  const env = report.environment;
  const local = [...report.local].toSorted((a, b) => ORDER[a.outcome] - ORDER[b.outcome]);
  return (
    <div className="diagnostics">
      <h4>Environment</h4>
      <dl className="diagnostics-env">
        <dt>OS</dt>
        <dd>{env.os}</dd>
        <dt>DOCKER_HOST</dt>
        <dd>{env.docker_host ?? <span className="muted">not set</span>}</dd>
        <dt>CONTAINER_HOST</dt>
        <dd>{env.container_host ?? <span className="muted">not set</span>}</dd>
        <dt>SANDBOX_WATCHER_RUNTIME</dt>
        <dd>{env.runtime_override ?? <span className="muted">not set</span>}</dd>
        <dt>sandbox config</dt>
        <dd>
          <code>{env.config_path || "none"}</code>
          {env.config_runtime && <span>, runtime = {env.config_runtime}</span>}
        </dd>
        <dt>on PATH</dt>
        <dd>{env.binaries.map((b) => `${b.name} ${b.found ? "yes" : "no"}`).join(", ")}</dd>
      </dl>
      <h4>Local runtimes</h4>
      <table className="diagnostics-table">
        <thead>
          <tr>
            <th>Runtime</th>
            <th>Candidate</th>
            <th>Outcome</th>
            <th>Detail</th>
          </tr>
        </thead>
        <tbody>
          {local.map((p, i) => {
            const o = outcomeText(p);
            return (
              <tr key={`${p.kind}-${p.candidate}-${i}`}>
                <td>{RUNTIME_LABELS[p.kind]}</td>
                <td>
                  <code>{p.candidate}</code>
                </td>
                <td>
                  <span
                    className={`badge badge-${p.outcome === "found" ? "ok" : p.outcome === "failed" ? "err" : "muted"}`}
                  >
                    {o.label}
                  </span>
                </td>
                <td>{o.detail}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {report.hosts.map((h) => (
        <section key={h.alias}>
          <h4>
            Remote host {h.status.display_name}
            {h.status.display_name !== h.alias && <span className="muted"> ({h.alias})</span>}
          </h4>
          <p>
            {h.status.state === "unreachable"
              ? `unreachable since ${new Date(h.status.since).toLocaleString()}: ${h.status.reason}`
              : h.status.state}
          </p>
          {h.status.probes.length > 0 && (
            <ul>
              {h.status.probes.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </div>
  );
}

export function DiagnosticsSection({
  initialReport = null,
  focus = false,
}: {
  initialReport?: DiagnosticsReport | null;
  focus?: boolean;
}) {
  const [report, setReport] = useState<DiagnosticsReport | null>(initialReport);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    if (focus) ref.current?.scrollIntoView({ block: "start" });
  }, [focus]);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      setReport(await runDiagnostics());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  const copy = async () => {
    if (!report) return;
    await navigator.clipboard.writeText(await diagnosticsText(report));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <section className="settings-section" id="diagnostics" ref={ref}>
      <h3>Diagnostics</h3>
      <p className="muted small">
        Probes every place a runtime could be, locally and on each remote host, and shows what
        answered. Nothing is sent anywhere; copy the report into an issue yourself.
      </p>
      <div className="settings-row">
        <button type="button" onClick={() => void run()} disabled={running}>
          {running ? "Running…" : "Run diagnostics"}
        </button>
        {report && (
          <button type="button" onClick={() => void copy()}>
            {copied ? "Copied" : "Copy report"}
          </button>
        )}
        {report && (
          <span className="muted small">{new Date(report.generated_at).toLocaleString()}</span>
        )}
      </div>
      {error && <p className="notice notice-err">{error}</p>}
      {report && <DiagnosticsReportView report={report} />}
    </section>
  );
}
