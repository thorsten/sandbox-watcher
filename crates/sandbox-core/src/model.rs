//! Domain model shared by all runtimes and by the UI.
//!
//! `src/types.ts` mirrors these types by hand. Keep both in sync.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Container runtime a sandbox container is managed by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    Docker,
    Podman,
    AppleContainer,
}

impl RuntimeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::Docker => "docker",
            RuntimeKind::Podman => "podman",
            RuntimeKind::AppleContainer => "apple-container",
        }
    }
}

impl std::fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lifecycle state of a container, normalised across runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Stopping,
    Exited,
    Dead,
    Unknown,
}

impl ContainerState {
    /// Parse a runtime status string such as Docker's `running` or `exited`.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "created" => ContainerState::Created,
            "running" | "restarting" => ContainerState::Running,
            "paused" => ContainerState::Paused,
            "stopping" | "removing" => ContainerState::Stopping,
            "exited" | "stopped" => ContainerState::Exited,
            "dead" => ContainerState::Dead,
            _ => ContainerState::Unknown,
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, ContainerState::Running | ContainerState::Paused)
    }
}

/// Whether the sandbox entrypoint has finished preparing the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Readiness {
    Starting,
    Ready,
    #[default]
    Unknown,
}

/// A container created by sandbox, identified by the `sandbox.project` label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxContainer {
    pub id: String,
    pub name: String,
    pub project_slug: String,
    pub image: String,
    pub state: ContainerState,
    pub created_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    /// Value of the `sandbox.hash` label: 12 hex characters identifying the
    /// configuration the container was created from.
    pub config_hash: Option<String>,
    pub labels: BTreeMap<String, String>,
}

/// What a mount is for, derived from sandbox's conventions. See `mounts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MountRole {
    /// The project directory the agent works in.
    Project,
    /// Data sandbox keeps between runs (agent state, dependency caches).
    Persisted,
    /// The read-only sandbox runtime files.
    Runtime,
    #[default]
    Other,
}

/// Mount as reported by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mount {
    pub source: String,
    pub destination: String,
    pub read_only: bool,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub role: MountRole,
    /// `source` as the host spells it, e.g. `C:\\Users\\...` for a WSL path
    /// on Windows. `None` for volumes.
    #[serde(default)]
    pub host_path: Option<String>,
}

/// Published or exposed port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: u16,
    pub host_port: Option<u16>,
    pub protocol: String,
}

/// Detailed view of one container, from `inspect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerDetails {
    pub container: SandboxContainer,
    pub readiness: Readiness,
    pub mounts: Vec<Mount>,
    pub ports: Vec<PortBinding>,
    /// Names only. Values are never read, they may contain secrets.
    pub env_names: Vec<String>,
    pub user: Option<String>,
    pub working_dir: Option<String>,
    pub exit_code: Option<i64>,
}

/// The agent or program a session runs. Classification arrives in M2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    OpenCode,
    Pi,
    Shell,
    Other,
}

/// One active sandbox session inside a container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub pid: u32,
    pub command: String,
    pub agent: AgentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkStatus {
    Allowed,
    Blocked,
}

/// Aggregated network destination seen by sandbox's proxy or firewall.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub container_id: String,
    pub destination: String,
    pub dst_ip: Option<String>,
    pub port: u16,
    pub count: u64,
    pub last_seen: Option<DateTime<Utc>>,
    pub status: NetworkStatus,
}

/// One point-in-time resource reading for a container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceSample {
    pub container_id: String,
    pub at: DateTime<Utc>,
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_limit_bytes: Option<u64>,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "detail")]
pub enum RuntimeEventKind {
    Created,
    Started,
    Stopped,
    Died,
    /// Carries the exec command as reported by the runtime, if any.
    ExecCreated(String),
    Removed,
    Other(String),
}

/// Lifecycle event emitted by the runtime for a sandbox container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub at: DateTime<Utc>,
    pub container_id: String,
    pub container_name: Option<String>,
    #[serde(flatten)]
    pub kind: RuntimeEventKind,
}

/// Information about the connected runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub kind: RuntimeKind,
    pub version: Option<String>,
    /// Socket path, named pipe or CLI binary used to reach the runtime.
    pub endpoint: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_states() {
        assert_eq!(ContainerState::parse("running"), ContainerState::Running);
        assert_eq!(ContainerState::parse("Exited"), ContainerState::Exited);
        assert_eq!(ContainerState::parse("removing"), ContainerState::Stopping);
        assert_eq!(ContainerState::parse("weird"), ContainerState::Unknown);
    }

    #[test]
    fn serialises_with_kebab_case_tags() {
        let json = serde_json::to_string(&RuntimeKind::AppleContainer).unwrap();
        assert_eq!(json, "\"apple-container\"");
        let event = RuntimeEvent {
            at: DateTime::parse_from_rfc3339("2026-10-30T10:00:00Z")
                .unwrap()
                .into(),
            container_id: "abc".into(),
            container_name: None,
            kind: RuntimeEventKind::Other("rename".into()),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "other");
        assert_eq!(json["detail"], "rename");
    }
}
