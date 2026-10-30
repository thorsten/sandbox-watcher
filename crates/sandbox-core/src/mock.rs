//! In-memory adapter for tests and for `SANDBOX_WATCHER_MOCK=1` demo mode.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::sync::broadcast;

use crate::adapter::{
    Capabilities, ContainerFilter, ExecOutput, ExecRequest, LogOptions, RuntimeAdapter,
};
use crate::model::{
    ContainerDetails, ContainerState, Mount, Readiness, ResourceSample, RuntimeEvent,
    RuntimeEventKind, RuntimeInfo, RuntimeKind, SandboxContainer,
};
use crate::network::NetworkSource;
use crate::sandbox::{CONTAINER_TOOLS_PATH, HASH_LABEL, PROJECT_LABEL};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct MockContainer {
    pub details: ContainerDetails,
    /// Output returned for any `exec` call.
    pub exec_output: ExecOutput,
    pub logs: String,
    /// Raw log returned for `sandbox-container-tools network diagnostic <source>`.
    pub network_logs: BTreeMap<NetworkSource, String>,
}

#[derive(Default)]
struct State {
    containers: Vec<MockContainer>,
}

/// Adapter backed by a vector of containers. Clone shares the state.
#[derive(Clone)]
pub struct MockAdapter {
    kind: RuntimeKind,
    state: Arc<Mutex<State>>,
    events: broadcast::Sender<RuntimeEvent>,
}

impl MockAdapter {
    pub fn empty() -> Self {
        let (events, _) = broadcast::channel(64);
        Self {
            kind: RuntimeKind::Docker,
            state: Arc::default(),
            events,
        }
    }

    pub fn with_kind(mut self, kind: RuntimeKind) -> Self {
        self.kind = kind;
        self
    }

    /// Two projects, one of them with two containers, as a realistic demo.
    pub fn demo() -> Self {
        let adapter = Self::empty();
        adapter.add(mock_container("web-shop", 1, "running", "a1b2c3d4e5f6"));
        adapter.add(mock_container("web-shop", 2, "running", "0f0f0f0f0f0f"));
        adapter.add(mock_container("billing-api", 1, "exited", "a1b2c3d4e5f6"));
        adapter.add(mock_container(
            "billing-api-live",
            1,
            "running",
            "a1b2c3d4e5f6",
        ));
        adapter
    }

    pub fn add(&self, container: MockContainer) {
        self.state
            .lock()
            .expect("mock state poisoned")
            .containers
            .push(container);
    }

    /// Simulate a lifecycle event for subscribers of `events()`.
    pub fn emit(&self, event: RuntimeEvent) {
        let _ = self.events.send(event);
    }

    fn find(&self, id: &str) -> Option<MockContainer> {
        let state = self.state.lock().expect("mock state poisoned");
        state
            .containers
            .iter()
            .find(|c| c.details.container.id == id || c.details.container.name == id)
            .cloned()
    }
}

/// Build a container the way sandbox would create it.
pub fn mock_container(slug: &str, instance: u32, state: &str, hash: &str) -> MockContainer {
    let name = if instance == 1 {
        format!("sandbox-{slug}")
    } else {
        format!("sandbox-{slug}-{instance}")
    };
    let id = format!("{:0>64}", format!("{slug}{instance}").replace('-', ""));
    let mut labels = BTreeMap::new();
    labels.insert(PROJECT_LABEL.to_string(), slug.to_string());
    labels.insert(HASH_LABEL.to_string(), hash.to_string());
    let container_state = ContainerState::parse(state);
    let started = Utc.with_ymd_and_hms(2026, 10, 30, 8, 0, 0).single();
    MockContainer {
        details: ContainerDetails {
            container: SandboxContainer {
                id,
                name,
                project_slug: slug.to_string(),
                image: format!("sandbox-project-{slug}:latest"),
                state: container_state,
                created_at: started,
                started_at: if container_state.is_running() {
                    started
                } else {
                    None
                },
                config_hash: Some(hash.to_string()),
                labels,
            },
            readiness: if container_state.is_running() {
                Readiness::Ready
            } else {
                Readiness::Unknown
            },
            mounts: vec![Mount {
                source: format!("/Users/dev/projects/{slug}"),
                destination: format!("/home/sandbox/{slug}"),
                read_only: false,
                kind: "bind".into(),
            }],
            ports: vec![],
            env_names: vec!["ANTHROPIC_API_KEY".into(), "TERM".into()],
            user: Some("sandbox".into()),
            exit_code: if container_state.is_running() {
                None
            } else {
                Some(0)
            },
        },
        exec_output: ExecOutput {
            exit_code: 0,
            stdout: "ready\n4242|claude\n4300|zsh -l\n".into(),
            stderr: String::new(),
        },
        logs: "sandbox: container ready\n".into(),
        network_logs: demo_network_logs(),
    }
}

