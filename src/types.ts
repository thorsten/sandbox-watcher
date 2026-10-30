// Mirrors crates/sandbox-core/src/model.rs and overview.rs. Rust is the source of truth.

export type RuntimeKind = "docker" | "podman" | "apple-container";

export type ContainerState =
  | "created"
  | "running"
  | "paused"
  | "stopping"
  | "exited"
  | "dead"
  | "unknown";

export type Readiness = "starting" | "ready" | "unknown";

export type AgentKind = "claude-code" | "codex" | "open-code" | "pi" | "shell" | "other";

export interface Session {
  pid: number;
  command: string;
  agent: AgentKind;
}

export interface SandboxContainer {
  id: string;
  name: string;
  project_slug: string;
  image: string;
  state: ContainerState;
  created_at: string | null;
  started_at: string | null;
  config_hash: string | null;
  labels: Record<string, string>;
  /** Which runtime listed this container. */
  runtime: RuntimeKind;
  /** Only meaningful for running containers. */
  readiness: Readiness;
  sessions: Session[];
}

export type RuntimeEventKind =
  | { kind: "created" | "started" | "stopped" | "died" | "removed" }
  | { kind: "exec-created"; detail: string }
  | { kind: "other"; detail: string };

export type RuntimeEvent = {
  at: string;
  container_id: string;
  container_name: string | null;
} & RuntimeEventKind;

export interface RuntimeInfo {
  kind: RuntimeKind;
  version: string | null;
  endpoint: string;
}

export interface DetectedRuntime {
  kind: RuntimeKind;
  endpoint: string;
  source: string;
}

export interface Capabilities {
  exec: boolean;
  stats: boolean;
  logs: boolean;
  events: boolean;
  stop: boolean;
}

export type ConnectedRuntime = RuntimeInfo & { capabilities: Capabilities };

export interface RuntimeFailure {
  kind: RuntimeKind;
  endpoint: string;
  reason: string;
}

export type RuntimeStatus =
  | { status: "not-detected" }
  | { status: "detected-only"; candidates: DetectedRuntime[] }
  | { status: "connected"; runtimes: ConnectedRuntime[]; failures: RuntimeFailure[] }
  | { status: "unavailable"; candidates: DetectedRuntime[]; reason: string };

export const RUNTIME_LABELS: Record<RuntimeKind, string> = {
  docker: "Docker",
  podman: "Podman",
  "apple-container": "Apple container",
};

export type DataSource = "mock" | "live";

export interface Overview {
  source: DataSource;
  runtime: RuntimeStatus;
  containers: SandboxContainer[];
  tested_sandbox_version: string;
}

export type NetworkStatus = "allowed" | "blocked";
export type NetworkSource = "firewall" | "dns" | "proxy-access";

export interface NetworkEntry {
  container_id: string;
  destination: string;
  dst_ip: string | null;
  /** 0 when only seen in DNS. */
  port: number;
  count: number;
  last_seen: string | null;
  status: NetworkStatus;
}

export interface NetworkReport {
  container_id: string;
  collected_at: string;
  entries: NetworkEntry[];
  unparsed_sources: NetworkSource[];
  failed_sources: NetworkSource[];
}

export interface ResourceSample {
  container_id: string;
  at: string;
  cpu_percent: number;
  memory_used_bytes: number;
  memory_limit_bytes: number | null;
  net_rx_bytes: number;
  net_tx_bytes: number;
  block_read_bytes: number;
  block_write_bytes: number;
}

export type MountRole = "project" | "persisted" | "runtime" | "other";

export interface Mount {
  source: string;
  destination: string;
  read_only: boolean;
  type: string;
  role: MountRole;
  /** Host spelling of `source` for bind mounts, e.g. a Windows path for a WSL mount. */
  host_path: string | null;
}

export interface PortBinding {
  container_port: number;
  host_port: number | null;
  protocol: string;
}

export interface ContainerDetails {
  readiness: Readiness;
  mounts: Mount[];
  ports: PortBinding[];
  env_names: string[];
  user: string | null;
  working_dir: string | null;
  exit_code: number | null;
}

export interface StopResult {
  container_id: string;
  name: string;
  ok: boolean;
  error: string | null;
}

export type Theme = "system" | "light" | "dark";

export type TerminalChoice =
  | { kind: "auto" }
  | { kind: "iterm" }
  | { kind: "terminal-app" }
  | { kind: "custom"; command: string };

export interface NotificationSettings {
  enabled: boolean;
  session_started: boolean;
  session_ended: boolean;
  container_started: boolean;
  container_stopped: boolean;
  container_died: boolean;
  blocked_burst: boolean;
  stale_container: boolean;
}

export interface Settings {
  poll_interval_secs: number;
  runtime_override: RuntimeKind | null;
  terminal: TerminalChoice;
  confirm_stop: boolean;
  notifications: NotificationSettings;
  launch_at_login: boolean;
  close_to_tray: boolean;
  start_minimized: boolean;
  theme: Theme;
  blocked_burst_threshold: number;
  blocked_burst_window_secs: number;
}

export interface ProcessInfo {
  pid: number;
  ppid: number;
  command: string;
  elapsed_secs: number;
  rss_bytes: number;
  depth: number;
}

export type ActivityEventKind =
  | "user-message"
  | "assistant-text"
  | "tool-use"
  | "tool-result"
  | "tool-error";

export interface ActivityEvent {
  at: string | null;
  kind: ActivityEventKind;
  label: string;
  detail: string;
  tool_use_id: string | null;
}

export interface AgentActivity {
  agent: AgentKind;
  source: string;
  updated_at: string | null;
  events: ActivityEvent[];
  in_flight: ActivityEvent | null;
}

export interface WorkingTreeChange {
  status: string;
  path: string;
}

export interface WorkingTree {
  is_repo: boolean;
  branch: string | null;
  changes: WorkingTreeChange[];
  truncated: boolean;
  shortstat: string | null;
}

export interface SessionProcesses {
  session: Session;
  tree: ProcessInfo[];
  running_now: ProcessInfo[];
}

export interface ContainerActivity {
  container_id: string;
  collected_at: string;
  sessions: SessionProcesses[];
  agents: AgentActivity[];
  working_tree: WorkingTree | null;
  project_dir: string | null;
  errors: string[];
}
