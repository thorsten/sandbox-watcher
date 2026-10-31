//! Environment of the user's login shell, for apps started outside a terminal.
//!
//! On macOS an app opened from Finder or the Dock inherits launchd's minimal
//! environment: `PATH` is `/usr/bin:/bin:/usr/sbin:/sbin`, and variables set
//! in `.zprofile` or `.zshrc` are missing. Runtime detection then misses
//! `container`, Homebrew's `docker` and `podman`, and a `DOCKER_HOST` the
//! user set. Asking the login shell once at startup closes that gap.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Variables taken from the login shell. Only these are read.
pub const IMPORTED_VARS: [&str; 3] = ["PATH", "DOCKER_HOST", "CONTAINER_HOST"];

const START_MARKER: &str = "__SANDBOX_WATCHER_ENV_START__";
const END_MARKER: &str = "__SANDBOX_WATCHER_ENV_END__";
const SHELL_TIMEOUT: Duration = Duration::from_secs(3);

/// Whether the process looks like it was started outside a terminal, where
/// the environment is launchd's rather than the user's shell.
pub fn started_outside_terminal(term: Option<&str>) -> bool {
    term.is_none_or(|t| t.is_empty())
}

/// Run `$SHELL -ilc` and return the imported variables it reports. `None`
/// when the shell is missing, fails, or takes longer than three seconds.
pub fn login_shell_env(shell: &str) -> Option<HashMap<String, String>> {
    // `echo` and `env` behave the same in sh, bash, zsh and fish.
    let script = format!("echo {START_MARKER}; env; echo {END_MARKER}");
    let mut child = Command::new(shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // Keep oh-my-zsh and similar frameworks from prompting for updates.
        .env("DISABLE_AUTO_UPDATE", "true")
        .spawn()
        .ok()?;
    let deadline = Instant::now() + SHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    Some(parse_env_block(&output))
}

/// Pick the imported variables from the block between the markers. Lines
/// outside it come from shell start-up files and are ignored, as are
/// continuation lines of multi-line values.
pub fn parse_env_block(output: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    let Some((_, rest)) = output.split_once(START_MARKER) else {
        return vars;
    };
    let block = rest.split(END_MARKER).next().unwrap_or("");
    for line in block.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if IMPORTED_VARS.contains(&key) && !value.is_empty() {
            vars.insert(key.to_string(), value.to_string());
        }
    }
    vars
}

/// The shell's `PATH` first, then entries only the current `PATH` has.
pub fn merge_path(shell_path: &str, current: &str) -> String {
    let mut seen = Vec::new();
    for entry in shell_path.split(':').chain(current.split(':')) {
        if !entry.is_empty() && !seen.contains(&entry) {
            seen.push(entry);
        }
    }
    seen.join(":")
}

/// Variables to set in this process: the merged `PATH`, and the runtime host
/// variables only where the process has none of its own.
pub fn updates(
    shell: &HashMap<String, String>,
    current: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(shell_path) = shell.get("PATH") {
        let merged = merge_path(shell_path, &current("PATH").unwrap_or_default());
        if current("PATH").as_deref() != Some(merged.as_str()) {
            out.push(("PATH".to_string(), merged));
        }
    }
    for key in ["DOCKER_HOST", "CONTAINER_HOST"] {
        let unset = current(key).is_none_or(|v| v.is_empty());
        if unset && let Some(value) = shell.get(key) {
            out.push((key.to_string(), value.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_imported_vars_inside_markers() {
        let out = format!(
            "Welcome!\nPATH=/noise\n{START_MARKER}\nPATH=/opt/homebrew/bin:/usr/bin\nSECRET=x\nDOCKER_HOST=unix:///a.sock\nCONTAINER_HOST=\nMULTI=a\nb=c\n{END_MARKER}\nPATH=/after\n"
        );
        let vars = parse_env_block(&out);
        assert_eq!(vars.get("PATH").unwrap(), "/opt/homebrew/bin:/usr/bin");
        assert_eq!(vars.get("DOCKER_HOST").unwrap(), "unix:///a.sock");
        assert!(!vars.contains_key("CONTAINER_HOST"), "empty value skipped");
        assert!(!vars.contains_key("SECRET"));
        assert_eq!(vars.len(), 2);
        assert!(parse_env_block("no markers").is_empty());
    }

    #[test]
    fn merges_path_shell_first_without_duplicates() {
        assert_eq!(
            merge_path("/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin:/usr/sbin"),
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin"
        );
        assert_eq!(merge_path("", "/usr/bin"), "/usr/bin");
    }

    #[test]
    fn updates_keep_existing_host_vars() {
        let shell = HashMap::from([
            ("PATH".to_string(), "/opt/homebrew/bin".to_string()),
            ("DOCKER_HOST".to_string(), "unix:///shell.sock".to_string()),
            (
                "CONTAINER_HOST".to_string(),
                "unix:///podman.sock".to_string(),
            ),
        ]);
        let current = |key: &str| match key {
            "PATH" => Some("/usr/bin".to_string()),
            "DOCKER_HOST" => Some("unix:///own.sock".to_string()),
            _ => None,
        };
        assert_eq!(
            updates(&shell, current),
            vec![
                ("PATH".to_string(), "/opt/homebrew/bin:/usr/bin".to_string()),
                (
                    "CONTAINER_HOST".to_string(),
                    "unix:///podman.sock".to_string()
                ),
            ]
        );
    }

    #[test]
    fn unchanged_path_is_not_rewritten() {
        let shell = HashMap::from([("PATH".to_string(), "/usr/bin".to_string())]);
        assert!(updates(&shell, |_| Some("/usr/bin".to_string())).is_empty());
    }

    #[test]
    fn terminal_detection() {
        assert!(started_outside_terminal(None));
        assert!(started_outside_terminal(Some("")));
        assert!(!started_outside_terminal(Some("xterm-256color")));
    }

    #[cfg(unix)]
    #[test]
    fn reads_env_from_a_real_shell() {
        let vars = login_shell_env("/bin/sh").expect("sh runs");
        assert!(vars.get("PATH").is_some_and(|p| !p.is_empty()));
        assert!(login_shell_env("/nonexistent/shell").is_none());
    }
}
