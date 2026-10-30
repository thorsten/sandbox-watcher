//! The few things you can do from the overview: stop, open, copy.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::adapter::{ContainerFilter, RuntimeAdapter};
use crate::model::MountRole;
use crate::mounts::classify;
use crate::paths::annotate_host_paths;

/// Outcome of stopping one container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopResult {
    pub container_id: String,
    pub name: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Default grace period before the runtime kills the container.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Stop every running sandbox container of every adapter.
pub async fn stop_all(adapters: &[Arc<dyn RuntimeAdapter>], timeout: Duration) -> Vec<StopResult> {
    let mut results = Vec::new();
    for adapter in adapters {
        let Ok(containers) = adapter.list_containers(&ContainerFilter::running()).await else {
            continue;
        };
        let stops = containers.iter().map(|c| {
            let adapter = adapter.clone();
            async move {
                let outcome = adapter.stop(&c.id, Some(timeout)).await;
                StopResult {
                    container_id: c.id.clone(),
                    name: c.name.clone(),
                    ok: outcome.is_ok(),
                    error: outcome.err().map(|e| e.to_string()),
                }
            }
        });
        results.extend(futures::future::join_all(stops).await);
    }
    results
}

/// Host path of the container's project directory, spelled for this OS.
pub async fn project_path(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
    windows_host: bool,
) -> Result<Option<String>> {
    let Some(mut details) = adapter.inspect(container_id).await? else {
        return Ok(None);
    };
    classify(&mut details);
    annotate_host_paths(&mut details, windows_host);
    Ok(details
        .mounts
        .iter()
        .find(|m| m.role == MountRole::Project && m.kind == "bind")
        .and_then(|m| m.host_path.clone().or_else(|| Some(m.source.clone()))))
}

/// One way to open a terminal in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalLaunch {
    pub program: String,
    pub args: Vec<String>,
    /// Working directory to start the process in. Most terminals open there.
    pub cwd: Option<String>,
    /// Wait for the process and require exit code 0 before calling it a
    /// success. Needed for `osascript`, which starts fine and then fails.
    pub wait: bool,
}

/// Terminal apps we know how to drive on macOS, in preference order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacTerminal {
    ITerm,
    TerminalApp,
}

impl MacTerminal {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "iterm" | "iterm2" | "iterm.app" => Some(MacTerminal::ITerm),
            "terminal" | "terminal.app" | "apple" => Some(MacTerminal::TerminalApp),
            _ => None,
        }
    }
}

fn applescript_launch(lines: &[String]) -> TerminalLaunch {
    let mut args = Vec::with_capacity(lines.len() * 2);
    for line in lines {
        args.push("-e".into());
        args.push(line.clone());
    }
    TerminalLaunch {
        program: "osascript".into(),
        args,
        cwd: None,
        wait: true,
    }
}

fn applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Shell-quote for the command typed into the new terminal.
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Candidate launches, most specific first. The caller tries them in order
/// until one succeeds. `terminal_env` is `$TERMINAL` on Linux. On macOS,
/// `mac_terminals` lists the installed apps in preference order (the user's
/// explicit choice first, then whatever is installed, Terminal.app last).
pub fn terminal_launches(
    os: &str,
    dir: &str,
    terminal_env: Option<&str>,
    mac_terminals: &[MacTerminal],
) -> Vec<TerminalLaunch> {
    let hint = "echo 'sandbox-watcher: run `sandbox` for a shell in this project, `sandbox status` for sessions'";
    match os {
        "macos" => {
            let script = applescript_string(&format!(
                "cd {} && clear && {hint}",
                shell_single_quote(dir)
            ));
            let mut launches = Vec::new();
            let mut order: Vec<MacTerminal> = mac_terminals.to_vec();
            if !order.contains(&MacTerminal::TerminalApp) {
                order.push(MacTerminal::TerminalApp);
            }
            for app in order {
                match app {
                    MacTerminal::ITerm => launches.push(applescript_launch(&[
                        "tell application \"iTerm\"".into(),
                        "activate".into(),
                        "set newWindow to (create window with default profile)".into(),
                        format!("tell current session of newWindow to write text \"{script}\""),
                        "end tell".into(),
                    ])),
                    MacTerminal::TerminalApp => launches.push(applescript_launch(&[
                        "tell application \"Terminal\"".into(),
                        "activate".into(),
                        format!("do script \"{script}\""),
                        "end tell".into(),
                    ])),
                }
            }
            launches.push(TerminalLaunch {
                program: "open".into(),
                args: vec!["-a".into(), "Terminal".into(), dir.into()],
                cwd: None,
                wait: true,
            });
            launches
        }
        "windows" => vec![
            TerminalLaunch {
                program: "wt.exe".into(),
                args: vec!["-d".into(), dir.into()],
                cwd: None,
                wait: false,
            },
            TerminalLaunch {
                program: "cmd.exe".into(),
                args: vec!["/c".into(), "start".into(), String::new(), "cmd.exe".into()],
                cwd: Some(dir.into()),
                wait: false,
            },
        ],
        _ => {
            let mut launches = Vec::new();
            if let Some(term) = terminal_env.filter(|t| !t.trim().is_empty()) {
                launches.push(TerminalLaunch {
                    program: term.into(),
                    args: vec![],
                    cwd: Some(dir.into()),
                    wait: false,
                });
            }
            for program in [
                "x-terminal-emulator",
                "gnome-terminal",
                "konsole",
                "xfce4-terminal",
                "kitty",
                "alacritty",
                "foot",
                "xterm",
            ] {
                launches.push(TerminalLaunch {
                    program: program.into(),
                    args: vec![],
                    cwd: Some(dir.into()),
                    wait: false,
                });
            }
            launches
        }
    }
}

