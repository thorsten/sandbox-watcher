//! `RuntimeAdapter` for Apple `container` (macOS 26+, Apple silicon), driven
//! through its CLI. Commands and JSON shape follow sandbox 0.73.1's own
//! adapter (`src/platform/container-runtime/apple/`). The runtime has no
//! event stream and no stats API, which `capabilities()` reports.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use serde_json::Value;

use crate::adapter::{
    Capabilities, ContainerFilter, ExecOutput, ExecRequest, LogOptions, RuntimeAdapter,
};
use crate::model::{
    ContainerDetails, ContainerState, Mount, MountRole, Readiness, ResourceSample, RuntimeEvent,
    RuntimeInfo, RuntimeKind, SandboxContainer,
};
use crate::runner::{CommandOutput, CommandRunner};
use crate::sandbox::{HASH_LABEL, PROJECT_LABEL, READY_FILE};
use crate::{Error, Result};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct AppleContainerAdapter {
    binary: String,
    runner: CommandRunner,
}

impl AppleContainerAdapter {
    pub fn new(binary: impl Into<String>) -> Self {
        Self::with_runner(CommandRunner::Local, binary)
    }

    pub fn with_runner(runner: CommandRunner, binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            runner,
        }
    }

    async fn run(&self, args: &[String], timeout: Duration) -> Result<CommandOutput> {
        self.runner
            .run(&self.binary, args, timeout)
            .await
            .map_err(|e| match e {
                Error::RuntimeUnavailable { reason, .. } => Error::RuntimeUnavailable {
                    runtime: RuntimeKind::AppleContainer.to_string(),
                    reason,
                },
                other => other,
            })
    }

    /// Run and require exit code 0.
    async fn run_ok(&self, args: &[&str]) -> Result<String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let out = self.run(&args, DEFAULT_TIMEOUT).await?;
        if out.exit_code != 0 {
            let detail = format!("{}\n{}", out.stderr, out.stdout);
            if is_missing_resource(&detail) {
                return Err(Error::ContainerNotFound(
                    args.last().cloned().unwrap_or_default(),
                ));
            }
            return Err(Error::Protocol(format!(
                "container {} failed with exit code {}: {}",
                args.join(" "),
                out.exit_code,
                out.stderr.trim()
            )));
        }
        Ok(out.stdout)
    }

    async fn readiness(&self, id: &str) -> Readiness {
        let request = ExecRequest::new(vec!["test".into(), "-f".into(), READY_FILE.into()]);
        match self.exec(id, request).await {
            Ok(o) if o.exit_code == 0 => Readiness::Ready,
            Ok(_) => Readiness::Starting,
            Err(_) => Readiness::Unknown,
        }
    }
}

/// Error text Apple `container` prints for unknown containers.
pub fn is_missing_resource(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("container not found:")
        || lower.contains("image not found:")
        || lower.contains("volume not found:")
        || (lower.contains("container with id") && lower.contains("not found"))
}

fn parse_date(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(Into::into)
}

