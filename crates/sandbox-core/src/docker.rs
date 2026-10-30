//! `RuntimeAdapter` for Docker, and for Podman through its Docker-compatible
//! API, built on `bollard`. Typed responses are converted to JSON once and
//! mapped with `docker_compat`, so the fixture tests cover the real path.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use bollard::container::LogOutput;
use bollard::exec::{StartExecOptions, StartExecResults};
use bollard::models::{ContainerStatsResponse, EventMessage, EventMessageTypeEnum, ExecConfig};
use bollard::query_parameters::{
    EventsOptionsBuilder, InspectContainerOptions, ListContainersOptionsBuilder,
    LogsOptionsBuilder, StatsOptionsBuilder, StopContainerOptionsBuilder,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use chrono::{TimeZone, Utc};
use futures::StreamExt;
use futures::stream::BoxStream;

use crate::adapter::{
    Capabilities, ContainerFilter, ExecOutput, ExecRequest, LogOptions, RuntimeAdapter,
};
use crate::docker_compat::{container_inspect, container_summary};
use crate::model::{
    ContainerDetails, ContainerState, Readiness, ResourceSample, RuntimeEvent, RuntimeEventKind,
    RuntimeInfo, RuntimeKind, SandboxContainer,
};
use crate::sandbox::{PROJECT_LABEL, READY_FILE};
use crate::{Error, Result};

/// Per-request timeout bollard applies to non-streaming calls.
const CLIENT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct DockerAdapter {
    docker: Docker,
    kind: RuntimeKind,
    endpoint: String,
}

impl DockerAdapter {
    /// Connect to the endpoint found by `detect`.
    ///
    /// Accepts `tcp://` and `http://` addresses, `unix://` or plain socket
    /// paths, `npipe://` or `\\.\pipe\...` named pipes (Windows only), and the
    /// bare binary names `docker` or `podman`, which fall back to bollard's
    /// local defaults (`DOCKER_HOST` or the default socket).
    pub fn connect(kind: RuntimeKind, endpoint: &str) -> Result<Self> {
        let docker = connect(endpoint).map_err(|e| Error::RuntimeUnavailable {
            runtime: kind.to_string(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            docker,
            kind,
            endpoint: endpoint.to_string(),
        })
    }

    /// Wrap an already connected client. Used by tests.
    pub fn from_client(docker: Docker, kind: RuntimeKind, endpoint: impl Into<String>) -> Self {
        Self {
            docker,
            kind,
            endpoint: endpoint.into(),
        }
    }

    fn map_err(&self, error: bollard::errors::Error, container: Option<&str>) -> Error {
        use bollard::errors::Error as B;
        match error {
            B::DockerResponseServerError {
                status_code: 404, ..
            } => Error::ContainerNotFound(container.unwrap_or("?").to_string()),
            B::DockerResponseServerError {
                status_code,
                message,
            } => Error::Protocol(format!("{status_code}: {message}")),
            B::IOError { err } if err.kind() == std::io::ErrorKind::PermissionDenied => {
                Error::PermissionDenied {
                    path: self.endpoint.clone(),
                }
            }
            other => Error::RuntimeUnavailable {
                runtime: self.kind.to_string(),
                reason: other.to_string(),
            },
        }
    }

    async fn readiness(&self, id: &str) -> Readiness {
        let request = ExecRequest::new(vec!["test".into(), "-f".into(), READY_FILE.into()]);
        match self.exec(id, request).await {
            Ok(output) if output.exit_code == 0 => Readiness::Ready,
            Ok(_) => Readiness::Starting,
            Err(_) => Readiness::Unknown,
        }
    }
}

fn connect(endpoint: &str) -> std::result::Result<Docker, bollard::errors::Error> {
    let lower = endpoint.to_ascii_lowercase();
    if lower.starts_with("tcp://") || lower.starts_with("http://") {
        return Docker::connect_with_http(endpoint, CLIENT_TIMEOUT_SECS, API_DEFAULT_VERSION);
    }
    if lower.starts_with("unix://") || endpoint.starts_with('/') {
        return Docker::connect_with_unix(endpoint, CLIENT_TIMEOUT_SECS, API_DEFAULT_VERSION);
    }
    if lower.starts_with("npipe://") || endpoint.starts_with(r"\\.\pipe") {
        #[cfg(windows)]
        {
            return Docker::connect_with_named_pipe(
                endpoint,
                CLIENT_TIMEOUT_SECS,
                API_DEFAULT_VERSION,
            );
        }
        #[cfg(not(windows))]
        {
            return Err(bollard::errors::Error::IOError {
                err: std::io::Error::other("named pipes are only supported on Windows"),
            });
        }
    }
    Docker::connect_with_local(
        &std::env::var("DOCKER_HOST").unwrap_or_default(),
        CLIENT_TIMEOUT_SECS,
        API_DEFAULT_VERSION,
    )
    .or_else(|_| Docker::connect_with_local_defaults())
}

fn docker_status(state: ContainerState) -> &'static str {
    match state {
        ContainerState::Created => "created",
        ContainerState::Running => "running",
        ContainerState::Paused => "paused",
        ContainerState::Stopping => "removing",
        ContainerState::Exited => "exited",
        ContainerState::Dead => "dead",
        ContainerState::Unknown => "",
    }
}

/// Map a Docker event onto the domain event. `None` for non-container events.
pub fn map_event(message: &EventMessage) -> Option<RuntimeEvent> {
    if message.typ != Some(EventMessageTypeEnum::CONTAINER) {
        return None;
    }
    let actor = message.actor.as_ref()?;
    let container_id = actor.id.clone()?;
    let action = message.action.as_deref().unwrap_or("");
    let kind = match action.split(':').next().unwrap_or("").trim() {
        "create" => RuntimeEventKind::Created,
        "start" => RuntimeEventKind::Started,
        "stop" => RuntimeEventKind::Stopped,
        "die" => RuntimeEventKind::Died,
        "exec_create" => RuntimeEventKind::ExecCreated,
        "destroy" => RuntimeEventKind::Removed,
        other => RuntimeEventKind::Other(other.to_string()),
    };
    let at = message
        .time_nano
        .and_then(|n| Utc.timestamp_nanos(n).into())
        .or_else(|| message.time.and_then(|t| Utc.timestamp_opt(t, 0).single()))
        .unwrap_or_else(Utc::now);
    Some(RuntimeEvent {
        at,
        container_id,
        container_name: actor
            .attributes
            .as_ref()
            .and_then(|a| a.get("name").cloned()),
        kind,
    })
}

/// Turn one stats response into a sample. Works for cgroup v1 and v2.
pub fn map_stats(id: &str, stats: &ContainerStatsResponse) -> ResourceSample {
    let cpu = stats.cpu_stats.as_ref();
    let pre = stats.precpu_stats.as_ref();
    let total = |c: Option<&bollard::models::ContainerCpuStats>| {
        c.and_then(|c| c.cpu_usage.as_ref())
            .and_then(|u| u.total_usage)
            .unwrap_or(0)
    };
    let system = |c: Option<&bollard::models::ContainerCpuStats>| {
        c.and_then(|c| c.system_cpu_usage).unwrap_or(0)
    };
    let cpu_delta = total(cpu).saturating_sub(total(pre)) as f64;
    let system_delta = system(cpu).saturating_sub(system(pre)) as f64;
    let online = cpu
        .and_then(|c| c.online_cpus)
        .filter(|n| *n > 0)
        .or_else(|| {
            cpu.and_then(|c| c.cpu_usage.as_ref())
                .and_then(|u| u.percpu_usage.as_ref())
                .map(|v| v.len() as u32)
        })
        .unwrap_or(1) as f64;
    let cpu_percent = if system_delta > 0.0 && cpu_delta >= 0.0 {
        cpu_delta / system_delta * online * 100.0
    } else {
        0.0
    };

    let mem = stats.memory_stats.as_ref();
    let usage = mem.and_then(|m| m.usage).unwrap_or(0);
    let cache = mem
        .and_then(|m| m.stats.as_ref())
        .and_then(|s| s.get("inactive_file").or_else(|| s.get("cache")).copied())
        .unwrap_or(0);
    let memory_used_bytes = usage.saturating_sub(cache);
    let memory_limit_bytes = mem.and_then(|m| m.limit).filter(|l| *l < u64::MAX / 2);

    let (net_rx_bytes, net_tx_bytes) = stats
        .networks
        .as_ref()
        .map(|nets| {
            nets.values().fold((0u64, 0u64), |(rx, tx), n| {
                (rx + n.rx_bytes.unwrap_or(0), tx + n.tx_bytes.unwrap_or(0))
            })
        })
        .unwrap_or((0, 0));

    let (block_read_bytes, block_write_bytes) = stats
        .blkio_stats
        .as_ref()
        .and_then(|b| b.io_service_bytes_recursive.as_ref())
        .map(|entries| {
            entries.iter().fold((0u64, 0u64), |(r, w), e| {
                match e.op.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    Some("read") => (r + e.value.unwrap_or(0), w),
                    Some("write") => (r, w + e.value.unwrap_or(0)),
                    _ => (r, w),
                }
            })
        })
        .unwrap_or((0, 0));

    ResourceSample {
        container_id: id.to_string(),
        at: stats.read.unwrap_or_else(Utc::now),
        cpu_percent,
        memory_used_bytes,
        memory_limit_bytes,
        net_rx_bytes,
        net_tx_bytes,
        block_read_bytes,
        block_write_bytes,
    }
}