/// Command line to copy for inspecting network logs of a project.
pub fn network_logs_command(project_dir: Option<&str>) -> String {
    match project_dir {
        Some(dir) => format!(
            "cd {} && sandbox network logs --status=all",
            shell_single_quote(dir)
        ),
        None => "sandbox network logs --status=all".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockAdapter;

    #[test]
    fn macos_prefers_iterm_when_listed_and_escapes_the_path() {
        let dir = "/Users/me/it's \"here\"";
        let launches = terminal_launches("macos", dir, None, &[MacTerminal::ITerm]);
        assert_eq!(launches.len(), 3, "iTerm, Terminal.app, open fallback");
        assert_eq!(launches[0].program, "osascript");
        assert!(launches[0].wait);
        assert!(launches[0].args.iter().any(|a| a.contains("iTerm")));
        let write = launches[0]
            .args
            .iter()
            .find(|a| a.starts_with("tell current session"))
            .unwrap();
        assert!(
            write.contains("cd '/Users/me/it'\\\\''s \\\"here\\\"'"),
            "{write}"
        );
        assert!(launches[1].args.iter().any(|a| a.contains("Terminal")));
        assert_eq!(launches[2].program, "open");
    }

    #[test]
    fn macos_without_iterm_goes_straight_to_terminal_app() {
        let launches = terminal_launches("macos", "/p", None, &[]);
        assert_eq!(launches.len(), 2);
        assert!(
            launches[0]
                .args
                .iter()
                .any(|a| a == "tell application \"Terminal\"")
        );
    }

    #[test]
    fn mac_terminal_names() {
        assert_eq!(MacTerminal::from_name("iTerm2"), Some(MacTerminal::ITerm));
        assert_eq!(
            MacTerminal::from_name(" terminal "),
            Some(MacTerminal::TerminalApp)
        );
        assert_eq!(MacTerminal::from_name("ghostty"), None);
    }

    #[test]
    fn linux_prefers_terminal_env_and_starts_in_dir() {
        let launches = terminal_launches("linux", "/home/me/p", Some("wezterm"), &[]);
        assert_eq!(launches[0].program, "wezterm");
        assert_eq!(launches[0].cwd.as_deref(), Some("/home/me/p"));
        assert!(!launches[0].wait);
        assert!(launches.iter().any(|l| l.program == "gnome-terminal"));
        let without = terminal_launches("linux", "/home/me/p", Some("  "), &[]);
        assert_eq!(without[0].program, "x-terminal-emulator");
    }

    #[test]
    fn windows_tries_windows_terminal_then_cmd() {
        let launches = terminal_launches("windows", r"C:\work\p", None, &[]);
        assert_eq!(launches[0].program, "wt.exe");
        assert_eq!(launches[1].cwd.as_deref(), Some(r"C:\work\p"));
    }

    #[test]
    fn network_command_quotes_dir() {
        assert_eq!(
            network_logs_command(Some("/a b")),
            "cd '/a b' && sandbox network logs --status=all"
        );
        assert_eq!(
            network_logs_command(None),
            "sandbox network logs --status=all"
        );
    }

    #[tokio::test]
    async fn stop_all_stops_running_only() {
        let mock = MockAdapter::demo();
        let adapters: Vec<Arc<dyn RuntimeAdapter>> = vec![Arc::new(mock.clone())];
        let results = stop_all(&adapters, STOP_TIMEOUT).await;
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.ok));
        let running = mock
            .list_containers(&ContainerFilter::running())
            .await
            .unwrap();
        assert!(running.is_empty());
    }

    #[tokio::test]
    async fn project_path_comes_from_project_mount() {
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let path = project_path(&adapter, "sandbox-web-shop", false)
            .await
            .unwrap();
        assert_eq!(path.as_deref(), Some("/Users/dev/projects/web-shop"));
        assert!(
            project_path(&adapter, "nope", false)
                .await
                .unwrap()
                .is_none()
        );
    }
}
