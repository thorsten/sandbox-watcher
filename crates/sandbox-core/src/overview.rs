//! The top-level state the UI renders: which runtime, which containers, who
//! is inside them.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::adapter::{ContainerFilter, RuntimeAdapter};
use crate::detect::DetectedRuntime;
use crate::model::{Readiness, RuntimeInfo, SandboxContainer, Session};
use crate::sandbox::TESTED_SANDBOX_VERSION;
use crate::sessions::probe_container;

/// Where the data on screen comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataSource {
    Mock,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
pub enum RuntimeStatus {
    /// No Docker, Podman or Apple `container` found.
    NotDetected,
    /// Runtime binaries or sockets exist, but no adapter is wired yet (M5).
    DetectedOnly { candidates: Vec<DetectedRuntime> },
    /// Adapter connected and answering.
    Connected { info: RuntimeInfo },
    /// Adapter exists but the runtime refused the connection.
    Unavailable {
        candidates: Vec<DetectedRuntime>,
        reason: String,
    },
}

/// A container plus what the probe found inside it. Serialises flat, so the
/// UI sees one object with the container fields, `readiness` and `sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerOverview {
    #[serde(flatten)]
    pub container: SandboxContainer,
    pub readiness: Readiness,
    pub sessions: Vec<Session>,
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
}

/// Load the overview through an adapter. Lists running and stopped
/// containers so the UI can show recently exited sandboxes greyed out, and
/// probes every running one for readiness and sessions.
pub async fn load(
    adapter: Arc<dyn RuntimeAdapter>,
    source: DataSource,
    candidates: Vec<DetectedRuntime>,
) -> Overview {
    let info = match adapter.ping().await {
        Ok(info) => info,
        Err(e) => {
            let status = RuntimeStatus::Unavailable {
                candidates,
                reason: e.to_string(),
            };
            return Overview::with(source, status, vec![]);
        }
    };
    let mut containers = match adapter.list_containers(&ContainerFilter::all()).await {
        Ok(containers) => containers,
        Err(e) => {
            let status = RuntimeStatus::Unavailable {
                candidates,
                reason: e.to_string(),
            };
            return Overview::with(source, status, vec![]);
        }
    };
    containers.sort_by(|a, b| {
        b.state
            .is_running()
            .cmp(&a.state.is_running())
            .then_with(|| a.name.cmp(&b.name))
    });

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

    let containers = containers
        .into_iter()
        .zip(probes)
        .map(|(container, probe)| ContainerOverview {
            container,
            readiness: probe.readiness,
            sessions: probe.sessions,
        })
        .collect();
    Overview::with(source, RuntimeStatus::Connected { info }, containers)
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
    async fn serialises_flat_for_the_ui() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        let json = serde_json::to_value(&overview).unwrap();
        assert_eq!(json["runtime"]["status"], "connected");
        let first = &json["containers"][0];
        assert!(first["name"].is_string(), "container fields are flattened");
        assert_eq!(first["readiness"], "ready");
        assert_eq!(first["sessions"][0]["agent"], "claude-code");
    }

    #[tokio::test]
    async fn empty_adapter_gives_no_containers() {
        let overview = load(Arc::new(MockAdapter::empty()), DataSource::Mock, vec![]).await;
        assert!(overview.containers.is_empty());
    }
}
