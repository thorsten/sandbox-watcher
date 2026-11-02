import { stopResultLabel } from "./format";
import { hostLabel, hostsInUse } from "./hosts";
import { t } from "./i18n";
import { RUNTIME_LABELS, type HostStatus, type ContainerOverview, type StopReport } from "./types";

let confirmStop = true;

/** Called whenever settings load or change; keeps the stop confirmation in sync. */
export function applyConfirmStop(enabled: boolean): void {
  confirmStop = enabled;
}

export function confirmStopEnabled(): boolean {
  return confirmStop;
}

/** Human description of what a stop interrupts, for the confirmation dialog. */
export function describeInterruption(containers: ContainerOverview[]): string[] {
  const lines: string[] = [];
  for (const c of containers) {
    if (c.sessions.length === 0) {
      lines.push(t("stop.noSessions", { name: c.name }));
      continue;
    }
    const sessions = c.sessions.map((s) => `${s.command} (PID ${s.pid})`).join(", ");
    lines.push(t("stop.interrupts", { name: c.name, n: c.sessions.length, sessions }));
  }
  return lines;
}

const shellQuote = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;

/**
 * [`describeInterruption`] grouped by host (local first, then each alias) with a
 * heading line per host. A single host gets no headings.
 */
export function describeInterruptionByHost(
  containers: ContainerOverview[],
  hosts: HostStatus[],
): string[] {
  const used = hostsInUse(containers);
  if (used.length <= 1) return describeInterruption(containers);
  return used.flatMap((host) => [
    host === "local" ? t("common.thisMachine") : hostLabel(host, hosts),
    ...describeInterruption(containers.filter((c) => c.host === host)),
  ]);
}

/**
 * One line for the "Stop all" notice. Containers that could not be stopped
 * and hosts whose containers could not even be listed are both named, so a
 * partial stop never reads as a success.
 */
export function describeStopReport(report: StopReport, hosts: HostStatus[] = []): string {
  const failed = report.results.filter((r) => !r.ok);
  const parts: string[] = [];
  const stopped = report.results.length - failed.length;
  if (failed.length === 0 && report.failures.length === 0) {
    return t("stop.stopped", { n: stopped });
  }
  if (stopped > 0) parts.push(t("stop.stoppedPart", { n: stopped }));
  if (failed.length > 0) {
    const list = failed
      .map((f) => `${stopResultLabel(f)} (${f.error ?? t("common.unknown")})`)
      .join(", ");
    parts.push(t("stop.failedPart", { n: failed.length, list }));
  }
  for (const f of report.failures) {
    const runtime = RUNTIME_LABELS[f.runtime];
    parts.push(
      f.host === "local"
        ? t("stop.couldNotListLocal", { runtime, error: f.error })
        : t("stop.couldNotListHost", { runtime, host: hostLabel(f.host, hosts), error: f.error }),
    );
  }
  return parts.join("; ");
}

export function networkLogsCommand(projectDir: string | null, host = "local"): string {
  const local = projectDir
    ? `cd ${shellQuote(projectDir)} && sandbox network logs --status=all`
    : "sandbox network logs --status=all";
  return host === "local"
    ? local
    : `ssh ${/^[\w.@-]+$/.test(host) ? host : shellQuote(host)} ${shellQuote(local)}`;
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