/// Map one element of `container list --format json` / `container inspect`.
/// `None` when it has no sandbox project label.
pub fn parse_container(value: &Value) -> Option<(SandboxContainer, Vec<Mount>)> {
    let config = &value["configuration"];
    let labels: BTreeMap<String, String> = config["labels"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let project_slug = labels.get(PROJECT_LABEL)?.clone();
    let id = config["id"]
        .as_str()
        .or_else(|| value["id"].as_str())?
        .to_string();
    let started_at = parse_date(&value["status"]["startedDate"]);
    let state = match value["status"]["state"].as_str().unwrap_or("") {
        "stopped" if started_at.is_none() => ContainerState::Created,
        "stopped" => ContainerState::Exited,
        other => ContainerState::parse(other),
    };
    let container = SandboxContainer {
        name: id.clone(),
        id,
        project_slug,
        image: config["image"]["reference"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        state,
        created_at: parse_date(&config["creationDate"]),
        started_at: if state.is_running() { started_at } else { None },
        config_hash: labels.get(HASH_LABEL).cloned(),
        labels,
    };
    let mounts = config["mounts"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let source = m["source"].as_str()?;
                    let destination = m["destination"].as_str()?;
                    let read_only = m["options"]
                        .as_array()
                        .is_some_and(|o| o.iter().any(|v| v.as_str() == Some("ro")));
                    Some(Mount {
                        source: source.to_string(),
                        destination: destination.to_string(),
                        read_only,
                        kind: "bind".into(),
                        role: MountRole::Other,
                        host_path: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some((container, mounts))
}

fn parse_array(output: &str) -> Result<Vec<Value>> {
    let value: Value = serde_json::from_str(output.trim())
        .map_err(|e| Error::Protocol(format!("Apple container returned invalid JSON: {e}")))?;
    match value {
        Value::Array(items) => Ok(items),
        Value::Object(_) => Ok(vec![value]),
        _ => Err(Error::Protocol(
            "Apple container returned unexpected JSON".into(),
        )),
    }
}

#[async_trait]
impl RuntimeAdapter for AppleContainerAdapter {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::AppleContainer
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            exec: true,
            stats: false,
            logs: true,
            events: false,
            stop: true,
        }
    }

    async fn ping(&self) -> Result<RuntimeInfo> {
        let version = self.run_ok(&["--version"]).await?;
        Ok(RuntimeInfo {
            kind: RuntimeKind::AppleContainer,
            version: Some(version.trim().to_string()),
            endpoint: self.binary.clone(),
        })
    }

    async fn list_containers(&self, filter: &ContainerFilter) -> Result<Vec<SandboxContainer>> {
        let output = self.run_ok(&["list", "-a", "--format", "json"]).await?;
        let containers = parse_array(&output)?
            .iter()
            .filter_map(parse_container)
            .map(|(c, _)| c)
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
            .collect();
        Ok(containers)
    }

    async fn inspect(&self, id: &str) -> Result<Option<ContainerDetails>> {
        let output = match self.run_ok(&["inspect", id]).await {
            Ok(output) => output,
            Err(Error::ContainerNotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let items = parse_array(&output)?;
        let Some((container, mounts)) = items.first().and_then(parse_container) else {
            return Ok(None);
        };
        let readiness = if container.state == ContainerState::Running {
            self.readiness(&container.id).await
        } else {
            Readiness::Unknown
        };
        Ok(Some(ContainerDetails {
            container,
            readiness,
            mounts,
            // Not exposed by `container inspect`.
            ports: vec![],
            env_names: vec![],
            user: None,
            working_dir: None,
            exit_code: None,
        }))
    }

    async fn exec(&self, id: &str, request: ExecRequest) -> Result<ExecOutput> {
        let mut args = vec!["exec".to_string()];
        if let Some(user) = &request.user {
            args.push("-u".into());
            args.push(user.clone());
        }
        for (k, v) in &request.env {
            args.push("-e".into());
            args.push(format!("{k}={v}"));
        }
        args.push(id.to_string());
        args.extend(request.command);
        let out = self
            .run(&args, request.timeout.unwrap_or(DEFAULT_TIMEOUT))
            .await?;
        if out.exit_code != 0 && is_missing_resource(&format!("{}\n{}", out.stderr, out.stdout)) {
            return Err(Error::ContainerNotFound(id.into()));
        }
        Ok(ExecOutput {
            exit_code: out.exit_code,
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    async fn stats(&self, _id: &str) -> Result<ResourceSample> {
        Err(Error::Unsupported {
            runtime: RuntimeKind::AppleContainer.to_string(),
            operation: "stats".into(),
        })
    }

    async fn logs(&self, id: &str, options: LogOptions) -> Result<String> {
        let tail = options.tail.unwrap_or(200).to_string();
        self.run_ok(&["logs", "-n", &tail, id]).await
    }

    async fn events(&self) -> Result<BoxStream<'static, Result<RuntimeEvent>>> {
        Err(Error::Unsupported {
            runtime: RuntimeKind::AppleContainer.to_string(),
            operation: "events".into(),
        })
    }

    async fn stop(&self, id: &str, _timeout: Option<Duration>) -> Result<()> {
        match self.run_ok(&["stop", id]).await {
            Ok(_) => Ok(()),
            Err(Error::ContainerNotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::FakeRunner;
    use std::sync::Arc;

    #[tokio::test]
    async fn apple_adapter_runs_through_the_runner() {
        let fake = Arc::new(FakeRunner::new(vec![(
            "container --version",
            CommandOutput {
                exit_code: 0,
                stdout: "container CLI version 0.6.0\n".into(),
                stderr: String::new(),
            },
        )]));
        let adapter =
            AppleContainerAdapter::with_runner(CommandRunner::Fake(fake.clone()), "container");
        let info = adapter.ping().await.unwrap();
        assert_eq!(info.version.as_deref(), Some("container CLI version 0.6.0"));
        assert_eq!(fake.calls(), vec!["container --version"]);
    }

    #[test]
    fn detects_missing_resource_messages() {
        assert!(is_missing_resource("Error: container not found: sandbox-x"));
        assert!(is_missing_resource("container with ID abc not found"));
        assert!(!is_missing_resource("permission denied"));
    }

    #[test]
    fn stopped_without_start_is_created() {
        let v: Value = serde_json::json!({
            "configuration": { "id": "sandbox-z", "image": { "reference": "img" }, "labels": { "sandbox.project": "z" } },
            "status": { "state": "stopped" }
        });
        let (c, _) = parse_container(&v).unwrap();
        assert_eq!(c.state, ContainerState::Created);
    }
}
