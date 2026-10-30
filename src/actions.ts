import type { SandboxContainer } from "./types";

const CONFIRM_KEY = "sandbox-watcher.confirm-stop";

export function confirmStopEnabled(): boolean {
  try {
    return window.localStorage.getItem(CONFIRM_KEY) !== "false";
  } catch {
    return true;
  }
}

export function setConfirmStop(enabled: boolean): void {
  try {
    window.localStorage.setItem(CONFIRM_KEY, enabled ? "true" : "false");
  } catch {
    // Private mode or storage disabled: the default (ask) applies.
  }
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

export function networkLogsCommand(projectDir: string | null): string {
  const quoted = projectDir ? `'${projectDir.replaceAll("'", "'\\''")}'` : null;
  return quoted
    ? `cd ${quoted} && sandbox network logs --status=all`
    : "sandbox network logs --status=all";
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
