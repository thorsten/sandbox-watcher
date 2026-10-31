//! Resolve a runtime endpoint when detection only found a binary.
//!
//! Docker users often switch contexts (Colima, Rancher Desktop), and Podman
//! machines on macOS and Windows expose a socket at a path only the CLI knows.
//! These helpers ask the CLI once so the Docker-compatible API can be used.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::detect::DetectedRuntime;
use crate::host::validate_alias;
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

/// A Docker context with an `ssh://` endpoint, as offered for import into
/// the remote hosts. Listing contexts never connects to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshContext {
    /// Docker context name.
    pub name: String,
    /// The `ssh://` endpoint as Docker reports it.
    pub endpoint: String,
    /// The alias to add, when the endpoint is a plain `user@host` or `host`.
    pub alias: Option<String>,
    /// Why the context cannot be added as is. Set exactly when `alias` is `None`.
    pub reason: Option<String>,
    /// A `~/.ssh/config` block that gives the context an alias. Set when
    /// `alias` is `None` and the endpoint names a usable host and port.
    pub snippet: Option<String>,
}

#[derive(Deserialize)]
struct ContextLine {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "DockerEndpoint", default)]
    docker_endpoint: String,
}

/// The `ssh://` contexts of the local Docker CLI. Empty when Docker is
/// missing or its output cannot be read.
pub async fn docker_ssh_contexts(runner: &CommandRunner) -> Vec<SshContext> {
    let Some(out) = run(
        runner,
        "docker",
        &["context", "ls", "--format", "{{json .}}"],
    )
    .await
    else {
        return vec![];
    };
    out.lines()
        .filter_map(|line| serde_json::from_str::<ContextLine>(line.trim()).ok())
        .filter(|c| c.docker_endpoint.starts_with("ssh://"))
        .map(|c| ssh_context(&c.name, &c.docker_endpoint))
        .collect()
}

/// Turn one `ssh://[user@]host[:port][/path]` endpoint into an import row.
pub fn ssh_context(name: &str, endpoint: &str) -> SshContext {
    let rejected = |reason: &str, snippet: Option<String>| SshContext {
        name: name.into(),
        endpoint: endpoint.into(),
        alias: None,
        reason: Some(reason.into()),
        snippet,
    };
    let rest = endpoint.strip_prefix("ssh://").unwrap_or(endpoint);
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (user, host_port) = match authority.rsplit_once('@') {
        Some((user, host_port)) => (Some(user).filter(|u| !u.is_empty()), host_port),
        None => (None, authority),
    };
    let (host, port) = if let Some(v6) = host_port.strip_prefix('[') {
        match v6.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':')),
            None => (v6, None),
        }
    } else {
        match host_port.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (host_port, None),
        }
    };
    if host.is_empty() {
        return rejected("has no host", None);
    }
    let port = match port {
        None => None,
        Some(p) => match p.parse::<u16>() {
            Ok(p) if p > 0 => Some(p),
            _ => return rejected("has an invalid port", None),
        },
    };
    let snippet = || Some(ssh_config_snippet(name, user, host, port));
    if let Some(port) = port {
        return rejected(&format!("uses port {port}"), snippet());
    }
    if !path.is_empty() {
        return rejected("has a path", snippet());
    }
    let alias = match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_string(),
    };
    match validate_alias(&alias) {
        Ok(()) => SshContext {
            name: name.into(),
            endpoint: endpoint.into(),
            alias: Some(alias),
            reason: None,
            snippet: None,
        },
        Err(reason) => rejected(&reason, snippet()),
    }
}

/// A `~/.ssh/config` block giving the endpoint an alias named after the context.
fn ssh_config_snippet(name: &str, user: Option<&str>, host: &str, port: Option<u16>) -> String {
    let mut out = format!("Host {}\n  HostName {host}\n", snippet_alias(name));
    if let Some(user) = user {
        out.push_str(&format!("  User {user}\n"));
    }
    if let Some(port) = port {
        out.push_str(&format!("  Port {port}\n"));
    }
    out
}