/// Small but realistic logs in sandbox's three formats.
pub fn demo_network_logs() -> BTreeMap<NetworkSource, String> {
    let mut logs = BTreeMap::new();
    logs.insert(
        NetworkSource::Firewall,
        "1761810001.1 IP 172.17.0.2.51234 > 93.184.216.34.443: Flags [S]\n\
         1761810002.2 IP 172.17.0.2.51235 > 93.184.216.34.443: Flags [S]\n"
            .into(),
    );
    logs.insert(
        NetworkSource::Dns,
        "Oct 30 08:00:05 dnsmasq[12]: reply registry.npmjs.org is 104.16.1.35\n\
         Oct 30 08:00:06 dnsmasq[12]: reply example.com is 93.184.216.34\n\
         Oct 30 08:00:07 dnsmasq[12]: config telemetry.example is NXDOMAIN\n"
            .into(),
    );
    logs.insert(
        NetworkSource::ProxyAccess,
        "1761810010.1 45 127.0.0.1 TCP_TUNNEL/200 5000 CONNECT registry.npmjs.org:443 - HIER_DIRECT/104.16.1.35 -\n\
         1761810011.4 0 127.0.0.1 TCP_DENIED/403 3456 CONNECT evil.example:443 - HIER_NONE/- text/html\n\
         1761810012.7 200 127.0.0.1 TCP_TUNNEL/200 900 CONNECT api.anthropic.com:443 - HIER_DIRECT/160.79.104.10 -\n"
            .into(),
    );
    logs
}

#[async_trait]
impl RuntimeAdapter for MockAdapter {
    fn kind(&self) -> RuntimeKind {
        self.kind
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::FULL
    }

    async fn ping(&self) -> Result<RuntimeInfo> {
        Ok(RuntimeInfo {
            kind: self.kind,
            version: Some("mock".into()),
            endpoint: "memory".into(),
        })
    }

    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<SandboxContainer>> {
        let state = self.state.lock().expect("mock state poisoned");
        Ok(state
            .containers
            .iter()
            .map(|c| c.details.container.clone())
            .filter(|c| {
                filter
                    .project_slug
                    .as_deref()
                    .is_none_or(|s| s == c.project_slug)
            })
            .filter(|c| {
                if !filter.states.is_empty() {
                    filter.states.contains(&c.state)
                } else {
                    filter.include_stopped || c.state.is_running()
                }
            })
            .collect())
    }

    async fn inspect(&self, id: &str) -> Result<Option<ContainerDetails>> {
        Ok(self.find(id).map(|c| c.details))
    }

