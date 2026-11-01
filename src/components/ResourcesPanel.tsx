import { getStats } from "../api";
import { formatBytes, formatPercent } from "../sparkline";
import { usePolling } from "../usePolling";
import { Sparkline } from "./Sparkline";

export const POLL_MS = 2_000;

export function ResourcesPanel({ containerKey }: { containerKey: string }) {
  // One stats call covers every container, so switching containers needs no new request.
  const { data: all, error } = usePolling(getStats, POLL_MS);
  const samples = all === null ? null : (all[containerKey] ?? []);

  // A failed refresh keeps the last samples on screen; the note says they may be stale.
  const note = error !== null ? <p className="network-note warn">{error}</p> : null;
  if (samples === null) {
    return note ?? <div className="resources muted">Collecting resource samples…</div>;
  }
  const latest = samples[samples.length - 1];
  if (!latest) {
    return (
      <div className="resources muted">
        {note}
        No samples yet. The first one arrives a second or two after the container is seen.
      </div>
    );
  }

  const cpu = samples.map((s) => s.cpu_percent);
  const mem = samples.map((s) => s.memory_used_bytes);
  const memLimit = latest.memory_limit_bytes ?? undefined;

  return (
    <div className="resources">
      {note}
      <div className="spark-grid">
        <Sparkline
          title="CPU"
          values={cpu}
          max={100}
          format={formatPercent}
          current={formatPercent(latest.cpu_percent)}
        />
        <Sparkline
          title="Memory"
          values={mem}
          max={memLimit}
          format={formatBytes}
          current={
            memLimit
              ? `${formatBytes(latest.memory_used_bytes)} of ${formatBytes(memLimit)}`
              : formatBytes(latest.memory_used_bytes)
          }
        />
      </div>
      <dl className="kv">
        <dt>Network</dt>
        <dd>
          ↓ {formatBytes(latest.net_rx_bytes)} · ↑ {formatBytes(latest.net_tx_bytes)}
        </dd>
        <dt>Disk I/O</dt>
        <dd>
          read {formatBytes(latest.block_read_bytes)} · write{" "}
          {formatBytes(latest.block_write_bytes)}
        </dd>
        <dt>Window</dt>
        <dd className="muted">
          {samples.length} samples, newest {new Date(latest.at).toLocaleTimeString()}
        </dd>
      </dl>
    </div>
  );
}