#[async_trait]
impl RuntimeAdapter for DockerAdapter {
    fn kind(&self) -> RuntimeKind {
        self.kind
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::FULL
    }

    async fn ping(&self) -> Result<RuntimeInfo> {
        let version = self
            .docker
            .version()
            .await
            .map_err(|e| self.map_err(e, None))?;
        Ok(RuntimeInfo {
            kind: self.kind,
            version: version.version,
            endpoint: self.endpoint.clone(),
        })
    }

    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<SandboxContainer>> {
        let mut filters: HashMap<String, Vec<String>> = HashMap::new();
        let label = match &filter.project_slug {
            Some(slug) => format!("{PROJECT_LABEL}={slug}"),
            None => PROJECT_LABEL.to_string(),
        };
        filters.insert("label".into(), vec![label]);
        let statuses: Vec<String> = filter
            .states
            .iter()
            .map(|s| docker_status(*s))
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let all = filter.include_stopped || !statuses.is_empty();
        if !statuses.is_empty() {
            filters.insert("status".into(), statuses);
        }
        let options = ListContainersOptionsBuilder::new()
            .all(all)
            .filters(&filters)
            .build();
        let summaries = self
            .docker
            .list_containers(Some(options))
            .await
            .map_err(|e| self.map_err(e, None))?;
        let mut containers = Vec::with_capacity(summaries.len());
        for summary in &summaries {
            let value = serde_json::to_value(summary)?;
            if let Some(container) = container_summary(&value)? {
                containers.push(container);
            }
        }
        Ok(containers)
    }

