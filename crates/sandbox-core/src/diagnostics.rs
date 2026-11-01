//! Types for the diagnostics page: what was tried and how it turned out.

use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::detect::{DetectedRuntime, Environment};
use crate::model::RuntimeKind;
use crate::remote::{HostState, HostStatus};
use crate::settings::RemoteHost;

/// How one detection candidate turned out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum ProbeOutcome {
    Found { endpoint: String },
    Missing,
    Skipped { reason: String },
    Failed { reason: String },
}

/// One candidate that detection tried, with its outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    pub kind: RuntimeKind,
    /// The socket path, variable name or `<binary> on PATH`, as shown to the user.
    pub candidate: String,
    #[serde(flatten)]
    pub outcome: ProbeOutcome,
}

/// Whether one runtime CLI is on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryCheck {
    pub name: String,
    pub found: bool,
}

/// The inputs detection works from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentReport {
    pub os: String,
    pub docker_host: Option<String>,
    pub container_host: Option<String>,
    pub runtime_override: Option<String>,
    pub config_runtime: Option<RuntimeKind>,
    pub config_path: String,
    pub binaries: Vec<BinaryCheck>,
}

/// One remote host with the result of a fresh connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostReport {
    pub alias: String,
    pub status: HostStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsReport {
    pub generated_at: DateTime<Utc>,
    pub app_version: String,
    pub environment: EnvironmentReport,
    pub local: Vec<Probe>,
    pub hosts: Vec<HostReport>,
}

/// Probe the local environment and test every enabled remote host.
/// `preferred` is the runtime pinned by settings, config or environment,
/// with a label for where the pin came from.
/// Disabled hosts are listed without opening a connection.
pub async fn run(
    app_version: &str,
    preferred: Option<(RuntimeKind, String)>,
    hosts: &[RemoteHost],
) -> DiagnosticsReport {
    // The environment holds non-Sync closures, so it must drop before any await.
    let (found, mut local, environment) = {
        let env = Environment::current();
        let (found, local) = crate::detect::detect_traced(&env);
        let environment = EnvironmentReport {
            os: env.os.to_string(),
            docker_host: env.docker_host.clone(),
            container_host: env.container_host.clone(),
            runtime_override: std::env::var("SANDBOX_WATCHER_RUNTIME").ok(),
            config_runtime: crate::config::load().runtime,
            config_path: crate::config::current_config_path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            binaries: ["docker", "podman", "container"]
                .iter()
                .map(|b| BinaryCheck {
                    name: b.to_string(),
                    found: (env.binary_on_path)(b),
                })
                .collect(),
        };
        (found, local, environment)
    };
    let (candidates, skipped) = plan_candidates(&found, preferred.as_ref());
    local.extend(skipped);
    for candidate in &candidates {
        let (adapter, steps) = crate::live::connect_traced(candidate).await;
        local.extend(steps);
        if let Some(adapter) = adapter {
            // Connecting is lazy for most endpoints, so only a ping tells.
            let outcome = match adapter.ping().await {
                Ok(info) => ProbeOutcome::Found {
                    endpoint: format!("{} {}", info.endpoint, info.version.unwrap_or_default())
                        .trim()
                        .to_string(),
                },
                Err(e) => ProbeOutcome::Failed {
                    reason: e.to_string(),
                },
            };
            local.push(Probe {
                kind: candidate.kind,
                candidate: "ping".into(),
                outcome,
            });
        }
    }
    let tests = hosts
        .iter()
        .filter(|h| h.enabled)
        .map(|h| crate::remote_hosts::test_remote_host(&h.alias));
    let mut tested = futures::future::join_all(tests).await.into_iter();
    let mut reports = Vec::new();
    for h in hosts {
        let display_name = h
            .display_name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| h.alias.clone());
        let status = if h.enabled {
            let mut s = tested.next().expect("one status per enabled host");
            s.display_name = display_name;
            s
        } else {
            HostStatus {
                alias: h.alias.clone(),
                display_name,
                state: HostState::Disabled,
                runtimes: vec![],
                probes: vec![],
            }
        };
        reports.push(HostReport {
            alias: h.alias.clone(),
            status,
        });
    }
    DiagnosticsReport {
        generated_at: Utc::now(),
        app_version: app_version.into(),
        environment,
        local,
        hosts: reports,
    }
}

