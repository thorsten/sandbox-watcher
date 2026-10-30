import type {
  FileTouch,
  NetworkEntry,
  NetworkReport,
  RuntimeStatus,
  SandboxContainer,
  Session,
  Settings,
  Turn,
  TurnReport,
  Usage,
} from "../types";

export const NOW = new Date("2026-10-30T12:00:00Z");

export function session(partial: Partial<Session> = {}): Session {
  return { pid: 4242, command: "claude", agent: "claude-code", ...partial };
}

export function container(
  partial: Partial<SandboxContainer> & { id?: string } = {},
): SandboxContainer {
  const id = partial.id ?? "c1";
  return {
    id,
    name: `sandbox-${partial.project_slug ?? "web-shop"}`,
    project_slug: "web-shop",
    image: "sandbox-project-web-shop:0.73.1",
    state: "running",
    created_at: "2026-10-30T09:00:00Z",
    started_at: "2026-10-30T10:00:00Z",
    config_hash: "a1b2c3d4e5f6",
    labels: {},
    runtime: "docker",
    readiness: "ready",
    sessions: [],
    ...partial,
  };
}

export function entry(partial: Partial<NetworkEntry> = {}): NetworkEntry {
  return {
    container_id: "c1",
    destination: "example.com",
    dst_ip: null,
    port: 443,
    count: 1,
    last_seen: "2026-10-30T11:59:00Z",
    status: "blocked",
    ...partial,
  };
}

export function report(
  entries: NetworkEntry[],
  partial: Partial<NetworkReport> = {},
): NetworkReport {
  return {
    container_id: "c1",
    collected_at: NOW.toISOString(),
    entries,
    unparsed_sources: [],
    failed_sources: [],
    ...partial,
  };
}

export function connected(
  kinds: ("docker" | "podman" | "apple-container")[] = ["docker"],
): RuntimeStatus {
  return {
    status: "connected",
    runtimes: kinds.map((kind) => ({
      kind,
      version: "27.0.1",
      endpoint: `/var/run/${kind}.sock`,
      capabilities: {
        exec: true,
        stats: kind !== "apple-container",
        logs: true,
        events: kind !== "apple-container",
        stop: true,
      },
    })),
    failures: [],
  };
}

export function usage(partial: Partial<Usage> = {}): Usage {
  return {
    agent: "claude-code",
    input_tokens: 1000,
    output_tokens: 500,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    reasoning_tokens: 0,
    turns: 2,
    cost_usd: 0.0105,
    cost_source: "estimated",
    models: ["claude-sonnet-4-5"],
    ...partial,
  };
}

export function settings(partial: Partial<Settings> = {}): Settings {
  return {
    poll_interval_secs: 10,
    runtime_override: null,
    terminal: { kind: "auto" },
    confirm_stop: true,
    notifications: {
      enabled: true,
      session_started: true,
      session_ended: false,
      container_started: true,
      container_stopped: true,
      container_died: true,
      blocked_burst: true,
      stale_container: true,
    },
    launch_at_login: false,
    close_to_tray: true,
    start_minimized: false,
    theme: "system",
    blocked_burst_threshold: 5,
    blocked_burst_window_secs: 60,
    history_retention_days: 30,
    history_enabled: true,
    prices: { "claude-sonnet-4": { input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 } },
    check_for_updates: true,
    editor: { kind: "system" },
    ...partial,
  };
}

export function file(partial: Partial<FileTouch> = {}): FileTouch {
  return {
    path: "/Users/dev/web-shop/src/cart.ts",
    reads: 1,
    edits: 0,
    created: false,
    deleted: false,
    last_at: "2026-10-30T11:58:00Z",
    ...partial,
  };
}

export function turn(partial: Partial<Turn> = {}): Turn {
  return {
    index: 1,
    prompt: "Fix the cart total rounding bug",
    started_at: "2026-10-30T11:50:00Z",
    ended_at: "2026-10-30T11:53:20Z",
    usage: usage(),
    tool_calls: 3,
    tool_errors: 0,
    files: [file({ edits: 1 })],
    ...partial,
  };
}

export function turnReport(partial: Partial<TurnReport> = {}): TurnReport {
  return {
    agent: "claude-code",
    source: "/home/sandbox/.claude/projects/-p/s.jsonl",
    turns: [turn()],
    truncated: false,
    ...partial,
  };
}