    async fn inspect(&self, id: &str) -> Result<Option<ContainerDetails>> {
        let response = match self
            .docker
            .inspect_container(id, None::<InspectContainerOptions>)
            .await
        {
            Ok(response) => response,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {
                return Ok(None);
            }
            Err(e) => return Err(self.map_err(e, Some(id))),
        };
        let value = serde_json::to_value(&response)?;
        let Some(mut details) = container_inspect(&value)? else {
            return Ok(None);
        };
        if details.container.state == ContainerState::Running {
            details.readiness = self.readiness(&details.container.id).await;
        }
        Ok(Some(details))
    }

    async fn exec(&self, id: &str, request: ExecRequest) -> Result<ExecOutput> {
        let timeout = request.timeout.unwrap_or(DEFAULT_EXEC_TIMEOUT);
        let env: Vec<String> = request
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let config = ExecConfig {
            cmd: Some(request.command),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            user: request.user,
            env: (!env.is_empty()).then_some(env),
            ..Default::default()
        };
        let run = async {
            let created = self
                .docker
                .create_exec(id, config)
                .await
                .map_err(|e| self.map_err(e, Some(id)))?;
            let mut stdout = String::new();
            let mut stderr = String::new();
            match self
                .docker
                .start_exec(&created.id, None::<StartExecOptions>)
                .await
                .map_err(|e| self.map_err(e, Some(id)))?
            {
                StartExecResults::Attached { mut output, .. } => {
                    while let Some(chunk) = output.next().await {
                        match chunk.map_err(|e| self.map_err(e, Some(id)))? {
                            LogOutput::StdOut { message } | LogOutput::Console { message } => {
                                stdout.push_str(&String::from_utf8_lossy(&message));
                            }
                            LogOutput::StdErr { message } => {
                                stderr.push_str(&String::from_utf8_lossy(&message));
                            }
                            LogOutput::StdIn { .. } => {}
                        }
                    }
                }
                StartExecResults::Detached => {}
            }
            let inspect = self
                .docker
                .inspect_exec(&created.id)
                .await
                .map_err(|e| self.map_err(e, Some(id)))?;
            Ok(ExecOutput {
                exit_code: inspect.exit_code.unwrap_or(-1),
                stdout,
                stderr,
            })
        };
        tokio::time::timeout(timeout, run)
            .await
            .unwrap_or_else(|_| {
                Err(Error::Protocol(format!(
                    "exec in {id} timed out after {}s",
                    timeout.as_secs()
                )))
            })
    }

    async fn stats(&self, id: &str) -> Result<ResourceSample> {
        let options = StatsOptionsBuilder::new().stream(false).build();
        let mut stream = self.docker.stats(id, Some(options));
        match stream.next().await {
            Some(Ok(stats)) => Ok(map_stats(id, &stats)),
            Some(Err(e)) => Err(self.map_err(e, Some(id))),
            None => Err(Error::Protocol(format!("no stats returned for {id}"))),
        }
    }

