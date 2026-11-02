import { useState } from "react";
import { stopAll } from "../api";
import { confirmStopEnabled, describeInterruptionByHost, describeStopReport } from "../actions";
import { t } from "../i18n";
import { disableConfirmStop } from "../settingsActions";
import type { HostStatus, ContainerOverview } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

export function StopAllButton({
  running,
  hosts = [],
  onChanged,
}: {
  running: ContainerOverview[];
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
        {busy ? t("stop.stopping") : t("stop.allCount", { n: running.length })}
      </button>
      {result && <span className="notice">{result}</span>}
      {confirming && (
        <ConfirmDialog
          title={t("stop.allTitle", { n: running.length })}
          lines={describeInterruptionByHost(running, hosts)}
          confirmLabel={t("stop.all")}
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
