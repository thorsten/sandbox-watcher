import { useState } from "react";
import { getTurns } from "../api";
import { t } from "../i18n";
import { formatRelative } from "../network";
import { turnFacts, turnLabel } from "../turns";
import type { AgentKind, TurnReport } from "../types";
import { FilesList } from "./FilesList";
import { describeError } from "../errors";

interface Props {
  containerKey: string;
  agent: AgentKind;
  /** Session whose transcript to read; without it, the agent's newest. */
  pid?: number | null;
  projectDir: string | null;
  now: Date;
}

/** Loads on request: the whole transcript is read, not just its tail. */
export function TurnsSection({ containerKey, agent, pid = null, projectDir, now }: Props) {
  const [report, setReport] = useState<TurnReport | null | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    setError(null);
    getTurns(containerKey, agent, pid)
      .then(setReport)
      .catch((e: unknown) => setError(describeError(e)))
      .finally(() => setLoading(false));
  };

  return (
    <div className="turns">
      <div className="files-head">
        <h4 className="files-title">{t("turns.title")}</h4>
        <button type="button" className="link small" onClick={load} disabled={loading}>
          {loading ? t("turns.reading") : report === undefined ? t("turns.show") : t("app.refresh")}
        </button>
      </div>
      {error && <p className="notice notice-err">{error}</p>}
      {report === null && <p className="muted small">{t("turns.noTranscript")}</p>}
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
  if (report.turns.length === 0) return <p className="muted small">{t("turns.noPrompts")}</p>;
  return (
    <>
      <p className="muted small">
        {t("turns.count", { n: report.turns.length })}
        {report.truncated && ` · ${t("turns.olderHidden")}`}
      </p>
      <ol className="turn-list">
        {report.turns.toReversed().map((turn) => (
          <li key={turn.index} className="turn">
            <details>
              <summary>
                <span className="turn-label muted">{turnLabel(turn)}</span>
                <span className="turn-prompt" title={turn.prompt}>
                  {turn.prompt || t("turns.beforeFirst")}
                </span>
                <span className="turn-time muted small">
                  {formatRelative(turn.started_at, now)}
                </span>
              </summary>
              <div className="turn-body">
                <p className="turn-facts muted small">
                  {turnFacts(turn).join(" · ") || t("turns.nothing")}
                  {turn.usage && turn.usage.models.length > 0 && (
                    <>
                      {" "}
                      <code>{turn.usage.models.join(", ")}</code>
                    </>
                  )}
                </p>
                <FilesList
                  files={turn.files}
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
