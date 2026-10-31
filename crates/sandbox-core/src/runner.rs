//! Run a CLI on this machine or, through an SSH session, on a remote host.
//! Adapters and endpoint discovery go through this so the same code serves
//! both.

use std::process::Stdio;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone)]
pub enum CommandRunner {
    Local,
    #[cfg(test)]
    Fake(Arc<FakeRunner>),
}

impl CommandRunner {
    pub fn is_local(&self) -> bool {
        matches!(self, CommandRunner::Local)
    }

    pub async fn run(
        &self,
        binary: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput> {
        match self {
            CommandRunner::Local => run_local(binary, args, timeout).await,
            #[cfg(test)]
            CommandRunner::Fake(fake) => Ok(fake.answer(binary, args)),
        }
    }
}

async fn run_local(binary: &str, args: &[String], timeout: Duration) -> Result<CommandOutput> {
    let child = tokio::process::Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::RuntimeUnavailable {
            runtime: binary.to_string(),
            reason: format!("cannot start {binary}: {e}"),
        })?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| Error::Protocol(format!("{binary} {} timed out", args.join(" "))))??;
    Ok(CommandOutput {
        exit_code: output.status.code().map(i64::from).unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Canned answers keyed by `"<binary> <args>"`; unknown commands exit 127.
#[cfg(test)]
pub struct FakeRunner {
    responses: Vec<(String, CommandOutput)>,
    calls: Mutex<Vec<String>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn new(responses: Vec<(&str, CommandOutput)>) -> Self {
        Self {
            responses: responses
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            calls: Mutex::default(),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn answer(&self, binary: &str, args: &[String]) -> CommandOutput {
        let key = std::iter::once(binary.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        self.calls.lock().unwrap().push(key.clone());
        self.responses
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(CommandOutput {
                exit_code: 127,
                stdout: String::new(),
                stderr: format!("{binary}: not found"),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_runner_captures_output_and_exit_code() {
        let out = CommandRunner::Local
            .run(
                "sh",
                &["-c".into(), "echo hi; echo err >&2; exit 3".into()],
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        assert_eq!(out.exit_code, 3);
        assert_eq!(out.stdout, "hi\n");
        assert_eq!(out.stderr, "err\n");
    }

    #[tokio::test]
    async fn local_runner_times_out() {
        let err = CommandRunner::Local
            .run(
                "sh",
                &["-c".into(), "sleep 5".into()],
                Duration::from_millis(100),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn missing_binary_is_unavailable() {
        let err = CommandRunner::Local
            .run(
                "sandbox-watcher-no-such-binary",
                &[],
                Duration::from_secs(1),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::RuntimeUnavailable { .. }), "{err}");
    }
}
