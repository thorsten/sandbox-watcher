//! Find which container runtimes exist on this machine.
//!
//! Mirrors sandbox's own order (`docs/RUNTIMES.md`): Docker, then Podman,
//! then Apple `container`. Like sandbox, this only checks for presence; a
//! stopped daemon is reported by `ping` later, not here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    let mut found = Vec::new();
    if let Some(r) = detect_docker(env) {
        found.push(r);
    }
    if let Some(r) = detect_podman(env) {
        found.push(r);
    }
    if let Some(r) = detect_apple_container(env) {
        found.push(r);
    }
    found
}

fn detect_docker(env: &Environment<'_>) -> Option<DetectedRuntime> {
    if let Some(host) = &env.docker_host {
        return Some(DetectedRuntime {
            kind: RuntimeKind::Docker,
            endpoint: host.clone(),
            source: "DOCKER_HOST".into(),
        });
    }
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
        if (env.path_exists)(&path) {
            return Some(DetectedRuntime {
                kind: RuntimeKind::Docker,
                endpoint: path.to_string_lossy().into_owned(),
                source: source.into(),
            });
        }
    }
    if (env.binary_on_path)("docker") {
        return Some(DetectedRuntime {
            kind: RuntimeKind::Docker,
            endpoint: "docker".into(),
            source: "docker binary on PATH, no socket found".into(),
        });
    }
    None
}

fn detect_podman(env: &Environment<'_>) -> Option<DetectedRuntime> {
    if let Some(host) = &env.container_host {
        return Some(DetectedRuntime {
            kind: RuntimeKind::Podman,
            endpoint: host.clone(),
            source: "CONTAINER_HOST".into(),
        });
    }
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
        if (env.path_exists)(&path) {
            return Some(DetectedRuntime {
                kind: RuntimeKind::Podman,
                endpoint: path.to_string_lossy().into_owned(),
                source: source.into(),
            });
        }
    }
    if (env.binary_on_path)("podman") {
        return Some(DetectedRuntime {
            kind: RuntimeKind::Podman,
            endpoint: "podman".into(),
            source: "podman binary on PATH, no socket found".into(),
        });
    }
    None
}

fn detect_apple_container(env: &Environment<'_>) -> Option<DetectedRuntime> {
    if env.os != "macos" {
        return None;
    }
    (env.binary_on_path)("container").then(|| DetectedRuntime {
        kind: RuntimeKind::AppleContainer,
        endpoint: "container".into(),
        source: "container binary on PATH".into(),
    })
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
}
