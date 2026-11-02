import { useMemo, useState } from "react";
import { t } from "../i18n";
import { filterEntries, formatRelative } from "../network";
import type { NetworkEntry, NetworkReport } from "../types";
import { AllowDialog } from "./AllowDialog";

interface Props {
  containerKey: string;
  report: NetworkReport | undefined;
  now: Date;
  onChanged?: () => void;
}

/** Group blocked entries by host so one selection covers all its ports. */
function selectionsFor(entries: NetworkEntry[], hosts: Set<string>) {
  const byHost = new Map<string, Set<number>>();
  for (const e of entries) {
    if (e.status !== "blocked" || !hosts.has(e.destination)) continue;
    const ports = byHost.get(e.destination) ?? new Set<number>();
    if (e.port > 0) ports.add(e.port);
    byHost.set(e.destination, ports);
  }
  return [...byHost].map(([host, ports]) => ({
    host,
    ports: [...ports].toSorted((a, b) => a - b),
  }));
}

export function NetworkPanel({ containerKey, report, now, onChanged }: Props) {
  const [status, setStatus] = useState<"all" | "blocked">("all");
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [allowOpen, setAllowOpen] = useState(false);
  const selections = useMemo(
    () => selectionsFor(report?.entries ?? [], selected),
    [report, selected],
  );
  const toggle = (host: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(host)) next.delete(host);
      else next.add(host);
      return next;
    });

  if (!report) {
    return <div className="network muted">{t("network.reading")}</div>;
  }

  const entries = filterEntries(report.entries, { status, search });
  const blocked = report.entries.filter((e) => e.status === "blocked").length;

  return (
    <div className="network">
      <div className="network-toolbar">
        <div className="segmented" aria-label={t("network.statusFilter")}>
          <button
            type="button"
            className={status === "all" ? "active" : ""}
            onClick={() => setStatus("all")}
          >
            {t("network.all", { n: report.entries.length })}
          </button>
          <button
            type="button"
            className={status === "blocked" ? "active" : ""}
            onClick={() => setStatus("blocked")}
          >
            {t("network.blocked", { n: blocked })}
          </button>
        </div>
        <input
          type="search"
          placeholder={t("network.searchPlaceholder")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          aria-label={t("network.searchLabel")}
        />
        {selected.size > 0 && (
          <button type="button" className="primary" onClick={() => setAllowOpen(true)}>
            {t("network.allowSelected", { n: selected.size })}
          </button>
        )}
      </div>

      {report.failed_sources.length > 0 && (
        <p className="network-note warn">
          {t("network.couldNotRead", {
            n: report.failed_sources.length,
            sources: report.failed_sources.map((s) => t(`source.${s}`)).join(", "),
          })}
        </p>
      )}
      {report.unparsed_sources.length > 0 && (
        <p className="network-note warn">
          {t("network.unparsed", {
            n: report.unparsed_sources.length,
            sources: report.unparsed_sources.map((s) => t(`source.${s}`)).join(t("common.and")),
          })}
        </p>
      )}

      {entries.length === 0 ? (
        <p className="muted">{t("network.noMatch")}</p>
      ) : (
        <table className="network-table">
          <thead>
            <tr>
              <th aria-label={t("network.select")} />
              <th>{t("network.destination")}</th>
              <th>{t("network.port")}</th>
              <th>{t("network.status")}</th>
              <th className="num">{t("network.count")}</th>
              <th>{t("network.lastSeen")}</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={`${e.destination}:${e.port}:${e.status}`}>
                <td>
                  {e.status === "blocked" && !isIp(e.destination) && (
                    <input
                      type="checkbox"
                      aria-label={t("network.selectHost", { host: e.destination })}
                      checked={selected.has(e.destination)}
                      onChange={() => toggle(e.destination)}
                    />
                  )}
                </td>
                <td>
                  <span title={e.dst_ip ?? undefined}>{e.destination}</span>
                  {e.dst_ip && e.dst_ip !== e.destination && (
                    <span className="muted"> {e.dst_ip}</span>
                  )}
                </td>
                <td>{e.port === 0 ? <span className="muted">DNS</span> : e.port}</td>
                <td>
                  <span className={`badge badge-${e.status === "blocked" ? "err" : "ok"}`}>
                    {t(`netstatus.${e.status}`)}
                  </span>
                </td>
                <td className="num">{e.count}</td>
                <td className="muted">{formatRelative(e.last_seen, now)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {allowOpen && (
        <AllowDialog
          containerKey={containerKey}
          selections={selections}
          onClose={(changed) => {
            setAllowOpen(false);
            if (changed) {
              setSelected(new Set());
              onChanged?.();
            }
          }}
        />
      )}

      <details className="network-caveats">
        <summary className="muted">{t("network.about")}</summary>
        <ul className="muted">
          <li>{t("network.caveatAllow")}</li>
          <li>{t("network.caveatCollapsed")}</li>
          <li>{t("network.caveatIpv6")}</li>
          <li>{t("network.caveatNames")}</li>
          <li>{t("network.caveatDns")}</li>
        </ul>
      </details>
    </div>
  );
}

function isIp(host: string): boolean {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.includes(":");
}