/// The context name reduced to characters an alias may contain.
fn snippet_alias(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    match mapped.trim_start_matches('-') {
        "" => "docker-context".into(),
        "local" => "docker-local".into(),
        other => other.into(),
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

    fn importable(alias: &str) -> (Option<String>, Option<String>, Option<String>) {
        (Some(alias.into()), None, None)
    }

    fn parts(c: &SshContext) -> (Option<String>, Option<String>, Option<String>) {
        (c.alias.clone(), c.reason.clone(), c.snippet.clone())
    }

    #[test]
    fn plain_ssh_endpoints_become_aliases() {
        assert_eq!(
            parts(&ssh_context("box", "ssh://dev@box")),
            importable("dev@box")
        );
        assert_eq!(parts(&ssh_context("box", "ssh://box")), importable("box"));
        assert_eq!(
            parts(&ssh_context("box", "ssh://dev@box/")),
            importable("dev@box")
        );
        assert_eq!(
            parts(&ssh_context("box", "ssh://dev@box.example.com")),
            importable("dev@box.example.com")
        );
    }

    #[test]
    fn ports_get_a_reason_and_a_snippet() {
        let c = ssh_context("vm", "ssh://me@vm:2222");
        assert_eq!(c.alias, None);
        assert_eq!(c.reason.as_deref(), Some("uses port 2222"));
        assert_eq!(
            c.snippet.as_deref(),
            Some("Host vm\n  HostName vm\n  User me\n  Port 2222\n")
        );
    }

    #[test]
    fn paths_get_a_reason_and_a_snippet_without_user() {
        let c = ssh_context("vm", "ssh://vm/run/docker.sock");
        assert_eq!(c.alias, None);
        assert_eq!(c.reason.as_deref(), Some("has a path"));
        assert_eq!(c.snippet.as_deref(), Some("Host vm\n  HostName vm\n"));
    }

    #[test]
    fn invalid_characters_and_ipv6_get_a_snippet() {
        let c = ssh_context("v6", "ssh://me@[::1]:22");
        assert_eq!(c.alias, None);
        assert_eq!(c.reason.as_deref(), Some("uses port 22"));
        assert_eq!(
            c.snippet.as_deref(),
            Some("Host v6\n  HostName ::1\n  User me\n  Port 22\n")
        );
        let c = ssh_context("odd", "ssh://me+x@box");
        assert_eq!(c.alias, None);
        assert_eq!(
            c.reason.as_deref(),
            Some("host alias may only contain letters, digits, '.', '_', '@' and '-'")
        );
        assert!(c.snippet.is_some());
    }

    #[test]
    fn empty_hosts_are_rejected_without_a_snippet() {
        for endpoint in ["ssh://", "ssh://me@", "ssh://me@:22"] {
            let c = ssh_context("x", endpoint);
            assert_eq!(c.alias, None, "{endpoint}");
            assert_eq!(c.reason.as_deref(), Some("has no host"), "{endpoint}");
            assert_eq!(c.snippet, None, "{endpoint}");
        }
    }

    #[test]
    fn invalid_ports_are_rejected() {
        let c = ssh_context("x", "ssh://me@box:99999");
        assert_eq!(c.reason.as_deref(), Some("has an invalid port"));
        assert_eq!(c.snippet, None);
    }

    #[test]
    fn snippet_names_are_sanitised() {
        let host_line = |name: &str| {
            ssh_context(name, "ssh://box:2222")
                .snippet
                .unwrap()
                .lines()
                .next()
                .unwrap()
                .to_string()
        };
        assert_eq!(host_line("my vm"), "Host my-vm");
        assert_eq!(host_line("-x"), "Host x");
        assert_eq!(host_line("local"), "Host docker-local");
        assert_eq!(host_line("äö"), "Host docker-context");
    }

    fn context_lines(stdout: &str) -> CommandRunner {
        CommandRunner::Fake(Arc::new(FakeRunner::new(vec![(
            "docker context ls --format {{json .}}",
            CommandOutput {
                exit_code: 0,
                stdout: stdout.into(),
                stderr: String::new(),
            },
        )])))
    }

    #[tokio::test]
    async fn lists_only_ssh_contexts() {
        let runner = context_lines(concat!(
            "WARNING: some docker warning\n",
            r#"{"Current":true,"Name":"default","DockerEndpoint":"unix:///var/run/docker.sock"}"#,
            "\n",
            r#"{"Current":false,"Name":"colima","DockerEndpoint":"unix:///Users/me/.colima/default/docker.sock"}"#,
            "\n",
            r#"{"Current":false,"Name":"remote","DockerEndpoint":"tcp://10.0.0.2:2376"}"#,
            "\n",
            r#"{"Current":false,"Name":"devbox","DockerEndpoint":"ssh://dev@devbox"}"#,
            "\n",
            r#"{"Current":false,"Name":"vm","DockerEndpoint":"ssh://me@vm:2222"}"#,
            "\n",
        ));
        let contexts = docker_ssh_contexts(&runner).await;
        let names: Vec<_> = contexts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["devbox", "vm"]);
        assert_eq!(contexts[0].alias.as_deref(), Some("dev@devbox"));
        assert_eq!(contexts[0].endpoint, "ssh://dev@devbox");
        assert_eq!(contexts[1].alias, None);
    }

    #[tokio::test]
    async fn missing_docker_or_garbage_gives_an_empty_list() {
        let missing = CommandRunner::Fake(Arc::new(FakeRunner::new(vec![])));
        assert!(docker_ssh_contexts(&missing).await.is_empty());
        assert!(
            docker_ssh_contexts(&context_lines("not json\n{\n"))
                .await
                .is_empty()
        );
    }

    #[test]
    fn demo_contexts_cover_both_row_kinds() {
        let demo = crate::mock::demo_ssh_contexts();
        assert!(demo.iter().any(|c| c.alias.is_some()));
        assert!(demo.iter().any(|c| c.snippet.is_some()));
    }
}
