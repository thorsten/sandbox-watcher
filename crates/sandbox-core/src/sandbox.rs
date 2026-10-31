//! Conventions of MaibornWolff Sandbox that the watcher relies on.
//!
//! Taken from `@maibornwolff/sandbox` 0.73.1. None of this is a public API of
//! sandbox, so every constant notes the source file it was read from. Bump
//! `TESTED_SANDBOX_VERSION` after re-checking these against a new release,
//! then record the files with `pnpm check:upstream --update`.

/// Newest sandbox release these conventions were verified against.
pub const TESTED_SANDBOX_VERSION: &str = "0.73.1";

use crate::model::Readiness;

/// Label carrying the project slug. `src/modules/sandbox-containers/container-labels.ts`.
pub const PROJECT_LABEL: &str = "sandbox.project";

/// Label carrying the 12-char configuration hash. `src/modules/sandbox-containers/container-hashing.ts`.
pub const HASH_LABEL: &str = "sandbox.hash";

/// Marker created by the entrypoint once the container accepts sessions.
/// `src/platform/container-system/container-paths.ts`.
pub const READY_FILE: &str = "/tmp/.sandbox-ready";

/// Directory holding one marker file per active session, named by PID.
/// `src/platform/container-system/container-paths.ts`.
pub const SESSIONS_DIRECTORY: &str = "/tmp/sandbox-sessions";

/// In-container helper binary used for network diagnostics.
/// `src/modules/network-diagnostics/container-diagnostic-commands.ts`.
pub const CONTAINER_TOOLS_PATH: &str = "/usr/local/bin/sandbox-container-tools";

/// Home directory of the container user. `configuration/mount-path-resolution.ts`.
pub const CONTAINER_HOME: &str = "/home/sandbox";

/// Where the read-only runtime files are mounted. `sandbox-runtime/runtime-paths.ts`.
pub const CONTAINER_RUNTIME_DIRECTORY: &str = "/opt/sandbox-cli";

/// Prefix of every sandbox container name. `container-naming.ts`.
pub const CONTAINER_NAME_PREFIX: &str = "sandbox-";

/// Split a container name into project slug and instance number.
///
/// Sandbox names containers `sandbox-<slug>` and appends `-2`, `-3`, ... when a
/// project needs more than one container (`container-discovery.ts`,
/// `findAvailableName`). Slugs can themselves contain `-` and digits, so the
/// label is the authoritative source for the slug; this helper is only for
/// display and for runtimes where labels are missing.
pub fn parse_container_name(name: &str) -> Option<(String, u32)> {
    let rest = name.strip_prefix('/').unwrap_or(name);
    let rest = rest.strip_prefix(CONTAINER_NAME_PREFIX)?;
    if rest.is_empty() {
        return None;
    }
    if let Some((slug, suffix)) = rest.rsplit_once('-')
        && let Ok(n) = suffix.parse::<u32>()
        && n >= 2
        && !slug.is_empty()
    {
        return Some((slug.to_string(), n));
    }
    Some((rest.to_string(), 1))
}

/// Marker passed as `$0` to our probe so its `exec_create` event can be told
/// apart from execs started by sandbox or the user.
pub const PROBE_SENTINEL: &str = "sandbox-watcher-probe";

/// What one probe exec reports about a running container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    pub readiness: Readiness,
    /// `(pid, command line)` of each live session.
    pub sessions: Vec<(u32, String)>,
}

/// Shell snippet that reports readiness and active sessions in one exec.
///
/// First line is `ready` or `starting` depending on [`READY_FILE`]. Then one
/// `<pid>|<cmdline>` line per session marker, mirroring what `sandbox status`
/// runs (`container-status.ts`, `buildSessionDetailsCommand` in
/// `session-scripts.ts`): markers whose PID is gone are removed,
/// `/proc/<pid>/cmdline` is read for the rest and NUL separators become spaces.
pub fn status_probe_command() -> Vec<String> {
    let script = format!(
        "if [ -f '{ready}' ]; then echo ready; else echo starting; fi; \
         d='{dir}'; [ -d \"$d\" ] || exit 0; \
         for m in \"$d\"/*; do [ -e \"$m\" ] || continue; p=\"${{m##*/}}\"; \
         if [ -d \"/proc/$p\" ]; then \
           printf '%s|' \"$p\"; tr '\\0' ' ' < \"/proc/$p/cmdline\" 2>/dev/null || true; printf '\\n'; \
         else rm -f \"$m\"; fi; done",
        ready = READY_FILE,
        dir = SESSIONS_DIRECTORY
    );
    vec!["sh".into(), "-c".into(), script, PROBE_SENTINEL.into()]
}

/// Parse the output of [`status_probe_command`].
pub fn parse_probe(output: &str) -> ProbeReport {
    let mut lines = output.lines().peekable();
    let readiness = match lines.peek().map(|l| l.trim()) {
        Some("ready") => {
            lines.next();
            Readiness::Ready
        }
        Some("starting") => {
            lines.next();
            Readiness::Starting
        }
        _ => Readiness::Unknown,
    };
    let sessions = lines
        .filter_map(|line| {
            let (pid, command) = line.split_once('|')?;
            let pid = pid.trim().parse::<u32>().ok()?;
            let command = command.trim();
            let command = if command.is_empty() {
                "unknown"
            } else {
                command
            };
            Some((pid, command.to_string()))
        })
        .collect();
    ProbeReport {
        readiness,
        sessions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_numbered_names() {
        assert_eq!(
            parse_container_name("sandbox-my-app"),
            Some(("my-app".into(), 1))
        );
        assert_eq!(
            parse_container_name("/sandbox-my-app-3"),
            Some(("my-app".into(), 3))
        );
        // "-1" is never generated by sandbox, treat it as part of the slug.
        assert_eq!(
            parse_container_name("sandbox-app-1"),
            Some(("app-1".into(), 1))
        );
        assert_eq!(parse_container_name("other-thing"), None);
        assert_eq!(parse_container_name("sandbox-"), None);
    }

    #[test]
    fn parses_probe_output() {
        let out = "ready\n123|claude --dangerously-skip-permissions\n  456 | \nbroken\n7|zsh -l\n";
        let report = parse_probe(out);
        assert_eq!(report.readiness, Readiness::Ready);
        assert_eq!(
            report.sessions,
            vec![
                (123, "claude --dangerously-skip-permissions".to_string()),
                (456, "unknown".to_string()),
                (7, "zsh -l".to_string()),
            ]
        );
        assert_eq!(parse_probe("starting\n").readiness, Readiness::Starting);
        let legacy = parse_probe("42|codex\n");
        assert_eq!(legacy.readiness, Readiness::Unknown);
        assert_eq!(legacy.sessions.len(), 1);
    }

    #[test]
    fn probe_command_carries_sentinel() {
        let cmd = status_probe_command();
        assert_eq!(cmd[0], "sh");
        assert_eq!(cmd.last().unwrap(), PROBE_SENTINEL);
        assert!(cmd[2].contains(READY_FILE) && cmd[2].contains(SESSIONS_DIRECTORY));
    }
}
