//! Find which container runtimes exist on this machine.
//!
//! Mirrors sandbox's own order (`docs/RUNTIMES.md`): Docker, then Podman,
//! then Apple `container`. Like sandbox, this only checks for presence; a
//! stopped daemon is reported by `ping` later, not here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Probe, ProbeOutcome};
use crate::model::RuntimeKind;

/// A runtime candidate found on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedRuntime {
    pub kind: RuntimeKind,
    /// Socket path, named pipe, or CLI binary path.
    pub endpoint: String,
    /// Where the hint came from, for the diagnostics page.
    pub source: String,
}

/// Inputs to detection, separated from the OS so tests can drive it.
pub struct Environment<'a> {
    pub os: &'a str,
    pub home: Option<PathBuf>,
    pub docker_host: Option<String>,
    /// Podman's equivalent of `DOCKER_HOST`.
    pub container_host: Option<String>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub path_exists: &'a dyn Fn(&Path) -> bool,
    pub binary_on_path: &'a dyn Fn(&str) -> bool,
}

impl Environment<'_> {
    /// Build from the real process environment.
    pub fn current() -> Environment<'static> {
        fn exists(p: &Path) -> bool {
            p.exists()
        }
        fn on_path(bin: &str) -> bool {
            let Some(path) = std::env::var_os("PATH") else {
                return false;
            };
            let exts: Vec<String> = if cfg!(windows) {
                vec!["".into(), ".exe".into(), ".cmd".into()]
            } else {
                vec!["".into()]
            };
            std::env::split_paths(&path).any(|dir| {
                exts.iter()
                    .any(|ext| dir.join(format!("{bin}{ext}")).is_file())
            })
        }
        Environment {
            os: std::env::consts::OS,
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
            docker_host: std::env::var("DOCKER_HOST").ok().filter(|v| !v.is_empty()),
            container_host: std::env::var("CONTAINER_HOST")
                .ok()
                .filter(|v| !v.is_empty()),
            xdg_runtime_dir: std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            path_exists: &exists,
            binary_on_path: &on_path,
        }
    }
}

/// Detect runtimes in sandbox's order. Each kind appears at most once.
pub fn detect(env: &Environment<'_>) -> Vec<DetectedRuntime> {
    detect_traced(env).0
}

/// Detection with every candidate recorded, for the diagnostics page. A
/// kind stops at its first hit, like `detect`.
pub fn detect_traced(env: &Environment<'_>) -> (Vec<DetectedRuntime>, Vec<Probe>) {
    let mut trace = Vec::new();
    let mut found = Vec::new();
    for detect_kind in [detect_docker, detect_podman, detect_apple_container] {
        if let Some(r) = detect_kind(env, &mut trace) {
            found.push(r);
        }
    }
    (found, trace)
}

fn hit(
    trace: &mut Vec<Probe>,
    kind: RuntimeKind,
    candidate: &str,
    endpoint: &str,
    source: &str,
) -> Option<DetectedRuntime> {
    trace.push(Probe {
        kind,
        candidate: candidate.into(),
        outcome: ProbeOutcome::Found {
            endpoint: endpoint.into(),
        },
    });
    Some(DetectedRuntime {
        kind,
        endpoint: endpoint.into(),
        source: source.into(),
    })
}

fn miss(trace: &mut Vec<Probe>, kind: RuntimeKind, candidate: &str) {
    trace.push(Probe {
        kind,
        candidate: candidate.into(),
        outcome: ProbeOutcome::Missing,
    });
}

fn detect_docker(env: &Environment<'_>, trace: &mut Vec<Probe>) -> Option<DetectedRuntime> {
    let kind = RuntimeKind::Docker;
    if let Some(host) = &env.docker_host {
        return hit(trace, kind, "DOCKER_HOST", host, "DOCKER_HOST");
    }
    miss(trace, kind, "DOCKER_HOST");
    let candidates: Vec<(PathBuf, &str)> = match env.os {
        "windows" => vec![(
            PathBuf::from(r"\\.\pipe\docker_engine"),
            "default named pipe",
        )],
        "macos" => {
            let mut v = vec![(PathBuf::from("/var/run/docker.sock"), "default socket")];
            if let Some(home) = &env.home {
                v.push((
                    home.join(".docker/run/docker.sock"),
                    "Docker Desktop user socket",
                ));
                v.push((home.join(".colima/default/docker.sock"), "Colima socket"));
                v.push((home.join(".rd/docker.sock"), "Rancher Desktop socket"));
            }
            v
        }
        _ => {
            let mut v = vec![(PathBuf::from("/var/run/docker.sock"), "default socket")];
            if let Some(home) = &env.home {
                v.push((
                    home.join(".docker/desktop/docker.sock"),
                    "Docker Desktop user socket",
                ));
            }
            v
        }
    };
    for (path, source) in candidates {
        let shown = path.to_string_lossy().into_owned();
        if (env.path_exists)(&path) {
            return hit(trace, kind, &shown, &shown, source);
        }
        miss(trace, kind, &shown);
    }
    if (env.binary_on_path)("docker") {
        return hit(
            trace,
            kind,
            "docker on PATH",
            "docker",
            "docker binary on PATH, no socket found",
        );
    }
    miss(trace, kind, "docker on PATH");
    None
}

