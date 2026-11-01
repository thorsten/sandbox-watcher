import { useState } from "react";
import { stopAll } from "../api";
import { confirmStopEnabled, describeInterruptionByHost, describeStopReport } from "../actions";
import { disableConfirmStop } from "../settingsActions";
import type { HostStatus, SandboxContainer } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

export function StopAllButton({
  running,
  hosts = [],
  onChanged,
}: {
  running: SandboxContainer[];
  hosts?: HostStatus[];
  onChanged: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  if (running.length === 0) return null;

  const doStop = async () => {
    setBusy(true);
    try {
      const report = await stopAll();
      setResult(describeStopReport(report, hosts));
      onChanged();
    } catch (e) {
      setResult(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
      setConfirming(false);
      window.setTimeout(() => setResult(null), 6000);
    }
  };

  return (
    <span className="stop-all">
      <button
        type="button"
        className="danger"
        disabled={busy}
        onClick={() => (confirmStopEnabled() ? setConfirming(true) : void doStop())}
      >
        {busy ? "Stopping…" : `Stop all (${running.length})`}
      </button>
      {result && <span className="notice">{result}</span>}
      {confirming && (
        <ConfirmDialog
          title={`Stop all ${running.length} running sandbox containers?`}
          lines={describeInterruptionByHost(running, hosts)}
          confirmLabel="Stop all"
          busy={busy}
          onCancel={() => setConfirming(false)}
          onConfirm={(dontAsk) => {
            if (dontAsk) void disableConfirmStop();
            void doStop();
          }}
        />
      )}
    </span>
  );
}
