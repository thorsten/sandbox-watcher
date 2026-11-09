import { useCallback } from "react";
import { getActivity } from "../api";
import {
  describeAttribution,
  describeCoverage,
  sessionsWithoutTranscript,
  transcriptTitle,
} from "../activity";
import { AGENT_LABELS } from "../format";
import { t } from "../i18n";
import { FilesList } from "./FilesList";
import { TurnsSection } from "./TurnsSection";
import { formatRelative } from "../network";
import { formatBytes } from "../sparkline";
import { formatCost, formatTokens, totalTokens } from "../usageFormat";
import { usePolling } from "../usePolling";
import type { ActivityEvent, ProcessInfo, SessionProcesses, Usage } from "../types";

/** Each poll costs the container three commands (processes, transcript tail, git status). */
export const POLL_MS = 5_000;

function formatElapsed(secs: number): string {
  if (secs < 60) return t("duration.seconds", { n: secs });
  const m = Math.floor(secs / 60);
  if (m < 60) return t("elapsed.minutesSeconds", { m, s: secs % 60 });
  const h = Math.floor(m / 60);
  return t("duration.hoursMinutes", { h, m: m % 60 });
}

type Change = "new" | "modified" | "staged" | "added" | "deleted" | "renamed";
/** `git status --porcelain` codes to a change kind; the kind is also the CSS class. */
const CHANGE_KINDS: Record<string, Change> = {
  "??": "new",
  " M": "modified",
  "M ": "staged",
  MM: "modified",
  "A ": "added",
  AM: "added",
  " D": "deleted",
  "D ": "deleted",
  R: "renamed",
};

export function ActivityPanel({ containerKey, now }: { containerKey: string; now: Date }) {
  const load = useCallback(() => getActivity(containerKey), [containerKey]);
  const { data: activity, error } = usePolling(load, POLL_MS);

  // A failed refresh keeps the last activity on screen; the note says the data may be stale.
  const note = error !== null ? <p className="network-note warn">{error}</p> : null;
  if (!activity) return note ?? <div className="activity muted">{t("activity.looking")}</div>;

  return (
    <div className="activity">
      {note}
      {activity.agents.map((agent) => (
        <section key={agent.source} className="activity-block">
          <h3>
            {transcriptTitle(agent)}
            {agent.updated_at && (
              <span className="muted">
                {" "}
                · {t("activity.lastActivity", { when: formatRelative(agent.updated_at, now) })}
              </span>
            )}
          </h3>
          <p className="muted small" title={agent.source}>
            {describeAttribution(agent.attribution)}
          </p>
          {describeCoverage(agent.coverage) && (
            <p className="network-note warn">{describeCoverage(agent.coverage)}</p>
          )}
          {agent.usage && <UsageLine usage={agent.usage} label={t("activity.wholeTranscript")} />}
          {agent.session_usage && (
            <UsageLine
              usage={agent.session_usage}
              label={t("activity.thisSession")}
              note={t("activity.sessionNote")}
            />
          )}
          {agent.in_flight ? (
            <div className="in-flight">
              <span className="pulse" aria-hidden="true" />
              <strong>{agent.in_flight.label}</strong>
              <code title={agent.in_flight.detail}>{agent.in_flight.detail}</code>
            </div>
          ) : (
            <div className="muted small">{t("activity.waiting")}</div>
          )}
          <ol className="timeline">
            {agent.events.toReversed().map((e, i) => (
              <EventRow key={`${e.tool_use_id ?? ""}-${e.at ?? ""}-${i}`} event={e} now={now} />
            ))}
          </ol>
          <h4 className="files-title">{t("activity.files")}</h4>
          <FilesList
            files={agent.files}
            containerKey={containerKey}
            projectDir={activity.project_dir}
            now={now}
          />
          <TurnsSection
            containerKey={containerKey}
            agent={agent.agent}
            pid={agent.attribution.kind === "session" ? agent.attribution.pid : null}
            projectDir={activity.project_dir}
            now={now}
          />
        </section>
      ))}

      {sessionsWithoutTranscript(activity).map((s) => (
        <p key={s.pid} className="muted small">
          {t("activity.noTranscript", { agent: AGENT_LABELS[s.agent], pid: String(s.pid) })}
        </p>
      ))}

      {activity.sessions.map((s) => (
        <SessionBlock key={s.session.pid} session={s} now={now} />
      ))}

      {activity.working_tree && (
        <section className="activity-block">
          <h3>
            {t("activity.changes")}
            {activity.working_tree.branch && (
              <span className="muted">
                {" "}
                · {t("activity.branch", { branch: activity.working_tree.branch })}
              </span>
            )}
          </h3>
          {!activity.working_tree.is_repo && <p className="muted small">{t("activity.notRepo")}</p>}
          {activity.working_tree.is_repo && activity.working_tree.changes.length === 0 && (
            <p className="muted small">{t("activity.clean")}</p>
          )}
          {activity.working_tree.changes.length > 0 && (
            <>
              <ul className="changes">
                {activity.working_tree.changes.map((c) => {
                  const kind = CHANGE_KINDS[c.status];
                  return (
                    <li key={`${c.status}${c.path}`}>
                      <span className={`chip change-${kind ?? "other"}`}>
                        {kind ? t(`change.${kind}`) : c.status.trim()}
                      </span>
                      <code>{c.path}</code>
                    </li>
                  );
                })}
              </ul>
              <p className="muted small">
                {activity.working_tree.shortstat}
                {activity.working_tree.truncated ? ` · ${t("activity.listTruncated")}` : ""}
              </p>
            </>
          )}
        </section>
      )}

      {activity.agents.length === 0 && activity.sessions.length === 0 && (
        <p className="muted">{t("activity.noSessions")}</p>
      )}
      {activity.errors.length > 0 && (
        <p className="network-note warn">{activity.errors.join("; ")}</p>
      )}
    </div>
  );
}

