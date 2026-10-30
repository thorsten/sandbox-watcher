//! Classify a container's mounts by what sandbox uses them for.

use crate::model::{ContainerDetails, MountRole};
use crate::sandbox::{CONTAINER_HOME, CONTAINER_RUNTIME_DIRECTORY};

fn is_under(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Sanitise the way sandbox's `generateProjectSlug` does: lowercase,
/// non-alphanumerics become dashes, runs collapse, ends trimmed.
pub fn sanitize_slug_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Does this project slug belong to a project at `path`? Sandbox builds the
/// slug from the last two path components plus a hash (`project-slug.ts`).
pub fn slug_matches_path(slug: &str, path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    let parts: Vec<&str> = normalised.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return false;
    }
    let tail = parts[parts.len().saturating_sub(2)..].join("-");
    let prefix = sanitize_slug_component(&tail);
    !prefix.is_empty() && slug.starts_with(&format!("{prefix}-"))
}

/// Assign a role to every mount.
///
/// Sandbox mounts the project (and an external git worktree, if any) at the
/// same path inside the container as on the host (`container-arguments.ts`,
/// mounts of type `workspace`). So:
///
/// - `Project`: the mount containing the working directory, if the runtime
///   reports one; otherwise the identity mount (source == destination) whose
///   path matches the `sandbox.project` slug; otherwise the only identity mount.
/// - `Runtime`: sandbox's read-only runtime files under `/opt/sandbox-cli`.
/// - `Persisted`: named volumes, bind mounts into the container home
///   (`/home/sandbox/...`) that come from elsewhere on the host, and bind
///   mounts whose host side is a sandbox-managed `data` directory.
/// - `Other`: everything else, typically extra host paths from the config and
///   external worktrees.
pub fn classify(details: &mut ContainerDetails) {
    let working_dir = details.working_dir.clone();
    let slug = details.container.project_slug.clone();

    let identity: Vec<usize> = details
        .mounts
        .iter()
        .enumerate()
        .filter(|(_, m)| m.kind == "bind" && same_path(&m.source, &m.destination))
        .filter(|(_, m)| !is_under(&m.destination, CONTAINER_RUNTIME_DIRECTORY))
        .map(|(i, _)| i)
        .collect();
    let by_working_dir = working_dir
        .as_deref()
        .filter(|wd| !wd.is_empty())
        .and_then(|wd| {
            details
                .mounts
                .iter()
                .position(|m| m.kind == "bind" && is_under(wd, &m.destination))
        });
    let by_slug = identity
        .iter()
        .copied()
        .find(|&i| slug_matches_path(&slug, &details.mounts[i].destination));
    let only_identity = (identity.len() == 1).then(|| identity[0]);
    let project_index = by_working_dir.or(by_slug).or(only_identity);

    for (i, mount) in details.mounts.iter_mut().enumerate() {
        mount.role = if is_under(&mount.destination, CONTAINER_RUNTIME_DIRECTORY) {
            MountRole::Runtime
        } else if Some(i) == project_index {
            MountRole::Project
        } else if mount.kind == "volume"
            || looks_persisted(&mount.source)
            || (mount.kind == "bind"
                && is_under(&mount.destination, CONTAINER_HOME)
                && !same_path(&mount.source, &mount.destination))
        {
            MountRole::Persisted
        } else {
            MountRole::Other
        };
    }
}

/// Compare a host path with a container path, tolerating Windows spellings
/// (`C:\\x` vs `/c/x`) and trailing slashes.
fn same_path(host: &str, container: &str) -> bool {
    fn norm(p: &str) -> String {
        let mut s = p.replace('\\', "/");
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            let drive = s.as_bytes()[0].to_ascii_lowercase() as char;
            s = format!("/{drive}{}", &s[2..]);
        }
        s.trim_end_matches('/').to_string()
    }
    norm(host) == norm(container)
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
                    host_path: None,
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
    fn classifies_by_sandbox_conventions_with_working_dir() {
        let mut d = details(
            "/Users/dev/web-shop/packages/api",
            vec![
                ("/Users/dev/web-shop", "/Users/dev/web-shop", "bind"),
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
    fn without_working_dir_the_identity_mount_matching_the_slug_is_the_project() {
        let mut d = details(
            "",
            vec![
                (
                    "/Users/dev/worktrees/feature-x",
                    "/Users/dev/worktrees/feature-x",
                    "bind",
                ),
                (
                    "/Users/dev/Projects/web-shop",
                    "/Users/dev/Projects/web-shop",
                    "bind",
                ),
                ("/Users/dev/.agents", "/home/sandbox/.agents", "bind"),
            ],
        );
        d.working_dir = None;
        d.container.project_slug = "projects-web-shop-7f3a".into();
        classify(&mut d);
        let roles: Vec<_> = d.mounts.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![MountRole::Other, MountRole::Project, MountRole::Persisted]
        );
    }

    #[test]
    fn a_single_identity_mount_is_the_project_even_without_slug_match() {
        let mut d = details("", vec![("/srv/app", "/srv/app", "bind")]);
        d.working_dir = None;
        d.container.project_slug = "something-else-0000".into();
        classify(&mut d);
        assert_eq!(d.mounts[0].role, MountRole::Project);
    }

    #[test]
    fn slug_matching_follows_sandbox() {
        assert!(slug_matches_path(
            "projects-web-shop-7f3a",
            "/Users/dev/Projects/web-shop"
        ));
        assert!(slug_matches_path(
            "opensource-sandbox-watcher-1a2b",
            "/Users/me/Projects/OpenSource/sandbox-watcher/"
        ));
        assert!(slug_matches_path("work-my-app-1234", r"C:\work\My App"));
        assert!(!slug_matches_path(
            "projects-web-shop-7f3a",
            "/Users/dev/other/web-shop"
        ));
        assert_eq!(
            sanitize_slug_component("OpenSource-sandbox--watcher!"),
            "opensource-sandbox-watcher"
        );
    }

    #[test]
    fn windows_identity_mounts_are_recognised() {
        assert!(same_path(r"C:\work\app", "/c/work/app"));
        assert!(same_path("/Users/me/p/", "/Users/me/p"));
        assert!(!same_path("/Users/me/p", "/home/sandbox/p"));
    }

    #[test]
    fn prefix_matching_respects_path_boundaries() {
        assert!(is_under("/opt/sandbox-cli/bin", "/opt/sandbox-cli"));
        assert!(!is_under("/opt/sandbox-cli-extra", "/opt/sandbox-cli"));
    }
}
