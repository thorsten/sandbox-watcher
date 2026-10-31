//! The top-level state the UI renders: which runtimes, which containers, who
//! is inside them.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::adapter::{Capabilities, ContainerFilter, RuntimeAdapter};
use crate::detect::DetectedRuntime;
use crate::host::{Host, container_key};
use crate::model::{Readiness, RuntimeInfo, RuntimeKind, SandboxContainer, Session};
use crate::sandbox::TESTED_SANDBOX_VERSION;
use crate::sessions::probe_container;

/// Where the data on screen comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataSource {
    Mock,
    Live,
}

/// A runtime that was detected but could not be used this round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFailure {
    #[serde(default)]
    pub host: Host,
    pub kind: RuntimeKind,
    pub endpoint: String,
    pub reason: String,
}

/// A runtime that answered, with what it can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectedRuntime {
    #[serde(default)]
    pub host: Host,
    #[serde(flatten)]
    pub info: RuntimeInfo,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
pub enum RuntimeStatus {
    /// No Docker, Podman or Apple `container` found.
    NotDetected,
    /// Runtime binaries or sockets exist, but none could be connected.
    DetectedOnly { candidates: Vec<DetectedRuntime> },
    /// At least one runtime answers. `failures` lists detected runtimes that
    /// did not, so the UI can say why a second runtime is missing.
    Connected {
        runtimes: Vec<ConnectedRuntime>,
        failures: Vec<RuntimeFailure>,
    },
    /// Adapters exist but every runtime refused the connection.
    Unavailable {
        candidates: Vec<DetectedRuntime>,
        reason: String,
    },
}

/// A container plus what the probe found inside it. Serialises flat, so the
/// UI sees one object with the container fields, `runtime`, `readiness` and
/// `sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerOverview {
    #[serde(flatten)]
    pub container: SandboxContainer,
    pub host: Host,
    /// Unique across hosts: the raw id locally, `alias/id` on a remote host.
    pub key: String,
    pub runtime: RuntimeKind,
    pub readiness: Readiness,
    pub sessions: Vec<Session>,
    /// Set only for containers of an unreachable host, shown from cache.
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overview {
    pub source: DataSource,
    pub runtime: RuntimeStatus,
    pub containers: Vec<ContainerOverview>,
    pub tested_sandbox_version: String,
}

impl Overview {
    fn with(
        source: DataSource,
        runtime: RuntimeStatus,
        containers: Vec<ContainerOverview>,
    ) -> Self {
        Self {
            source,
            runtime,
            containers,
            tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
        }
    }

    pub fn not_detected(source: DataSource) -> Self {
        Self::with(source, RuntimeStatus::NotDetected, vec![])
    }

    pub fn detected_only(candidates: Vec<DetectedRuntime>) -> Self {
        Self::with(
            DataSource::Live,
            RuntimeStatus::DetectedOnly { candidates },
            vec![],
        )
    }

    /// Runtime kinds that answered this round.
    pub fn connected_kinds(&self) -> Vec<RuntimeKind> {
        match &self.runtime {
            RuntimeStatus::Connected { runtimes, .. } => {
                runtimes.iter().map(|r| r.info.kind).collect()
            }
            _ => vec![],
        }
    }
}

/// An adapter plus the machine it talks to.
#[derive(Clone)]
pub struct HostedAdapter {
    pub host: Host,
    pub adapter: Arc<dyn RuntimeAdapter>,
}

impl HostedAdapter {
    pub fn local(adapter: Arc<dyn RuntimeAdapter>) -> Self {
        Self {
            host: Host::Local,
            adapter,
        }
    }
}

struct RuntimeLoad {
    runtime: ConnectedRuntime,
    containers: Vec<ContainerOverview>,
}

async fn load_one(hosted: HostedAdapter) -> std::result::Result<RuntimeLoad, RuntimeFailure> {
    let HostedAdapter { host, adapter } = hosted;
    let failure = |endpoint: String, reason: String| RuntimeFailure {
        host: host.clone(),
        kind: adapter.kind(),
        endpoint,
        reason,
    };
    let info = adapter
        .ping()
        .await
        .map_err(|e| failure(String::new(), e.to_string()))?;
    let containers = adapter
        .list_containers(&ContainerFilter::all())
        .await
        .map_err(|e| failure(info.endpoint.clone(), e.to_string()))?;

    let probes = containers.iter().map(|c| {
        let adapter = adapter.clone();
        let running = c.state.is_running();
        let id = c.id.clone();
        async move {
            if !running {
                return Default::default();
            }
            // A failed probe must not hide the container; it just shows as unknown.
            probe_container(&adapter, &id).await.unwrap_or_default()
        }
    });
    let probes = futures::future::join_all(probes).await;
    let kind = adapter.kind();
    let containers = containers
        .into_iter()
        .zip(probes)
        .map(|(container, probe)| ContainerOverview {
            key: container_key(&host, &container.id),
            host: host.clone(),
            container,
            runtime: kind,
            readiness: probe.readiness,
            sessions: probe.sessions,
            last_seen_at: None,
        })
        .collect();
    Ok(RuntimeLoad {
        runtime: ConnectedRuntime {
            host: host.clone(),
            info,
            capabilities: adapter.capabilities(),
        },
        containers,
    })
}

