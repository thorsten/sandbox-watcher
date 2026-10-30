//! The top-level state the UI renders: which runtime, which containers.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::adapter::{ContainerFilter, RuntimeAdapter};
use crate::detect::DetectedRuntime;
use crate::model::{RuntimeInfo, SandboxContainer};
use crate::sandbox::TESTED_SANDBOX_VERSION;

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
    /// Runtime binaries or sockets exist, but no adapter is wired yet (M1).
    DetectedOnly { candidates: Vec<DetectedRuntime> },
    /// Adapter connected and answering.
    Connected { info: RuntimeInfo },
    /// Adapter exists but the runtime refused the connection.
    Unavailable {
        candidates: Vec<DetectedRuntime>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overview {
    pub source: DataSource,
    pub runtime: RuntimeStatus,
    pub containers: Vec<SandboxContainer>,
    pub tested_sandbox_version: String,
}

impl Overview {
    pub fn not_detected(source: DataSource) -> Self {
        Self {
            source,
            runtime: RuntimeStatus::NotDetected,
            containers: vec![],
            tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
        }
    }

    pub fn detected_only(candidates: Vec<DetectedRuntime>) -> Self {
        Self {
            source: DataSource::Live,
            runtime: RuntimeStatus::DetectedOnly { candidates },
            containers: vec![],
            tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
        }
    }
}

/// Load the overview through an adapter. Lists running and stopped
/// containers so the UI can show recently exited sandboxes greyed out.
pub async fn load(adapter: Arc<dyn RuntimeAdapter>, source: DataSource) -> Overview {
    let candidates = Vec::new();
    let info = match adapter.ping().await {
        Ok(info) => info,
        Err(e) => {
            return Overview {
                source,
                runtime: RuntimeStatus::Unavailable {
                    candidates,
                    reason: e.to_string(),
                },
                containers: vec![],
                tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
            };
        }
    };
    match adapter.list_containers(&ContainerFilter::all()).await {
        Ok(mut containers) => {
            containers.sort_by(|a, b| {
                b.state
                    .is_running()
                    .cmp(&a.state.is_running())
                    .then_with(|| a.name.cmp(&b.name))
            });
            Overview {
                source,
                runtime: RuntimeStatus::Connected { info },
                containers,
                tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
            }
        }
        Err(e) => Overview {
            source,
            runtime: RuntimeStatus::Unavailable {
                candidates,
                reason: e.to_string(),
            },
            containers: vec![],
            tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockAdapter;

    #[tokio::test]
    async fn running_containers_sort_first() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock).await;
        assert!(matches!(overview.runtime, RuntimeStatus::Connected { .. }));
        let names: Vec<_> = overview
            .containers
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "sandbox-web-shop",
                "sandbox-web-shop-2",
                "sandbox-billing-api"
            ]
        );
    }

    #[tokio::test]
    async fn empty_adapter_gives_no_containers() {
        let overview = load(Arc::new(MockAdapter::empty()), DataSource::Mock).await;
        assert!(overview.containers.is_empty());
        let json = serde_json::to_value(&overview).unwrap();
        assert_eq!(json["runtime"]["status"], "connected");
    }
}
