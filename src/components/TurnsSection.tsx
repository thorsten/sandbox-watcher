import { useState } from "react";
import { getTurns } from "../api";
import { formatRelative } from "../network";
import { turnFacts, turnLabel } from "../turns";
import type { AgentKind, TurnReport } from "../types";
import { FilesList } from "./FilesList";

interface Props {
  containerKey: string;
  agent: AgentKind;
  projectDir: string | null;
  now: Date;
}

/** Loads on request: the whole transcript is read, not just its tail. */
export function TurnsSection({ containerKey, agent, projectDir, now }: Props) {
  const [report, setReport] = useState<TurnReport | null | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    setError(null);
    getTurns(containerKey, agent)
      .then(setReport)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  };

  return (
    <div className="turns">
      <div className="files-head">
        <h4 className="files-title">Turns</h4>
        <button type="button" className="link small" onClick={load} disabled={loading}>
          {loading ? "Reading transcript…" : report === undefined ? "Show turns" : "Refresh"}
        </button>
      </div>
      {error && <p className="notice notice-err">{error}</p>}
      {report === null && <p className="muted small">No transcript found for this agent.</p>}
      {report && (
        <TurnsList report={report} containerKey={containerKey} projectDir={projectDir} now={now} />
      )}
    </div>
  );
}

export function TurnsList({
  report,
  containerKey,
  projectDir,
  now,
}: {
  report: TurnReport;
  containerKey?: string;
  projectDir: string | null;
  now: Date;
}) {
  if (report.turns.length === 0) return <p className="muted small">No prompts yet.</p>;
  return (
    <>
      <p className="muted small">
        {report.turns.length} {report.turns.length === 1 ? "turn" : "turns"}, newest first
        {report.truncated && " · older turns not shown"}
      </p>
      <ol className="turn-list">
        {report.turns.toReversed().map((t) => (
          <li key={t.index} className="turn">
            <details>
              <summary>
                <span className="turn-label muted">{turnLabel(t)}</span>
                <span className="turn-prompt" title={t.prompt}>
                  {t.prompt || "Activity before the first prompt read"}
                </span>
                <span className="turn-time muted small">{formatRelative(t.started_at, now)}</span>
              </summary>
              <div className="turn-body">
                <p className="turn-facts muted small">
                  {turnFacts(t).join(" · ") || "Nothing recorded."}
                  {t.usage && t.usage.models.length > 0 && (
                    <>
                      {" "}
                      <code>{t.usage.models.join(", ")}</code>
                    </>
                  )}
                </p>
                <FilesList
                  files={t.files}
                  containerKey={containerKey}
                  projectDir={projectDir}
                  now={now}
                />
              </div>
            </details>
          </li>
        ))}
      </ol>
    </>
  );
}