/// Load the overview across several runtimes at once. Running containers
/// sort first, then by host (local first), runtime order, then name. Every running container
/// is probed for readiness and sessions.
pub async fn load_many(
    adapters: &[HostedAdapter],
    source: DataSource,
    candidates: Vec<DetectedRuntime>,
) -> Overview {
    let results = futures::future::join_all(adapters.iter().cloned().map(load_one)).await;
    let mut runtimes = Vec::new();
    let mut failures = Vec::new();
    let mut containers = Vec::new();
    for result in results {
        match result {
            Ok(load) => {
                runtimes.push(load.runtime);
                containers.extend(load.containers);
            }
            Err(failure) => failures.push(failure),
        }
    }
    if runtimes.is_empty() {
        let reason = failures
            .iter()
            .map(|f| {
                let label = if f.host.is_local() {
                    f.kind.to_string()
                } else {
                    format!("{} on {}", f.kind, f.host)
                };
                format!("{label}: {}", f.reason)
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Overview::with(
            source,
            RuntimeStatus::Unavailable { candidates, reason },
            vec![],
        );
    }
    containers.sort_by(|a, b| {
        b.container
            .state
            .is_running()
            .cmp(&a.container.state.is_running())
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.runtime.as_str().cmp(b.runtime.as_str()))
            .then_with(|| a.container.name.cmp(&b.container.name))
    });
    Overview::with(
        source,
        RuntimeStatus::Connected { runtimes, failures },
        containers,
    )
}

/// Single-runtime convenience over [`load_many`].
pub async fn load(
    adapter: Arc<dyn RuntimeAdapter>,
    source: DataSource,
    candidates: Vec<DetectedRuntime>,
) -> Overview {
    load_many(&[HostedAdapter::local(adapter)], source, candidates).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockAdapter;
    use crate::model::AgentKind;

    #[tokio::test]
    async fn running_containers_sort_first_and_are_probed() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        assert!(matches!(overview.runtime, RuntimeStatus::Connected { .. }));
        let names: Vec<_> = overview
            .containers
            .iter()
            .map(|c| c.container.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "sandbox-billing-api-live",
                "sandbox-web-shop",
                "sandbox-web-shop-2",
                "sandbox-billing-api"
            ]
        );
        let shop = &overview.containers[1];
        assert_eq!(shop.readiness, Readiness::Ready);
        let agents: Vec<_> = shop.sessions.iter().map(|s| s.agent).collect();
        assert_eq!(agents, vec![AgentKind::ClaudeCode, AgentKind::Shell]);
        let stopped = overview.containers.last().unwrap();
        assert_eq!(stopped.readiness, Readiness::Unknown);
        assert!(
            stopped.sessions.is_empty(),
            "stopped containers are not probed"
        );
    }

    #[tokio::test]
    async fn merges_several_runtimes() {
        let docker = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let podman =
            HostedAdapter::local(Arc::new(MockAdapter::demo().with_kind(RuntimeKind::Podman)));
        let overview = load_many(&[docker, podman], DataSource::Mock, vec![]).await;
        assert_eq!(
            overview.connected_kinds(),
            vec![RuntimeKind::Docker, RuntimeKind::Podman]
        );
        assert_eq!(overview.containers.len(), 8);
        let running: Vec<_> = overview
            .containers
            .iter()
            .take(6)
            .map(|c| (c.runtime, c.container.name.as_str()))
            .collect();
        assert_eq!(
            running[0],
            (RuntimeKind::Docker, "sandbox-billing-api-live")
        );
        assert_eq!(
            running[3],
            (RuntimeKind::Podman, "sandbox-billing-api-live")
        );
    }

    #[tokio::test]
    async fn serialises_flat_for_the_ui() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        let json = serde_json::to_value(&overview).unwrap();
        assert_eq!(json["runtime"]["status"], "connected");
        assert_eq!(json["runtime"]["runtimes"][0]["kind"], "docker");
        let first = &json["containers"][0];
        assert!(first["name"].is_string(), "container fields are flattened");
        assert_eq!(first["runtime"], "docker");
        assert_eq!(first["readiness"], "ready");
        assert_eq!(first["sessions"][0]["agent"], "claude-code");
    }

    #[tokio::test]
    async fn same_container_id_on_two_hosts_stays_apart() {
        let local = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let remote = HostedAdapter {
            host: Host::Remote("devbox".into()),
            adapter: Arc::new(MockAdapter::demo()),
        };
        let overview = load_many(&[local, remote], DataSource::Mock, vec![]).await;
        assert_eq!(overview.containers.len(), 8);
        let keys: std::collections::HashSet<_> =
            overview.containers.iter().map(|c| c.key.clone()).collect();
        assert_eq!(keys.len(), 8, "keys are unique across hosts");
        let first_remote = overview
            .containers
            .iter()
            .find(|c| !c.host.is_local())
            .unwrap();
        assert_eq!(
            first_remote.key,
            format!("devbox/{}", first_remote.container.id)
        );
        // Local first within running containers.
        assert!(overview.containers[0].host.is_local());
    }

    #[tokio::test]
    async fn serialises_host_and_key() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        let json = serde_json::to_value(&overview).unwrap();
        let first = &json["containers"][0];
        assert_eq!(first["host"], "local");
        assert_eq!(first["key"], first["id"]);
        assert!(first["last_seen_at"].is_null());
        assert_eq!(json["runtime"]["runtimes"][0]["host"], "local");
    }

    #[tokio::test]
    async fn empty_adapter_gives_no_containers() {
        let overview = load(Arc::new(MockAdapter::empty()), DataSource::Mock, vec![]).await;
        assert!(overview.containers.is_empty());
    }
}
