//! Classify a container's mounts by what sandbox uses them for.

use crate::model::{ContainerDetails, MountRole};
use crate::sandbox::CONTAINER_RUNTIME_DIRECTORY;

fn is_under(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Assign a role to every mount.
///
/// - `Project`: the mount that contains the container's working directory.
/// - `Runtime`: sandbox's own read-only runtime files under `/opt/sandbox-cli`.
/// - `Persisted`: named volumes, and bind mounts whose host side lives in a
///   sandbox-managed `data` directory (`{data}/<project-slug>/`, `{data}/global/`).
/// - `Other`: everything else, typically extra host paths from the config.
pub fn classify(details: &mut ContainerDetails) {
    let working_dir = details.working_dir.clone();
    for mount in &mut details.mounts {
        mount.role = if is_under(&mount.destination, CONTAINER_RUNTIME_DIRECTORY) {
            MountRole::Runtime
        } else if working_dir
            .as_deref()
            .is_some_and(|wd| is_under(wd, &mount.destination))
        {
            MountRole::Project
        } else if mount.kind == "volume" || looks_persisted(&mount.source) {
            MountRole::Persisted
        } else {
            MountRole::Other
        };
    }
}

fn looks_persisted(source: &str) -> bool {
    let lower = source.to_ascii_lowercase().replace('\\', "/");
    lower.contains("/sandbox/data/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContainerState, Mount, Readiness, SandboxContainer};
    use std::collections::BTreeMap;

    fn details(working_dir: &str, mounts: Vec<(&str, &str, &str)>) -> ContainerDetails {
        ContainerDetails {
            container: SandboxContainer {
                id: "x".into(),
                name: "sandbox-x".into(),
                project_slug: "x".into(),
                image: "img".into(),
                state: ContainerState::Running,
                created_at: None,
                started_at: None,
                config_hash: None,
                labels: BTreeMap::new(),
            },
            readiness: Readiness::Unknown,
            mounts: mounts
                .into_iter()
                .map(|(source, destination, kind)| Mount {
                    source: source.into(),
                    destination: destination.into(),
                    read_only: false,
                    kind: kind.into(),
                    role: MountRole::Other,
                })
                .collect(),
            ports: vec![],
            env_names: vec![],
            user: None,
            working_dir: Some(working_dir.into()),
            exit_code: None,
        }
    }

    #[test]
    fn classifies_by_sandbox_conventions() {
        let mut d = details(
            "/home/sandbox/web-shop/packages/api",
            vec![
                ("/Users/dev/web-shop", "/home/sandbox/web-shop", "bind"),
                (
                    "/Users/dev/Library/Application Support/sandbox/data/web-shop/home/.claude",
                    "/home/sandbox/.claude",
                    "bind",
                ),
                ("sandbox-npm-cache", "/home/sandbox/.npm", "volume"),
                (
                    "/Users/dev/Library/Caches/sandbox/runtime/0.73.1-abc",
                    "/opt/sandbox-cli",
                    "bind",
                ),
                ("/Users/dev/notes", "/mnt/notes", "bind"),
            ],
        );
        classify(&mut d);
        let roles: Vec<_> = d.mounts.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![
                MountRole::Project,
                MountRole::Persisted,
                MountRole::Persisted,
                MountRole::Runtime,
                MountRole::Other
            ]
        );
    }

    #[test]
    fn prefix_matching_respects_path_boundaries() {
        assert!(is_under("/opt/sandbox-cli/bin", "/opt/sandbox-cli"));
        assert!(!is_under("/opt/sandbox-cli-extra", "/opt/sandbox-cli"));
    }
}
