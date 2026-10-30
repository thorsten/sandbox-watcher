//! Resolve a runtime endpoint when detection only found a binary.
//!
//! Docker users often switch contexts (Colima, Rancher Desktop), and Podman
//! machines on macOS and Windows expose a socket at a path only the CLI knows.
//! These helpers ask the CLI once so the Docker-compatible API can be used.

use std::process::Stdio;
use std::time::Duration;

use crate::detect::DetectedRuntime;
use crate::model::RuntimeKind;

const CLI_TIMEOUT: Duration = Duration::from_secs(8);

async fn run(binary: &str, args: &[&str]) -> Option<String> {
    let child = tokio::process::Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let output = tokio::time::timeout(CLI_TIMEOUT, child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Host of the active Docker context, e.g. `unix:///Users/me/.colima/default/docker.sock`.
pub async fn docker_host_from_context() -> Option<String> {
    let host = run(
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
pub async fn podman_socket() -> Option<String> {
    if let Ok(host) = std::env::var("CONTAINER_HOST")
        && let Some(host) = normalise_host(&host)
    {
        return Some(host);
    }
    if let Some(path) = run(
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
        "podman",
        &["info", "--format", "{{.Host.RemoteSocket.Path}}"],
    )
    .await?;
    normalise_host(&path)
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
        (RuntimeKind::Docker, "docker") => {
            docker_host_from_context().await.or(Some("docker".into()))
        }
        (RuntimeKind::Podman, "podman") => podman_socket().await,
        _ => Some(candidate.endpoint.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
