import { useState } from "react";
import { filterEntries, formatRelative } from "../network";
import type { NetworkReport } from "../types";

const SOURCE_LABELS = {
  firewall: "firewall log",
  dns: "DNS log",
  "proxy-access": "proxy log",
} as const;

export function NetworkPanel({ report, now }: { report: NetworkReport | undefined; now: Date }) {
  const [status, setStatus] = useState<"all" | "blocked">("all");
  const [search, setSearch] = useState("");

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

      <details className="network-caveats">
        <summary className="muted">About these numbers</summary>
        <ul className="muted">
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