function UsageLine({ usage, label, note }: { usage: Usage; label: string; note?: string }) {
  return (
    <div className="usage-line muted" title={note}>
      <span className="small">{label}:</span>
      <span title={t("usage.tokensTitle")}>
        {t("usage.tokens", { tokens: formatTokens(totalTokens(usage)) })}
        <span className="small">
          {" "}
          ({t("usage.in", { n: formatTokens(usage.input_tokens) })} ·{" "}
          {t("usage.out", { n: formatTokens(usage.output_tokens) })}
          {usage.cache_read_tokens > 0 && (
            <> · {t("usage.cache", { n: formatTokens(usage.cache_read_tokens) })}</>
          )}
          )
        </span>
      </span>
      <span
        title={
          usage.cost_source === "estimated"
            ? t("usage.estimated")
            : usage.cost_source === "reported"
              ? t("usage.reported")
              : t("usage.noPriceTitle")
        }
      >
        {formatCost(usage.cost_usd, usage.cost_source)}
      </span>
      {usage.models.length > 0 && <code>{usage.models.join(", ")}</code>}
      <span className="small">{t("usage.turns", { n: usage.turns })}</span>
    </div>
  );
}

function EventRow({ event, now }: { event: ActivityEvent; now: Date }) {
  const icon = {
    "user-message": "›",
    "assistant-text": "✦",
    "tool-use": "▶",
    "tool-result": "✓",
    "tool-error": "✗",
  }[event.kind];
  return (
    <li className={`event event-${event.kind}`}>
      <span className="event-icon" aria-hidden="true">
        {icon}
      </span>
      <span className="event-label">{event.label}</span>
      <span className="event-detail" title={event.detail}>
        {event.detail}
      </span>
      <span className="event-time muted">{formatRelative(event.at, now)}</span>
    </li>
  );
}

function SessionBlock({ session, now }: { session: SessionProcesses; now: Date }) {
  const s = session.session;
  return (
    <section className="activity-block">
      <h3>
        <span className={`chip agent-${s.agent}`}>{AGENT_LABELS[s.agent]}</span>
        <code className="muted">{s.command}</code>
        <span className="muted"> · {t("common.pid", { pid: String(s.pid) })}</span>
        {session.started_at && (
          <span className="muted">
            {" "}
            · {t("session.started", { when: formatRelative(session.started_at, now) })}
          </span>
        )}
      </h3>
      {session.running_now.length > 0 ? (
        <div className="running-now">
          <span className="muted">{t("session.runningNow")}</span>
          {session.running_now.map((p) => (
            <code key={p.pid} title={p.command}>
              {p.command}
            </code>
          ))}
        </div>
      ) : (
        <div className="muted small">{t("session.idle")}</div>
      )}
      {session.tree.length > 0 && (
        <details>
          <summary className="muted small">
            {t("session.processTree", { n: session.tree.length })}
          </summary>
          <ul className="proc-tree">
            {session.tree.map((p) => (
              <ProcRow key={p.pid} p={p} />
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

function ProcRow({ p }: { p: ProcessInfo }) {
  return (
    <li style={{ paddingLeft: `${p.depth * 1.1}rem` }}>
      <code title={p.command}>{p.command}</code>
      <span className="muted">
        {" "}
        {formatElapsed(p.elapsed_secs)} · {formatBytes(p.rss_bytes)} ·{" "}
        {t("common.pid", { pid: String(p.pid) })}
      </span>
    </li>
  );
}
