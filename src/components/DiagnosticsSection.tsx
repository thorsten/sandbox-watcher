import { useEffect, useRef, useState } from "react";
import { diagnosticsText, runDiagnostics } from "../api";
import { formatDateTime, t } from "../i18n";
import { RUNTIME_LABELS, type DiagnosticsReport, type Probe } from "../types";

function outcomeText(p: Probe): { label: string; detail: string } {
  switch (p.outcome) {
    case "found":
      return { label: t("outcome.found"), detail: p.endpoint };
    case "missing":
      return { label: t("outcome.missing"), detail: "" };
    case "skipped":
      return { label: t("outcome.skipped"), detail: p.reason };
    case "failed":
      return { label: t("outcome.failed"), detail: p.reason };
  }
}

const ORDER = { found: 0, failed: 1, skipped: 2, missing: 3 } as const;

export function DiagnosticsReportView({ report }: { report: DiagnosticsReport }) {
  const env = report.environment;
  const local = [...report.local].toSorted((a, b) => ORDER[a.outcome] - ORDER[b.outcome]);
  return (
    <div className="diagnostics">
      <h4>{t("diag.environment")}</h4>
      <dl className="diagnostics-env">
        <dt>{t("diag.os")}</dt>
        <dd>{env.os}</dd>
        <dt>DOCKER_HOST</dt>
        <dd>{env.docker_host ?? <span className="muted">{t("diag.notSet")}</span>}</dd>
        <dt>CONTAINER_HOST</dt>
        <dd>{env.container_host ?? <span className="muted">{t("diag.notSet")}</span>}</dd>
        <dt>SANDBOX_WATCHER_RUNTIME</dt>
        <dd>{env.runtime_override ?? <span className="muted">{t("diag.notSet")}</span>}</dd>
        <dt>{t("diag.sandboxConfig")}</dt>
        <dd>
          <code>{env.config_path || t("diag.none")}</code>
          {env.config_runtime && <span>, runtime = {env.config_runtime}</span>}
        </dd>
        <dt>{t("diag.onPath")}</dt>
        <dd>
          {env.binaries
            .map((b) => `${b.name} ${b.found ? t("diag.yes") : t("diag.no")}`)
            .join(", ")}
        </dd>
      </dl>
      <h4>{t("diag.localRuntimes")}</h4>
      <table className="diagnostics-table">
        <thead>
          <tr>
            <th>{t("diag.runtime")}</th>
            <th>{t("diag.candidate")}</th>
            <th>{t("diag.outcome")}</th>
            <th>{t("diag.detail")}</th>
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
            {t("diag.remoteHost", { name: h.status.display_name })}
            {h.status.display_name !== h.alias && <span className="muted"> ({h.alias})</span>}
          </h4>
          <p>
            {h.status.state === "unreachable"
              ? t("diag.unreachableSince", {
                  since: formatDateTime(h.status.since),
                  reason: h.status.reason,
                })
              : t(`hoststate.${h.status.state}`)}
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
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (focus) ref.current?.scrollIntoView({ block: "start" });
  }, [focus]);

  useEffect(() => () => clearTimeout(copiedTimer.current), []);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      setReport(await runDiagnostics());
      setCopied(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  const copy = async () => {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(await diagnosticsText(report));
      setError(null);
      setCopied(true);
      clearTimeout(copiedTimer.current);
      copiedTimer.current = setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      setCopied(false);
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="settings-section" id="diagnostics" ref={ref}>
      <h3>{t("runtime.diagnostics")}</h3>
      <p className="muted small">{t("diag.intro")}</p>
      <div className="settings-row">
        <button type="button" onClick={() => void run()} disabled={running}>
          {running ? t("diag.running") : t("diag.run")}
        </button>
        {report && (
          <button type="button" onClick={() => void copy()}>
            {copied ? t("actions.copied") : t("diag.copyReport")}
          </button>
        )}
        {report && <span className="muted small">{formatDateTime(report.generated_at)}</span>}
      </div>
      {error && <p className="notice notice-err">{error}</p>}
      {report && <DiagnosticsReportView report={report} />}
    </section>
  );
}
