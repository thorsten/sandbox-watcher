//! Who is running inside a sandbox container.

use std::sync::Arc;

use crate::Result;
use crate::adapter::{ExecRequest, RuntimeAdapter};
use crate::model::{AgentKind, Readiness, Session};
use crate::sandbox::{parse_probe, status_probe_command};

const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "dash", "ash"];
const WRAPPERS: &[&str] = &[
    "node", "bun", "deno", "npx", "pnpx", "bunx", "python", "python3", "env", "exec", "timeout",
    "nice",
];

fn basename(token: &str) -> &str {
    token.rsplit(['/', '\\']).next().unwrap_or(token)
}

impl AgentKind {
    /// Classify a session by its command line.
    ///
    /// Looks at the program name first, then at script paths for the usual
    /// Node-based launchers (`node .../claude-code/cli.js`).
    pub fn classify(command: &str) -> AgentKind {
        let lower = command.to_ascii_lowercase();
        let mut tokens = lower
            .split_whitespace()
            .filter(|t| !t.contains('=') || t.starts_with('-'));
        let mut program = None;
        for token in tokens.by_ref() {
            let name = basename(token);
            if WRAPPERS.contains(&name) {
                continue;
            }
            program = Some(name.to_string());
            break;
        }
        match program.as_deref() {
            Some("claude") => return AgentKind::ClaudeCode,
            Some("codex") => return AgentKind::Codex,
            Some("opencode") => return AgentKind::OpenCode,
            Some("pi") => return AgentKind::Pi,
            Some(name)
                if SHELLS.contains(&name) || SHELLS.contains(&name.trim_start_matches('-')) =>
            {
                return AgentKind::Shell;
            }
            _ => {}
        }
        if lower.contains("claude-code") || lower.contains("@anthropic-ai/claude") {
            AgentKind::ClaudeCode
        } else if lower.contains("@openai/codex") || lower.contains("/codex/") {
            AgentKind::Codex
        } else if lower.contains("opencode") {
            AgentKind::OpenCode
        } else if lower.contains("@mariozechner/pi") || lower.contains("pi-coding-agent") {
            AgentKind::Pi
        } else {
            AgentKind::Other
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Pi => "Pi",
            AgentKind::Shell => "Shell",
            AgentKind::Other => "Other",
        }
    }
}

/// Readiness plus classified sessions of one container.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContainerProbe {
    pub readiness: Readiness,
    pub sessions: Vec<Session>,
}

/// Run the status probe inside a running container.
pub async fn probe_container(
    adapter: &Arc<dyn RuntimeAdapter>,
    container_id: &str,
) -> Result<ContainerProbe> {
    let output = adapter
        .exec(container_id, ExecRequest::new(status_probe_command()))
        .await?;
    if output.exit_code != 0 {
        return Ok(ContainerProbe::default());
    }
    let report = parse_probe(&output.stdout);
    Ok(ContainerProbe {
        readiness: report.readiness,
        sessions: report
            .sessions
            .into_iter()
            .map(|(pid, command)| Session {
                pid,
                agent: AgentKind::classify(&command),
                command,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_agents() {
        let cases = [
            ("claude", AgentKind::ClaudeCode),
            (
                "claude --dangerously-skip-permissions",
                AgentKind::ClaudeCode,
            ),
            ("/home/sandbox/.local/bin/claude", AgentKind::ClaudeCode),
            (
                "node /usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                AgentKind::ClaudeCode,
            ),
            ("codex --full-auto", AgentKind::Codex),
            (
                "node /usr/local/lib/node_modules/@openai/codex/bin/codex.js",
                AgentKind::Codex,
            ),
            ("opencode", AgentKind::OpenCode),
            ("bun /opt/opencode/dist/index.js", AgentKind::OpenCode),
            ("pi", AgentKind::Pi),
            (
                "node /usr/lib/node_modules/@mariozechner/pi-coding-agent/dist/cli.js",
                AgentKind::Pi,
            ),
            ("zsh -l", AgentKind::Shell),
            ("-zsh", AgentKind::Shell),
            ("/bin/bash", AgentKind::Shell),
            ("TERM=xterm env FOO=1 bash", AgentKind::Shell),
            ("npm test", AgentKind::Other),
            ("unknown", AgentKind::Other),
            ("", AgentKind::Other),
        ];
        for (command, expected) in cases {
            assert_eq!(AgentKind::classify(command), expected, "{command:?}");
        }
    }

    #[tokio::test]
    async fn probes_mock_container() {
        use crate::mock::MockAdapter;
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let probe = probe_container(&adapter, "sandbox-web-shop").await.unwrap();
        assert_eq!(probe.readiness, Readiness::Ready);
        assert_eq!(probe.sessions.len(), 2);
        assert_eq!(probe.sessions[0].agent, AgentKind::ClaudeCode);
        assert_eq!(probe.sessions[1].agent, AgentKind::Shell);
    }
}
