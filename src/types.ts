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

export type RuntimeStatus =
  | { status: "not-detected" }
  | { status: "detected-only"; candidates: DetectedRuntime[] }
  | { status: "connected"; info: RuntimeInfo }
  | { status: "unavailable"; candidates: DetectedRuntime[]; reason: string };

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