/// The candidates to connect, following `live::select_candidates`: a pin
/// applies only when its runtime was detected. The probes say what was
/// skipped, or that the pin could not apply.
fn plan_candidates(
    found: &[DetectedRuntime],
    pin: Option<&(RuntimeKind, String)>,
) -> (Vec<DetectedRuntime>, Vec<Probe>) {
    let selected = crate::live::select_candidates(found.to_vec(), pin.map(|(k, _)| *k));
    let mut probes = Vec::new();
    let Some((pinned, label)) = pin else {
        return (selected, probes);
    };
    if found.iter().any(|c| c.kind == *pinned) {
        for candidate in found.iter().filter(|c| c.kind != *pinned) {
            probes.push(Probe {
                kind: candidate.kind,
                candidate: format!("use {}", candidate.endpoint),
                outcome: ProbeOutcome::Skipped {
                    reason: format!("pinned to {pinned} by {label}"),
                },
            });
        }
    } else {
        probes.push(Probe {
            kind: *pinned,
            candidate: format!("pinned by {label}"),
            outcome: ProbeOutcome::Skipped {
                reason: format!(
                    "pinned to {pinned} by {label}, but {pinned} was not found; trying every runtime"
                ),
            },
        });
    }
    (selected, probes)
}

fn time(t: &DateTime<Utc>) -> String {
    t.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn outcome_text(outcome: &ProbeOutcome) -> String {
    let (label, detail) = match outcome {
        ProbeOutcome::Found { endpoint } => ("found", endpoint.as_str()),
        ProbeOutcome::Missing => ("missing", ""),
        ProbeOutcome::Skipped { reason } => ("skipped", reason.as_str()),
        ProbeOutcome::Failed { reason } => ("failed", reason.as_str()),
    };
    if detail.is_empty() {
        label.to_string()
    } else {
        format!("{label:<6} {detail}")
    }
}

/// The report as plain text, for copying into a bug report.
pub fn render_text(report: &DiagnosticsReport) -> String {
    let mut out = String::new();
    let env = &report.environment;
    let var = |v: &Option<String>| v.clone().unwrap_or_else(|| "(not set)".into());
    let _ = writeln!(
        out,
        "Sandbox Watcher {} diagnostics, {}\n",
        report.app_version,
        time(&report.generated_at)
    );
    let _ = writeln!(out, "Environment");
    let _ = writeln!(out, "  OS: {}", env.os);
    let _ = writeln!(out, "  DOCKER_HOST: {}", var(&env.docker_host));
    let _ = writeln!(out, "  CONTAINER_HOST: {}", var(&env.container_host));
    let _ = writeln!(
        out,
        "  SANDBOX_WATCHER_RUNTIME: {}",
        var(&env.runtime_override)
    );
    let config_runtime = env
        .config_runtime
        .map(|r| r.to_string())
        .unwrap_or_else(|| "(not set)".into());
    let _ = writeln!(
        out,
        "  sandbox config: {}, runtime = {config_runtime}",
        env.config_path
    );
    let binaries: Vec<String> = env
        .binaries
        .iter()
        .map(|b| format!("{} {}", b.name, if b.found { "yes" } else { "no" }))
        .collect();
    let _ = writeln!(out, "  on PATH: {}", binaries.join(", "));
    let _ = writeln!(out, "\nLocal runtimes");
    for p in &report.local {
        let _ = writeln!(
            out,
            "  {:<7} {:<30} {}",
            p.kind.as_str(),
            p.candidate,
            outcome_text(&p.outcome)
        );
    }
    for h in &report.hosts {
        let s = &h.status;
        if s.display_name == h.alias {
            let _ = writeln!(out, "\nRemote host {}", h.alias);
        } else {
            let _ = writeln!(out, "\nRemote host {} ({})", h.alias, s.display_name);
        }
        let state = match &s.state {
            HostState::Connected => "connected".to_string(),
            HostState::Connecting => "connecting".to_string(),
            HostState::Disabled => "disabled".to_string(),
            HostState::Unreachable { reason, since } => {
                format!("unreachable since {}: {reason}", time(since))
            }
        };
        let _ = writeln!(out, "  state: {state}");
        for line in &s.probes {
            let _ = writeln!(out, "  {line}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{HostState, HostStatus};
    use chrono::TimeZone;

    fn sample() -> DiagnosticsReport {
        DiagnosticsReport {
            generated_at: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap(),
            app_version: "0.8.0".into(),
            environment: EnvironmentReport {
                os: "linux".into(),
                docker_host: Some("tcp://127.0.0.1:2375".into()),
                container_host: None,
                runtime_override: None,
                config_runtime: Some(RuntimeKind::Docker),
                config_path: "/home/me/.config/sandbox/config.toml".into(),
                binaries: vec![
                    BinaryCheck {
                        name: "docker".into(),
                        found: true,
                    },
                    BinaryCheck {
                        name: "podman".into(),
                        found: false,
                    },
                    BinaryCheck {
                        name: "container".into(),
                        found: false,
                    },
                ],
            },
            local: vec![
                Probe {
                    kind: RuntimeKind::Docker,
                    candidate: "DOCKER_HOST".into(),
                    outcome: ProbeOutcome::Found {
                        endpoint: "tcp://127.0.0.1:2375".into(),
                    },
                },
                Probe {
                    kind: RuntimeKind::Docker,
                    candidate: "connect tcp://127.0.0.1:2375".into(),
                    outcome: ProbeOutcome::Failed {
                        reason: "connection refused".into(),
                    },
                },
                Probe {
                    kind: RuntimeKind::Podman,
                    candidate: "CONTAINER_HOST".into(),
                    outcome: ProbeOutcome::Missing,
                },
                Probe {
                    kind: RuntimeKind::Podman,
                    candidate: "podman on PATH".into(),
                    outcome: ProbeOutcome::Skipped {
                        reason: "pinned to docker by sandbox config".into(),
                    },
                },
            ],
            hosts: vec![HostReport {
                alias: "devbox".into(),
                status: HostStatus {
                    alias: "devbox".into(),
                    display_name: "Dev VM".into(),
                    state: HostState::Unreachable {
                        reason: "Permission denied".into(),
                        since: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 11, 0, 0).unwrap(),
                    },
                    runtimes: vec![],
                    probes: vec!["docker: not found on PATH".into()],
                },
            }],
        }
    }

    #[test]
    fn text_report_has_every_section_and_outcome() {
        let text = render_text(&sample());
        let expected = "\
Sandbox Watcher 0.8.0 diagnostics, 2026-11-01 12:00:00 UTC

Environment
  OS: linux
  DOCKER_HOST: tcp://127.0.0.1:2375
  CONTAINER_HOST: (not set)
  SANDBOX_WATCHER_RUNTIME: (not set)
  sandbox config: /home/me/.config/sandbox/config.toml, runtime = docker
  on PATH: docker yes, podman no, container no

Local runtimes
  docker  DOCKER_HOST                    found  tcp://127.0.0.1:2375
  docker  connect tcp://127.0.0.1:2375   failed connection refused
  podman  CONTAINER_HOST                 missing
  podman  podman on PATH                 skipped pinned to docker by sandbox config

Remote host devbox (Dev VM)
  state: unreachable since 2026-11-01 11:00:00 UTC: Permission denied
  docker: not found on PATH
";
        assert_eq!(text, expected);
    }

    fn found(kind: RuntimeKind, endpoint: &str) -> DetectedRuntime {
        DetectedRuntime {
            kind,
            endpoint: endpoint.into(),
            source: "test".into(),
        }
    }

    fn both() -> Vec<DetectedRuntime> {
        vec![
            found(RuntimeKind::Docker, "/var/run/docker.sock"),
            found(RuntimeKind::Podman, "/run/podman/podman.sock"),
        ]
    }

    #[test]
    fn detected_pin_skips_the_other_runtimes() {
        let pin = (RuntimeKind::Docker, "Settings".to_string());
        let (candidates, probes) = plan_candidates(&both(), Some(&pin));
        assert_eq!(candidates, vec![both()[0].clone()]);
        assert_eq!(
            probes,
            vec![Probe {
                kind: RuntimeKind::Podman,
                candidate: "use /run/podman/podman.sock".into(),
                outcome: ProbeOutcome::Skipped {
                    reason: "pinned to docker by Settings".into(),
                },
            }]
        );
    }

    #[test]
    fn undetected_pin_tries_every_runtime() {
        let pin = (RuntimeKind::AppleContainer, "sandbox config".to_string());
        let (candidates, probes) = plan_candidates(&both(), Some(&pin));
        assert_eq!(candidates, both());
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].kind, RuntimeKind::AppleContainer);
        let pinned = RuntimeKind::AppleContainer;
        assert_eq!(
            probes[0].outcome,
            ProbeOutcome::Skipped {
                reason: format!(
                    "pinned to {pinned} by sandbox config, but {pinned} was not found; trying every runtime"
                ),
            }
        );
    }

    #[test]
    fn no_pin_uses_every_candidate() {
        let (candidates, probes) = plan_candidates(&both(), None);
        assert_eq!(candidates, both());
        assert!(probes.is_empty());
    }

    #[tokio::test]
    async fn disabled_hosts_are_listed_not_tested() {
        let hosts = vec![crate::settings::RemoteHost {
            alias: "off".into(),
            display_name: None,
            enabled: false,
        }];
        let report = run("0.8.0", None, &hosts).await;
        assert_eq!(report.hosts.len(), 1);
        assert_eq!(report.hosts[0].status.state, HostState::Disabled);
        assert!(report.hosts[0].status.probes.is_empty());
    }
}
