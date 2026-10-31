//! Resolve a runtime endpoint when detection only found a binary.
//!
//! Docker users often switch contexts (Colima, Rancher Desktop), and Podman
//! machines on macOS and Windows expose a socket at a path only the CLI knows.
//! These helpers ask the CLI once so the Docker-compatible API can be used.

use std::time::Duration;

use crate::detect::DetectedRuntime;
use crate::model::RuntimeKind;
use crate::runner::CommandRunner;

const CLI_TIMEOUT: Duration = Duration::from_secs(8);

async fn run(runner: &CommandRunner, binary: &str, args: &[&str]) -> Option<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = runner.run(binary, &args, CLI_TIMEOUT).await.ok()?;
    if output.exit_code != 0 {
        return None;
    }
    let text = output.stdout.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Host of the active Docker context, e.g. `unix:///Users/me/.colima/default/docker.sock`.
pub async fn docker_host_from_context(runner: &CommandRunner) -> Option<String> {
    let host = run(
        runner,
        "docker",
        &[
            "context",
            "inspect",
            "--format",
            "{{.Endpoints.docker.Host}}",
        ],
    )
    .await?;
    normalise_host(&host)
}

/// Socket of the Podman service: `CONTAINER_HOST`, then the machine's
/// connection info, then `podman info`.
pub async fn podman_socket(runner: &CommandRunner) -> Option<String> {
    if runner.is_local()
        && let Ok(host) = std::env::var("CONTAINER_HOST")
        && let Some(host) = normalise_host(&host)
    {
        return Some(host);
    }
    if let Some(path) = run(
        runner,
        "podman",
        &[
            "machine",
            "inspect",
            "--format",
            "{{.ConnectionInfo.PodmanSocket.Path}}",
        ],
    )
    .await
        && let Some(host) = normalise_host(&path)
    {
        return Some(host);
    }
    let path = run(
        runner,
        "podman",
        &["info", "--format", "{{.Host.RemoteSocket.Path}}"],
    )
    .await?;
    normalise_host(&path)
}

/// Endpoint of a runtime on a remote host, discovered through its CLI there.
/// Docker falls back to the default socket; TCP endpoints are passed through
/// and rejected by the caller.
pub async fn resolve_remote(runner: &CommandRunner, kind: RuntimeKind) -> Option<String> {
    match kind {
        RuntimeKind::Docker => Some(
            docker_host_from_context(runner)
                .await
                .unwrap_or_else(|| "unix:///var/run/docker.sock".into()),
        ),
        RuntimeKind::Podman => podman_socket(runner).await,
        RuntimeKind::AppleContainer => Some("container".into()),
    }
}

/// Accept `unix://`, `npipe://`, `tcp://` and bare paths; reject SSH and empty values.
pub fn normalise_host(value: &str) -> Option<String> {
    let v = value.trim().trim_matches('"');
    if v.is_empty() || v == "<no value>" || v.starts_with("ssh://") {
        return None;
    }
    Some(v.to_string())
}

/// Endpoint to hand to the adapter for a detected candidate. Binary-only
/// candidates (`docker`, `podman`) are resolved through their CLI.
pub async fn resolve(candidate: &DetectedRuntime) -> Option<String> {
    match (candidate.kind, candidate.endpoint.as_str()) {
        (RuntimeKind::Docker, "docker") => docker_host_from_context(&CommandRunner::Local)
            .await
            .or(Some("docker".into())),
        (RuntimeKind::Podman, "podman") => podman_socket(&CommandRunner::Local).await,
        _ => Some(candidate.endpoint.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{CommandOutput, FakeRunner};
    use std::sync::Arc;

    #[tokio::test]
    async fn docker_context_is_read_through_the_runner() {
        let fake = Arc::new(FakeRunner::new(vec![(
            "docker context inspect --format {{.Endpoints.docker.Host}}",
            CommandOutput {
                exit_code: 0,
                stdout: "unix:///var/run/docker.sock\n".into(),
                stderr: String::new(),
            },
        )]));
        assert_eq!(
            docker_host_from_context(&CommandRunner::Fake(fake)).await,
            Some("unix:///var/run/docker.sock".into())
        );
    }

    #[tokio::test]
    async fn podman_socket_falls_back_to_podman_info() {
        let fake = Arc::new(FakeRunner::new(vec![(
            "podman info --format {{.Host.RemoteSocket.Path}}",
            CommandOutput {
                exit_code: 0,
                stdout: "/run/user/1000/podman/podman.sock\n".into(),
                stderr: String::new(),
            },
        )]));
        assert_eq!(
            podman_socket(&CommandRunner::Fake(fake)).await,
            Some("/run/user/1000/podman/podman.sock".into())
        );
    }

    #[test]
    fn normalises_hosts() {
        assert_eq!(
            normalise_host(" unix:///x.sock "),
            Some("unix:///x.sock".into())
        );
        assert_eq!(
            normalise_host("\"/run/podman/podman.sock\""),
            Some("/run/podman/podman.sock".into())
        );
        assert_eq!(normalise_host("ssh://user@host"), None);
        assert_eq!(normalise_host("<no value>"), None);
        assert_eq!(normalise_host(""), None);
    }
}