    async fn logs(&self, id: &str, options: LogOptions) -> Result<String> {
        let mut builder = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .timestamps(options.timestamps);
        let tail = options.tail.map(|n| n.to_string());
        if let Some(tail) = &tail {
            builder = builder.tail(tail);
        }
        let mut stream = self.docker.logs(id, Some(builder.build()));
        let mut out = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| self.map_err(e, Some(id)))?;
            out.push_str(&String::from_utf8_lossy(&chunk.into_bytes()));
        }
        Ok(out)
    }

    async fn events(&self) -> Result<BoxStream<'static, Result<RuntimeEvent>>> {
        let mut filters: HashMap<String, Vec<String>> = HashMap::new();
        filters.insert("type".into(), vec!["container".into()]);
        filters.insert("label".into(), vec![PROJECT_LABEL.into()]);
        let options = EventsOptionsBuilder::new().filters(&filters).build();
        let adapter = self.clone();
        let stream = self.docker.events(Some(options)).filter_map(move |item| {
            let adapter = adapter.clone();
            async move {
                match item {
                    Ok(message) => map_event(&message).map(Ok),
                    Err(e) => Some(Err(adapter.map_err(e, None))),
                }
            }
        });
        Ok(stream.boxed())
    }

    async fn stop(&self, id: &str, timeout: Option<Duration>) -> Result<()> {
        let secs = timeout.map(|t| t.as_secs() as i32).unwrap_or(10);
        let options = StopContainerOptionsBuilder::new().t(secs).build();
        match self.docker.stop_container(id, Some(options)).await {
            Ok(()) => Ok(()),
            // 304: already stopped.
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 304, ..
            }) => Ok(()),
            Err(e) => Err(self.map_err(e, Some(id))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::EventActor;

    fn event(typ: EventMessageTypeEnum, action: &str) -> EventMessage {
        EventMessage {
            typ: Some(typ),
            action: Some(action.into()),
            actor: Some(EventActor {
                id: Some("abc123".into()),
                attributes: Some(HashMap::from([(
                    "name".to_string(),
                    "sandbox-x".to_string(),
                )])),
            }),
            scope: None,
            time: Some(1_761_810_000),
            time_nano: None,
        }
    }

    #[test]
    fn maps_container_events_and_ignores_others() {
        let e = map_event(&event(EventMessageTypeEnum::CONTAINER, "die")).unwrap();
        assert_eq!(e.kind, RuntimeEventKind::Died);
        assert_eq!(e.container_id, "abc123");
        assert_eq!(e.container_name.as_deref(), Some("sandbox-x"));
        assert_eq!(e.at.timestamp(), 1_761_810_000);

        let e = map_event(&event(
            EventMessageTypeEnum::CONTAINER,
            "exec_create: sh -c ls",
        ))
        .unwrap();
        assert_eq!(e.kind, RuntimeEventKind::ExecCreated);

        let e = map_event(&event(EventMessageTypeEnum::CONTAINER, "rename")).unwrap();
        assert_eq!(e.kind, RuntimeEventKind::Other("rename".into()));

        assert!(map_event(&event(EventMessageTypeEnum::IMAGE, "pull")).is_none());
    }

    #[test]
    fn maps_stats_with_cgroup_v2_fields() {
        let json = serde_json::json!({
            "read": "2026-10-30T10:00:00Z",
            "cpu_stats": { "cpu_usage": { "total_usage": 2_000_000_000u64 }, "system_cpu_usage": 20_000_000_000u64, "online_cpus": 4 },
            "precpu_stats": { "cpu_usage": { "total_usage": 1_000_000_000u64 }, "system_cpu_usage": 10_000_000_000u64, "online_cpus": 4 },
            "memory_stats": { "usage": 600_000_000u64, "limit": 4_000_000_000u64, "stats": { "inactive_file": 100_000_000u64 } },
            "networks": { "eth0": { "rx_bytes": 10, "tx_bytes": 5 }, "eth1": { "rx_bytes": 1, "tx_bytes": 1 } },
            "blkio_stats": { "io_service_bytes_recursive": [ { "op": "Read", "value": 7 }, { "op": "write", "value": 3 } ] }
        });
        let stats: ContainerStatsResponse = serde_json::from_value(json).unwrap();
        let sample = map_stats("abc", &stats);
        assert!(
            (sample.cpu_percent - 40.0).abs() < 1e-9,
            "{}",
            sample.cpu_percent
        );
        assert_eq!(sample.memory_used_bytes, 500_000_000);
        assert_eq!(sample.memory_limit_bytes, Some(4_000_000_000));
        assert_eq!((sample.net_rx_bytes, sample.net_tx_bytes), (11, 6));
        assert_eq!((sample.block_read_bytes, sample.block_write_bytes), (7, 3));
    }

    #[test]
    fn docker_status_names_follow_engine_api() {
        assert_eq!(docker_status(ContainerState::Stopping), "removing");
        assert_eq!(docker_status(ContainerState::Unknown), "");
    }
}