    async fn exec(&self, id: &str, request: ExecRequest) -> Result<ExecOutput> {
        let container = self
            .find(id)
            .ok_or_else(|| Error::ContainerNotFound(id.into()))?;
        // `sandbox-container-tools network diagnostic <source>` serves the stored log.
        if let [tools, network, diagnostic, source] = request.command.as_slice()
            && tools == CONTAINER_TOOLS_PATH
            && network == "network"
            && diagnostic == "diagnostic"
        {
            let source = match source.as_str() {
                "firewall" => NetworkSource::Firewall,
                "dns" => NetworkSource::Dns,
                "proxy-access" => NetworkSource::ProxyAccess,
                _ => {
                    return Ok(ExecOutput {
                        exit_code: 2,
                        stdout: String::new(),
                        stderr: "unknown source".into(),
                    });
                }
            };
            let stdout = container
                .network_logs
                .get(&source)
                .cloned()
                .unwrap_or_default();
            return Ok(ExecOutput {
                exit_code: 0,
                stdout,
                stderr: String::new(),
            });
        }
        // Honour `sh -c "exit N"` so adapter conformance tests can check exit codes.
        if let [sh, flag, script] = request.command.as_slice()
            && sh == "sh"
            && flag == "-c"
            && let Some(code) = script.trim().strip_prefix("exit ")
            && let Ok(code) = code.trim().parse::<i64>()
        {
            return Ok(ExecOutput {
                exit_code: code,
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        Ok(container.exec_output)
    }

    async fn stats(&self, id: &str) -> Result<ResourceSample> {
        let c = self
            .find(id)
            .ok_or_else(|| Error::ContainerNotFound(id.into()))?;
        Ok(ResourceSample {
            container_id: c.details.container.id,
            at: Utc::now(),
            cpu_percent: 12.5,
            memory_used_bytes: 512 * 1024 * 1024,
            memory_limit_bytes: Some(4 * 1024 * 1024 * 1024),
            net_rx_bytes: 1_000_000,
            net_tx_bytes: 250_000,
            block_read_bytes: 0,
            block_write_bytes: 0,
        })
    }

    async fn logs(&self, id: &str, options: LogOptions) -> Result<String> {
        let c = self
            .find(id)
            .ok_or_else(|| Error::ContainerNotFound(id.into()))?;
        Ok(match options.tail {
            Some(n) => {
                let lines: Vec<&str> = c.logs.lines().collect();
                let start = lines.len().saturating_sub(n);
                lines[start..].join("\n")
            }
            None => c.logs,
        })
    }

    async fn events(&self) -> Result<BoxStream<'static, Result<RuntimeEvent>>> {
        let rx = self.events.subscribe();
        let stream = tokio_stream_from_broadcast(rx);
        Ok(stream.boxed())
    }

    async fn stop(&self, id: &str, _timeout: Option<Duration>) -> Result<()> {
        let mut state = self.state.lock().expect("mock state poisoned");
        let Some(c) = state
            .containers
            .iter_mut()
            .find(|c| c.details.container.id == id || c.details.container.name == id)
        else {
            return Err(Error::ContainerNotFound(id.into()));
        };
        c.details.container.state = ContainerState::Exited;
        c.details.container.started_at = None;
        let event = RuntimeEvent {
            at: Utc::now(),
            container_id: c.details.container.id.clone(),
            container_name: Some(c.details.container.name.clone()),
            kind: RuntimeEventKind::Stopped,
        };
        drop(state);
        let _ = self.events.send(event);
        Ok(())
    }
}

fn tokio_stream_from_broadcast(
    rx: broadcast::Receiver<RuntimeEvent>,
) -> impl futures::Stream<Item = Result<RuntimeEvent>> {
    // `unfold` keeps one `recv()` future alive across polls, so the waker
    // registered with the channel survives and the stream is woken on send.
    futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => return Some((Ok(event), rx)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_running_by_default_and_filters_by_project() {
        let adapter = MockAdapter::demo();
        let running = adapter
            .list_containers(&ContainerFilter::running())
            .await
            .unwrap();
        assert_eq!(running.len(), 3);
        let all = adapter
            .list_containers(&ContainerFilter::all())
            .await
            .unwrap();
        assert_eq!(all.len(), 4);
        let billing = adapter
            .list_containers(&ContainerFilter::for_project("billing-api"))
            .await
            .unwrap();
        assert!(billing.is_empty(), "billing-api is exited");
    }

    #[tokio::test]
    async fn stop_marks_exited_and_emits_event() {
        let adapter = MockAdapter::demo();
        let mut events = adapter.events().await.unwrap();
        adapter.stop("sandbox-web-shop", None).await.unwrap();
        let details = adapter.inspect("sandbox-web-shop").await.unwrap().unwrap();
        assert_eq!(details.container.state, ContainerState::Exited);
        let event = events.next().await.unwrap().unwrap();
        assert_eq!(event.kind, RuntimeEventKind::Stopped);
    }

    #[tokio::test]
    async fn unknown_container_errors() {
        let adapter = MockAdapter::empty();
        assert!(matches!(
            adapter.stop("nope", None).await,
            Err(Error::ContainerNotFound(_))
        ));
    }
}
