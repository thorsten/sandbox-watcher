import { hostLabel, hostsInUse } from "./hosts";
import type { HostStatus, SandboxContainer } from "./types";

let confirmStop = true;

/** Called whenever settings load or change; keeps the stop confirmation in sync. */
export function applyConfirmStop(enabled: boolean): void {
  confirmStop = enabled;
}

export function confirmStopEnabled(): boolean {
  return confirmStop;
}

/** Human description of what a stop interrupts, for the confirmation dialog. */
export function describeInterruption(containers: SandboxContainer[]): string[] {
  const lines: string[] = [];
  for (const c of containers) {
    if (c.sessions.length === 0) {
      lines.push(`${c.name}: no active sessions`);
      continue;
    }
    const sessions = c.sessions.map((s) => `${s.command} (PID ${s.pid})`).join(", ");
    lines.push(
      `${c.name}: ${c.sessions.length} session${c.sessions.length === 1 ? "" : "s"} will be interrupted: ${sessions}`,
    );
  }
  return lines;
}

const shellQuote = (s: string) => `'${s.replaceAll("'", "'\\''")}'`;

/**
 * [`describeInterruption`] grouped by host (local first, then each alias) with a
 * heading line per host. A single host gets no headings.
 */
export function describeInterruptionByHost(
  containers: SandboxContainer[],
  hosts: HostStatus[],
): string[] {
  const used = hostsInUse(containers);
  if (used.length <= 1) return describeInterruption(containers);
  return used.flatMap((host) => [
    host === "local" ? "This machine" : hostLabel(host, hosts),
    ...describeInterruption(containers.filter((c) => c.host === host)),
  ]);
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
