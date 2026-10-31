import { useMemo, useState } from "react";
import { filterEntries, formatRelative } from "../network";
import type { NetworkEntry, NetworkReport } from "../types";
import { AllowDialog } from "./AllowDialog";

const SOURCE_LABELS = {
  firewall: "firewall log",
  dns: "DNS log",
  "proxy-access": "proxy log",
} as const;

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
  const remote = containerKey.includes("/");
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
    return <div className="network muted">Reading network logs…</div>;
  }

  const entries = filterEntries(report.entries, { status, search });
  const blocked = report.entries.filter((e) => e.status === "blocked").length;

  return (
    <div className="network">
      <div className="network-toolbar">
        <div className="segmented" aria-label="Status filter">
          <button
            type="button"
            className={status === "all" ? "active" : ""}
            onClick={() => setStatus("all")}
          >
            All {report.entries.length}
          </button>
          <button
            type="button"
            className={status === "blocked" ? "active" : ""}
            onClick={() => setStatus("blocked")}
          >
            Blocked {blocked}
          </button>
        </div>
        <input
          type="search"
          placeholder="Filter by host, IP or port"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          aria-label="Filter destinations"
        />
        {!remote && selected.size > 0 && (
          <button type="button" className="primary" onClick={() => setAllowOpen(true)}>
            Allow {selected.size} selected…
          </button>
        )}
      </div>

      {report.failed_sources.length > 0 && (
        <p className="network-note warn">
          Could not read {report.failed_sources.map((s) => SOURCE_LABELS[s]).join(", ")}. The
          container may still be starting, or exec as root was denied.
        </p>
      )}
      {report.unparsed_sources.length > 0 && (
        <p className="network-note warn">
          The {report.unparsed_sources.map((s) => SOURCE_LABELS[s]).join(" and ")} had content in a
          format this version does not understand. The sandbox release in this container is probably
          newer than the one tested.
        </p>
      )}

      {entries.length === 0 ? (
        <p className="muted">No matching network activity.</p>
      ) : (
        <table className="network-table">
          <thead>
            <tr>
              <th aria-label="Select" />
              <th>Destination</th>
              <th>Port</th>
              <th>Status</th>
              <th className="num">Count</th>
              <th>Last seen</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={`${e.destination}:${e.port}:${e.status}`}>
                <td>
                  {!remote && e.status === "blocked" && !isIp(e.destination) && (
                    <input
                      type="checkbox"
                      aria-label={`Select ${e.destination}`}
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
                    {e.status}
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
        <summary className="muted">About these numbers</summary>
        <ul className="muted">
          <li>
            Tick blocked hosts and choose "Allow selected" to add them to sandbox's allow list. Raw
            IP addresses cannot be allowed by name.
          </li>
          <li>Repeated blocked connections can be collapsed by the firewall logger.</li>
          <li>IPv6 connections are not recorded.</li>
          <li>
            Hostnames come from the container's DNS replies. Services that share an IP can show a
            different name than the one requested.
          </li>
          <li>DNS-only rows are domains that were resolved but never seen by the proxy.</li>
        </ul>
      </details>
    </div>
  );
}

function isIp(host: string): boolean {
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.includes(":");
}
