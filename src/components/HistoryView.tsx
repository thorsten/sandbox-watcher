import { useCallback, useEffect, useState } from "react";
import { clearHistory, getHistory } from "../api";
import { AGENT_LABELS } from "../format";
import type { AgentKind, Aggregate, HistoryView as HistoryData } from "../types";
import { formatCost, formatDuration, formatTokens } from "../usageFormat";
import { describeFiles } from "../files";
import { ConfirmDialog } from "./ConfirmDialog";
import { FilesList } from "./FilesList";

const RANGES: { label: string; hours: number | null }[] = [
  { label: "Today", hours: 24 },
  { label: "7 days", hours: 24 * 7 },
  { label: "30 days", hours: 24 * 30 },
  { label: "All", hours: null },
];
const AGENTS: AgentKind[] = ["claude-code", "codex", "open-code", "pi", "shell", "other"];

export function HistoryView({ onClose }: { onClose: () => void }) {
  const [range, setRange] = useState<number | null>(24 * 7);
  const [project, setProject] = useState<string>("");
  const [agent, setAgent] = useState<AgentKind | "">("");
  const [data, setData] = useState<HistoryData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  const load = useCallback(async () => {
    try {
      setData(await getHistory(range, project || null, agent || null));
      setNow(Date.now());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [range, project, agent]);

  useEffect(() => {
    // load() awaits the command before setting state; no synchronous setState here.
    // oxlint-disable-next-line react/set-state-in-effect
    void load();
    const timer = window.setInterval(() => void load(), 15_000);
    return () => window.clearInterval(timer);
  }, [load]);

  return (
    <section className="history" aria-labelledby="history-title">
      <header className="settings-header">
        <h2 id="history-title">History</h2>
        <div className="header-meta">
          <button type="button" className="danger" onClick={() => setConfirmClear(true)}>
            Clear
          </button>
          <button type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </header>

      <div className="network-toolbar">
        <div className="segmented" aria-label="Time range">
          {RANGES.map((r) => (
            <button
              key={r.label}
              type="button"
              className={range === r.hours ? "active" : ""}
              onClick={() => setRange(r.hours)}
            >
              {r.label}
            </button>
          ))}
        </div>
        <select value={project} onChange={(e) => setProject(e.target.value)} aria-label="Project">
          <option value="">All projects</option>
          {(data?.projects ?? []).map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
        <select
          value={agent}
          onChange={(e) => setAgent(e.target.value as AgentKind | "")}
          aria-label="Agent"
        >
          <option value="">All agents</option>
          {AGENTS.map((a) => (
            <option key={a} value={a}>
              {AGENT_LABELS[a]}
            </option>
          ))}
        </select>
      </div>

      {error && <p className="network-note warn">{error}</p>}
      {data && (
        <>
          <div className="summary">
            <Tile label="Sessions" value={String(data.total.sessions)} />
            <Tile label="Agent time" value={formatDuration(data.total.seconds)} />
            <Tile label="Tokens" value={formatTokens(data.total.tokens)} />
            <Tile
              label={data.total.cost_incomplete ? "Cost (partial)" : "Cost"}
              value={data.total.cost_usd === null ? "–" : formatCost(data.total.cost_usd, "mixed")}
            />
          </div>

          <div className="history-grid">
            <AggregateTable title="By agent" rows={data.by_agent} />
            <AggregateTable title="By project" rows={data.by_project} />
          </div>

          <h3>Sessions</h3>
          {data.sessions.length === 0 ? (
            <p className="muted">Nothing recorded in this range.</p>
          ) : (
            <table className="network-table">
              <thead>
                <tr>
                  <th>Started</th>
                  <th>Duration</th>
                  <th>Project</th>
                  <th>Agent</th>
                  <th>Command</th>
                  <th>Files</th>
                  <th className="num">Tokens</th>
                  <th className="num">Cost</th>
                </tr>
              </thead>
              <tbody>
                {data.sessions.map((s) => (
                  <tr key={s.id} className={s.ended_at ? "" : "row-live"}>
                    <td>{new Date(s.started_at).toLocaleString()}</td>
                    <td>
                      {formatDuration(
                        Math.max(
                          0,
                          Math.floor(
                            ((s.ended_at ? Date.parse(s.ended_at) : now) -
                              Date.parse(s.started_at)) /
                              1000,
                          ),
                        ),
                      )}
                      {!s.ended_at && <span className="badge badge-ok badge-inline">live</span>}
                    </td>
                    <td>{s.project_slug}</td>
                    <td>
                      <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
                    </td>
                    <td className="path">
                      <code title={s.command}>{s.command}</code>
                    </td>
                    <td>
                      {s.files.length === 0 ? (
                        <span className="muted">–</span>
                      ) : (
                        <details>
                          <summary>{describeFiles(s.files)}</summary>
                          <FilesList
                            files={s.files}
                            projectDir={null}
                            now={new Date(now)}
                            collapseReads={false}
                          />
                        </details>
                      )}
                    </td>
                    <td className="num">
                      {s.usage
                        ? formatTokens(
                            s.usage.input_tokens +
                              s.usage.output_tokens +
                              s.usage.cache_read_tokens +
                              s.usage.cache_write_tokens,
                          )
                        : "–"}
                      {s.usage_scope === "agent" && (
                        <span
                          className="muted"
                          title="Transcripts that several sessions of this agent could have written are counted here"
                        >
                          {" "}
                          shared
                        </span>
                      )}
                      {s.usage_scope === "pooled" && (
                        <span
                          className="muted"
                          title="Counted in the sibling session marked shared"
                        >
                          {" "}
                          pooled
                        </span>
                      )}
                    </td>
                    <td className="num">
                      {s.usage ? formatCost(s.usage.cost_usd, s.usage.cost_source) : "–"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </>
      )}

      {confirmClear && (
        <ConfirmDialog
          title="Clear the history?"
          lines={["All recorded sessions and containers are deleted from this machine."]}
          confirmLabel="Clear history"
          onCancel={() => setConfirmClear(false)}
          onConfirm={() => {
            setConfirmClear(false);
            void clearHistory().then(load);
          }}
        />
      )}
    </section>
  );
}

function Tile({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <span className="stat-value">{value}</span>
      <span className="stat-label">{label}</span>
    </div>
  );
}

function AggregateTable({ title, rows }: { title: string; rows: Aggregate[] }) {
  return (
    <div>
      <h3>{title}</h3>
      {rows.length === 0 ? (
        <p className="muted">–</p>
      ) : (
        <table className="network-table">
          <thead>
            <tr>
              <th>{title.replace("By ", "")}</th>
              <th className="num">Sessions</th>
              <th className="num">Time</th>
              <th className="num">Tokens</th>
              <th className="num">Cost</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.key}>
                <td>{r.key}</td>
                <td className="num">{r.sessions}</td>
                <td className="num">{formatDuration(r.seconds)}</td>
                <td className="num">{formatTokens(r.tokens)}</td>
                <td className="num">
                  {r.cost_usd === null ? "–" : formatCost(r.cost_usd, "mixed")}
                  {r.cost_incomplete && (
                    <span className="muted" title="Some sessions had no price for their model">
                      {" "}
                      *
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
