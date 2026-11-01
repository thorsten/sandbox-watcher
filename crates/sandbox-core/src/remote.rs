//! Remote hosts: detect runtimes over SSH, build adapters for them, and
//! track each host's connection state.

#[cfg(unix)]
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::endpoints::resolve_remote;
use crate::model::RuntimeKind;
use crate::runner::CommandRunner;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const SANDBOX_ORDER: [(RuntimeKind, &str); 3] = [
    (RuntimeKind::Docker, "docker"),
    (RuntimeKind::Podman, "podman"),
    (RuntimeKind::AppleContainer, "container"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCandidate {
    pub kind: RuntimeKind,
    pub endpoint: String,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteDetection {
    pub candidates: Vec<RemoteCandidate>,
    pub preferred: Option<RuntimeKind>,
    pub probes: Vec<String>,
}

/// Stdout of a remote script. `Err` when it could not run at all (ssh
/// failure or timeout), which says nothing about what is installed.
async fn sh(runner: &CommandRunner, script: &str) -> crate::Result<String> {
    runner
        .run("sh", &["-c".into(), script.into()], PROBE_TIMEOUT)
        .await
        .map(|o| o.stdout)
}

/// The sandbox config file on a host, as sandbox resolves it: an explicit
/// `SANDBOX_CONFIG_DIR`, else `$XDG_CONFIG_HOME` or `~/.config`.
pub const REMOTE_CONFIG_PATH_SH: &str =
    "${SANDBOX_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/sandbox}/config.toml";

pub async fn detect_remote(runner: &CommandRunner) -> RemoteDetection {
    let mut detection = RemoteDetection::default();
    let found = match sh(
        runner,
        "command -v docker; command -v podman; command -v container; true",
    )
    .await
    {
        Ok(found) => found,
        Err(e) => {
            detection.probes.push(format!("probe failed: {e}"));
            return detection;
        }
    };
    let present: Vec<&str> = found
        .lines()
        .filter_map(|l| l.trim().rsplit('/').next())
        .collect();
    let config = sh(
        runner,
        &format!("cat \"{REMOTE_CONFIG_PATH_SH}\" 2>/dev/null; true"),
    )
    .await
    .unwrap_or_else(|e| {
        detection.probes.push(format!("probe failed: {e}"));
        String::new()
    });
    detection.preferred = crate::config::parse_runtime(&config);
    if let Some(kind) = detection.preferred {
        detection
            .probes
            .push(format!("sandbox config: runtime = {kind}"));
    }

    for (kind, binary) in SANDBOX_ORDER {
        if !present.contains(&binary) {
            detection
                .probes
                .push(format!("{binary}: not found on PATH"));
            continue;
        }
        if detection.preferred.is_some_and(|p| p != kind) {
            detection.probes.push(format!(
                "{binary}: found, skipped (sandbox config pins another runtime)"
            ));
            continue;
        }
        match resolve_remote(runner, kind).await {
            Some(endpoint) => {
                detection.probes.push(format!("{binary}: {endpoint}"));
                detection
                    .candidates
                    .push(RemoteCandidate { kind, endpoint });
            }
            None => detection
                .probes
                .push(format!("{binary}: found, but no endpoint")),
        }
    }
    detection
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum HostState {
    Disabled,
    Connecting,
    Connected,
    Unreachable {
        reason: String,
        since: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostStatus {
    pub alias: String,
    pub display_name: String,
    #[serde(flatten)]
    pub state: HostState,
    pub runtimes: Vec<RuntimeKind>,
    pub probes: Vec<String>,
}

/// 5 s doubling to 5 min.
#[derive(Debug, Default)]
pub struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_delay(&mut self) -> Duration {
        let secs = (5u64 << self.attempt.min(6)).min(300);
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_secs(secs)
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

/// Everything a successful connection yields.
#[cfg(unix)]
pub type Connected = (
    Arc<crate::ssh::SshSession>,
    Vec<crate::overview::HostedAdapter>,
    RemoteDetection,
);

/// Open the master connection, detect runtimes and build adapters.
#[cfg(unix)]
pub async fn connect_host(
    alias: &str,
    timeout: Duration,
) -> std::result::Result<Connected, (String, Vec<String>)> {
    use crate::adapter::RuntimeAdapter;
    use crate::apple::AppleContainerAdapter;
    use crate::docker::DockerAdapter;
    use crate::host::Host;
    use crate::overview::HostedAdapter;

    let session = Arc::new(
        crate::ssh::SshSession::open(alias, timeout)
            .await
            .map_err(|e| (e.to_string(), vec![]))?,
    );
    let runner = CommandRunner::Ssh(session.clone());
    let mut detection = detect_remote(&runner).await;
    let host = Host::Remote(alias.to_string());
    let mut adapters = Vec::new();
    for candidate in &detection.candidates {
        let adapter: Option<Arc<dyn RuntimeAdapter>> = match candidate.kind {
            RuntimeKind::Docker | RuntimeKind::Podman => {
                match crate::ssh::ForwardTarget::parse(&candidate.endpoint) {
                    Err(reason) => {
                        detection
                            .probes
                            .push(format!("{}: {reason}", candidate.kind));
                        None
                    }
                    Ok(target) => match session.forward(&target).await {
                        Ok(local) => match DockerAdapter::connect(
                            candidate.kind,
                            &local.display().to_string(),
                        ) {
                            Ok(a) => Some(Arc::new(a) as Arc<dyn RuntimeAdapter>),
                            Err(e) => {
                                detection.probes.push(format!("{}: {e}", candidate.kind));
                                None
                            }
                        },
                        Err(e) => {
                            detection.probes.push(format!("{}: {e}", candidate.kind));
                            None
                        }
                    },
                }
            }
            RuntimeKind::AppleContainer => Some(Arc::new(AppleContainerAdapter::with_runner(
                runner.clone(),
                candidate.endpoint.clone(),
            ))),
        };
        if let Some(adapter) = adapter {
            adapters.push(HostedAdapter {
                host: host.clone(),
                adapter,
            });
        }
    }
    if adapters.is_empty() {
        let probes = detection.probes.clone();
        session.close().await;
        return Err(("no usable container runtime found".into(), probes));
    }
    Ok((session, adapters, detection))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{CommandOutput, FakeRunner};
    use std::sync::Arc;

    fn ok(stdout: &str) -> CommandOutput {
        CommandOutput {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    const WHICH: &str = "sh -c command -v docker; command -v podman; command -v container; true";
    const CONFIG: &str = "sh -c cat \"${SANDBOX_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/sandbox}/config.toml\" 2>/dev/null; true";

    #[tokio::test]
    async fn detects_docker_and_podman_in_sandbox_order() {
        let fake = Arc::new(FakeRunner::new(vec![
            (WHICH, ok("/usr/bin/podman\n/usr/bin/docker\n")),
            (CONFIG, ok("")),
            (
                "docker context inspect --format {{.Endpoints.docker.Host}}",
                ok("unix:///var/run/docker.sock\n"),
            ),
            (
                "podman info --format {{.Host.RemoteSocket.Path}}",
                ok("/run/user/1000/podman/podman.sock\n"),
            ),
        ]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        let kinds: Vec<_> = d.candidates.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, vec![RuntimeKind::Docker, RuntimeKind::Podman]);
        assert_eq!(d.candidates[0].endpoint, "unix:///var/run/docker.sock");
        assert_eq!(
            d.candidates[1].endpoint,
            "/run/user/1000/podman/podman.sock"
        );
        assert_eq!(d.preferred, None);
    }

    #[tokio::test]
    async fn remote_sandbox_config_pins_the_runtime() {
        let fake = Arc::new(FakeRunner::new(vec![
            (WHICH, ok("/usr/bin/docker\n/usr/bin/podman\n")),
            (CONFIG, ok("runtime = \"podman\"\n")),
            (
                "podman info --format {{.Host.RemoteSocket.Path}}",
                ok("/run/podman/podman.sock\n"),
            ),
        ]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert_eq!(d.preferred, Some(RuntimeKind::Podman));
        assert_eq!(d.candidates.len(), 1);
        assert_eq!(d.candidates[0].kind, RuntimeKind::Podman);
    }

    #[tokio::test]
    async fn config_read_honours_sandbox_config_dir() {
        let fake = Arc::new(FakeRunner::new(vec![(WHICH, ok("/usr/bin/docker\n"))]));
        detect_remote(&CommandRunner::Fake(fake.clone())).await;
        let script = fake
            .calls()
            .into_iter()
            .find(|c| c.contains("config.toml"))
            .expect("config was read");
        assert!(
            script.contains(
                "${SANDBOX_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/sandbox}/config.toml"
            ),
            "{script}"
        );
    }

    #[tokio::test]
    async fn docker_without_context_uses_the_default_socket() {
        let fake = Arc::new(FakeRunner::new(vec![
            (WHICH, ok("/usr/bin/docker\n")),
            (CONFIG, ok("")),
        ]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert_eq!(d.candidates[0].endpoint, "unix:///var/run/docker.sock");
    }

    #[tokio::test]
    async fn nothing_found_reports_every_probe() {
        let fake = Arc::new(FakeRunner::new(vec![(WHICH, ok("")), (CONFIG, ok(""))]));
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert!(d.candidates.is_empty());
        assert!(
            d.probes.iter().any(|p| p.contains("docker: not found")),
            "{:?}",
            d.probes
        );
        assert!(d.probes.iter().any(|p| p.contains("podman: not found")));
        assert!(d.probes.iter().any(|p| p.contains("container: not found")));
    }

    #[tokio::test]
    async fn failed_probe_is_not_reported_as_missing_binaries() {
        let fake = Arc::new(
            FakeRunner::new(vec![(CONFIG, ok(""))]).with_error(WHICH, "ssh -O check timed out"),
        );
        let d = detect_remote(&CommandRunner::Fake(fake)).await;
        assert!(d.candidates.is_empty());
        assert!(
            d.probes
                .iter()
                .any(|p| p.starts_with("probe failed: ") && p.ends_with("timed out")),
            "{:?}",
            d.probes
        );
        assert!(
            !d.probes.iter().any(|p| p.contains("not found on PATH")),
            "{:?}",
            d.probes
        );
    }

    #[test]
    fn backoff_doubles_to_five_minutes_and_resets() {
        let mut b = Backoff::new();
        let delays: Vec<u64> = (0..8).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(delays, vec![5, 10, 20, 40, 80, 160, 300, 300]);
        b.reset();
        assert_eq!(b.next_delay().as_secs(), 5);
    }

    #[test]
    fn host_state_serialises_for_the_ui() {
        let s = HostState::Unreachable {
            reason: "x".into(),
            since: chrono::Utc::now(),
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["state"], "unreachable");
        assert_eq!(json["reason"], "x");
    }
}
