//! The contract every container runtime implements. See ADR 0002.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::model::{
    ContainerDetails, ContainerState, ResourceSample, RuntimeEvent, RuntimeInfo, RuntimeKind,
    SandboxContainer,
};

/// Which containers to list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerFilter {
    /// Restrict to one project slug. `None` lists every sandbox container.
    pub project_slug: Option<String>,
    /// Restrict to these states. Empty means running containers only unless
    /// `include_stopped` is set.
    pub states: Vec<ContainerState>,
    pub include_stopped: bool,
}

impl ContainerFilter {
    pub fn all() -> Self {
        Self {
            include_stopped: true,
            ..Self::default()
        }
    }

    pub fn running() -> Self {
        Self::default()
    }

    pub fn for_project(slug: impl Into<String>) -> Self {
        Self {
            project_slug: Some(slug.into()),
            ..Self::default()
        }
    }
}

/// A command to run inside a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRequest {
    pub command: Vec<String>,
    /// User to run as. Sandbox's diagnostics need `root`.
    pub user: Option<String>,
    pub env: BTreeMap<String, String>,
    pub timeout: Option<Duration>,
}

impl ExecRequest {
    pub fn new(command: Vec<String>) -> Self {
        Self {
            command,
            user: None,
            env: BTreeMap::new(),
            timeout: Some(Duration::from_secs(10)),
        }
    }

    pub fn as_root(mut self) -> Self {
        self.user = Some("root".into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogOptions {
    pub tail: Option<usize>,
    pub timestamps: bool,
}

/// Operations a runtime may or may not support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub exec: bool,
    pub stats: bool,
    pub logs: bool,
    pub events: bool,
    pub stop: bool,
}

impl Capabilities {
    pub const FULL: Capabilities = Capabilities {
        exec: true,
        stats: true,
        logs: true,
        events: true,
        stop: true,
    };
}

/// Everything the watcher needs from a container runtime.
///
/// Implementations must be cheap to clone behind an `Arc` and safe to call
/// concurrently. They return only containers that carry sandbox's project
/// label; non-sandbox containers never leak into the model.
#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> RuntimeKind;

    fn capabilities(&self) -> Capabilities;

    /// Check that the runtime answers and report its version.
    async fn ping(&self) -> Result<RuntimeInfo>;

    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<SandboxContainer>>;

    /// `Ok(None)` when the container no longer exists.
    async fn inspect(&self, id: &str) -> Result<Option<ContainerDetails>>;

    async fn exec(&self, id: &str, request: ExecRequest) -> Result<ExecOutput>;

    /// One resource sample. Streaming is layered on top in M4.
    async fn stats(&self, id: &str) -> Result<ResourceSample>;

    async fn logs(&self, id: &str, options: LogOptions) -> Result<String>;

    /// Lifecycle events for sandbox containers only.
    async fn events(&self) -> Result<BoxStream<'static, Result<RuntimeEvent>>>;

    async fn stop(&self, id: &str, timeout: Option<Duration>) -> Result<()>;
}