fn detect_podman(env: &Environment<'_>, trace: &mut Vec<Probe>) -> Option<DetectedRuntime> {
    let kind = RuntimeKind::Podman;
    if let Some(host) = &env.container_host {
        return hit(trace, kind, "CONTAINER_HOST", host, "CONTAINER_HOST");
    }
    miss(trace, kind, "CONTAINER_HOST");
    let mut candidates: Vec<(PathBuf, &str)> = Vec::new();
    match env.os {
        "windows" => candidates.push((
            PathBuf::from(r"\\.\pipe\podman-machine-default"),
            "podman machine pipe",
        )),
        "linux" => {
            if let Some(dir) = &env.xdg_runtime_dir {
                candidates.push((dir.join("podman/podman.sock"), "rootless socket"));
            }
            candidates.push((PathBuf::from("/run/podman/podman.sock"), "system socket"));
        }
        _ => {
            if let Some(home) = &env.home {
                candidates.push((
                    home.join(".local/share/containers/podman/machine/podman.sock"),
                    "podman machine socket",
                ));
            }
        }
    }
    for (path, source) in candidates {
        let shown = path.to_string_lossy().into_owned();
        if (env.path_exists)(&path) {
            return hit(trace, kind, &shown, &shown, source);
        }
        miss(trace, kind, &shown);
    }
    if (env.binary_on_path)("podman") {
        return hit(
            trace,
            kind,
            "podman on PATH",
            "podman",
            "podman binary on PATH, no socket found",
        );
    }
    miss(trace, kind, "podman on PATH");
    None
}

fn detect_apple_container(
    env: &Environment<'_>,
    trace: &mut Vec<Probe>,
) -> Option<DetectedRuntime> {
    let kind = RuntimeKind::AppleContainer;
    if env.os != "macos" {
        trace.push(Probe {
            kind,
            candidate: "container on PATH".into(),
            outcome: ProbeOutcome::Skipped {
                reason: format!("Apple container exists on macOS only, this is {}", env.os),
            },
        });
        return None;
    }
    if (env.binary_on_path)("container") {
        return hit(
            trace,
            kind,
            "container on PATH",
            "container",
            "container binary on PATH",
        );
    }
    miss(trace, kind, "container on PATH");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(
        os: &'a str,
        docker_host: Option<&str>,
        exists: &'a dyn Fn(&Path) -> bool,
        on_path: &'a dyn Fn(&str) -> bool,
    ) -> Environment<'a> {
        Environment {
            os,
            home: Some(PathBuf::from("/home/me")),
            docker_host: docker_host.map(String::from),
            container_host: None,
            xdg_runtime_dir: Some(PathBuf::from("/run/user/1000")),
            path_exists: exists,
            binary_on_path: on_path,
        }
    }

    #[test]
    fn nothing_installed_detects_nothing() {
        let no = |_: &Path| false;
        let none = |_: &str| false;
        assert!(detect(&env("linux", None, &no, &none)).is_empty());
    }

    #[test]
    fn docker_host_wins_over_sockets() {
        let yes = |_: &Path| true;
        let none = |_: &str| false;
        let found = detect(&env("linux", Some("tcp://127.0.0.1:2375"), &yes, &none));
        assert_eq!(found[0].endpoint, "tcp://127.0.0.1:2375");
        assert_eq!(found[0].source, "DOCKER_HOST");
    }

    #[test]
    fn rootless_podman_socket_on_linux() {
        let exists = |p: &Path| p == Path::new("/run/user/1000/podman/podman.sock");
        let none = |_: &str| false;
        let found = detect(&env("linux", None, &exists, &none));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, RuntimeKind::Podman);
        assert_eq!(found[0].source, "rootless socket");
    }

    #[test]
    fn apple_container_only_on_macos_in_sandbox_order() {
        let no = |_: &Path| false;
        let all = |_: &str| true;
        let kinds: Vec<_> = detect(&env("macos", None, &no, &all))
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                RuntimeKind::Docker,
                RuntimeKind::Podman,
                RuntimeKind::AppleContainer
            ]
        );
        let kinds: Vec<_> = detect(&env("linux", None, &no, &all))
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(kinds, vec![RuntimeKind::Docker, RuntimeKind::Podman]);
    }

    #[test]
    fn trace_lists_every_candidate_in_order() {
        use crate::diagnostics::ProbeOutcome;
        let exists = |p: &Path| p == Path::new("/home/me/.docker/desktop/docker.sock");
        let on_path = |b: &str| b == "podman";
        let (found, trace) = detect_traced(&env("linux", None, &exists, &on_path));
        assert_eq!(found, detect(&env("linux", None, &exists, &on_path)));
        // Joined paths use the host separator; compare with forward slashes.
        let names: Vec<String> = trace
            .iter()
            .map(|p| p.candidate.replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            [
                "DOCKER_HOST",
                "/var/run/docker.sock",
                "/home/me/.docker/desktop/docker.sock",
                "CONTAINER_HOST",
                "/run/user/1000/podman/podman.sock",
                "/run/podman/podman.sock",
                "podman on PATH",
                "container on PATH",
            ]
        );
        assert!(matches!(trace[2].outcome, ProbeOutcome::Found { .. }));
        assert!(matches!(trace[6].outcome, ProbeOutcome::Found { .. }));
        assert!(
            matches!(trace[7].outcome, ProbeOutcome::Skipped { .. }),
            "{:?}",
            trace[7]
        );
        assert_eq!(
            trace
                .iter()
                .filter(|p| matches!(p.outcome, ProbeOutcome::Missing))
                .count(),
            5
        );
    }

    #[test]
    fn trace_stops_a_kind_at_its_first_hit() {
        let yes = |_: &Path| true;
        let all = |_: &str| true;
        let (_, trace) = detect_traced(&env("macos", Some("tcp://x:1"), &yes, &all));
        let docker: Vec<_> = trace
            .iter()
            .filter(|p| p.kind == RuntimeKind::Docker)
            .collect();
        assert_eq!(docker.len(), 1);
        assert_eq!(docker[0].candidate, "DOCKER_HOST");
    }
}
