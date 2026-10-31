//! One SSH master connection per remote host, through the system `ssh`.
//!
//! Authentication, users, ports and jump hosts come from the user's
//! `~/.ssh/config` and agent. The app never prompts and never accepts host
//! keys (`BatchMode=yes`). Sockets live in a private `0700` directory.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::host::validate_alias;
#[cfg(unix)]
use crate::runner::CommandOutput;
#[cfg(unix)]
use crate::{Error, Result};

#[cfg(unix)]
static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// Characters that never need quoting in a POSIX shell word.
fn is_safe(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',')
}

/// Quote one argument for a POSIX shell.
pub fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(is_safe) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// The command line the remote login shell receives.
pub fn remote_command(binary: &str, args: &[String]) -> String {
    std::iter::once(shell_quote(binary))
        .chain(args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What to tell the user about an ssh failure.
pub fn ssh_hint(stderr: &str, alias: &str) -> String {
    let text = stderr.trim();
    if text.contains("Host key verification failed") {
        format!("Add the host to known_hosts first: run `ssh {alias}` once in a terminal.")
    } else if text.contains("Permission denied") {
        "Key or agent authentication is required; the watcher cannot prompt for passwords.".into()
    } else if text.is_empty() {
        format!("ssh to {alias} failed without an error message.")
    } else {
        text.lines().last().unwrap_or(text).to_string()
    }
}

fn common_options() -> Vec<String> {
    [
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn master_args(control: &Path, alias: &str) -> Vec<String> {
    let mut args = vec![
        "-M".into(),
        "-N".into(),
        "-S".into(),
        control.display().to_string(),
    ];
    args.extend(common_options());
    args.extend([
        "-o".into(),
        "ControlPersist=no".into(),
        "-o".into(),
        "StreamLocalBindUnlink=yes".into(),
    ]);
    args.extend(["--".into(), alias.into()]);
    args
}

pub fn control_args(control: &Path, alias: &str, op: &str) -> Vec<String> {
    vec![
        "-S".into(),
        control.display().to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-O".into(),
        op.into(),
        "--".into(),
        alias.into(),
    ]
}

pub fn run_args(control: &Path, alias: &str, remote: &str) -> Vec<String> {
    let mut args = vec!["-S".into(), control.display().to_string()];
    args.extend(common_options());
    args.extend(["--".into(), alias.into(), remote.into()]);
    args
}

pub fn forward_args(control: &Path, alias: &str, local: &Path, remote: &str) -> Vec<String> {
    vec![
        "-S".into(),
        control.display().to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-O".into(),
        "forward".into(),
        "-L".into(),
        format!("{}:{remote}", local.display()),
        "--".into(),
        alias.into(),
    ]
}

/// A forwarded remote socket must be an absolute path; ':' would be parsed
/// as a `-L` field separator or turn the forward into a TCP one.
pub fn validate_remote_socket(path: &str) -> std::result::Result<(), String> {
    if !path.starts_with('/') {
        return Err(format!("remote socket path must be absolute: {path}"));
    }
    if path.contains(':') {
        return Err(format!("remote socket path must not contain ':': {path}"));
    }
    Ok(())
}

/// A short, private directory: Unix socket paths are limited to 104 bytes on macOS.
#[cfg(unix)]
fn private_dir() -> Result<PathBuf> {
    let name = format!(
        "sw-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    );
    let mut base = if cfg!(target_os = "macos") {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let text = base.to_string_lossy();
    // ssh expands '%' and '~' in socket paths and ':' breaks `-L`; the
    // longest forward socket must also fit sockaddr_un on macOS.
    if text.contains(['%', '~', ':']) || base.join(&name).join("f99.sock").as_os_str().len() >= 100
    {
        base = PathBuf::from("/tmp");
    }
    let dir = base.join(name);
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    Ok(dir)
}

#[cfg(unix)]
async fn ssh(args: &[String], timeout: Duration) -> Result<CommandOutput> {
    crate::runner::run_local("ssh", args, timeout).await
}

#[cfg(unix)]
pub struct SshSession {
    alias: String,
    dir: PathBuf,
    control: PathBuf,
    master: Mutex<Option<tokio::process::Child>>,
    forwards: AtomicUsize,
    closed: AtomicBool,
    last_stderr: Arc<Mutex<Option<String>>>,
}

#[cfg(unix)]
impl SshSession {
    pub async fn open(alias: &str, timeout: Duration) -> Result<SshSession> {
        validate_alias(alias).map_err(|reason| Error::RuntimeUnavailable {
            runtime: "ssh".into(),
            reason,
        })?;
        let dir = private_dir()?;
        let control = dir.join("c");
        let mut child = tokio::process::Command::new("ssh")
            .args(master_args(&control, alias))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                let _ = std::fs::remove_dir_all(&dir);
                Error::RuntimeUnavailable {
                    runtime: "ssh".into(),
                    reason: format!("cannot start ssh: {e}"),
                }
            })?;
        let session = SshSession {
            alias: alias.to_string(),
            dir,
            control,
            master: Mutex::new(None),
            forwards: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            last_stderr: Arc::new(Mutex::new(None)),
        };
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = pipe.read_to_string(&mut stderr).await;
                }
                return Err(Error::RuntimeUnavailable {
                    runtime: format!("ssh {alias}"),
                    reason: format!("{} ({status})", ssh_hint(&stderr, alias)),
                });
            }
            if session.check().await {
                // A long-lived master keeps writing to stderr; an undrained
                // pipe would block ssh once it fills.
                if let Some(pipe) = child.stderr.take() {
                    let last = Arc::clone(&session.last_stderr);
                    tokio::spawn(async move {
                        use tokio::io::AsyncBufReadExt;
                        let mut lines = tokio::io::BufReader::new(pipe).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if !line.trim().is_empty() {
                                *last.lock().expect("ssh stderr poisoned") = Some(line);
                            }
                        }
                    });
                }
                *session.master.lock().expect("ssh master poisoned") = Some(child);
                return Ok(session);
            }
            if Instant::now() >= deadline {
                let _ = child.start_kill();
                return Err(Error::RuntimeUnavailable {
                    runtime: format!("ssh {alias}"),
                    reason: format!("no connection within {}s", timeout.as_secs()),
                });
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// The last line the master wrote to stderr, for diagnostics.
    pub fn last_error(&self) -> Option<String> {
        self.last_stderr
            .lock()
            .expect("ssh stderr poisoned")
            .clone()
    }

    /// Whether the master connection is still up.
    pub async fn check(&self) -> bool {
        ssh(
            &control_args(&self.control, &self.alias, "check"),
            Duration::from_secs(5),
        )
        .await
        .map(|o| o.exit_code == 0)
        .unwrap_or(false)
    }

    pub async fn run(&self, remote_command: &str, timeout: Duration) -> Result<CommandOutput> {
        ssh(
            &run_args(&self.control, &self.alias, remote_command),
            timeout,
        )
        .await
    }

    /// Forward a remote Unix socket to a new local one and return its path.
    pub async fn forward(&self, remote_socket: &str) -> Result<PathBuf> {
        validate_remote_socket(remote_socket).map_err(|reason| Error::RuntimeUnavailable {
            runtime: format!("ssh {}", self.alias),
            reason,
        })?;
        let n = self.forwards.fetch_add(1, Ordering::Relaxed);
        let local = self.dir.join(format!("f{n}.sock"));
        let out = ssh(
            &forward_args(&self.control, &self.alias, &local, remote_socket),
            Duration::from_secs(10),
        )
        .await?;
        if out.exit_code != 0 {
            return Err(Error::RuntimeUnavailable {
                runtime: format!("ssh {}", self.alias),
                reason: format!(
                    "cannot forward {remote_socket}: {}",
                    ssh_hint(&out.stderr, &self.alias)
                ),
            });
        }
        Ok(local)
    }

    pub async fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        let _ = ssh(
            &control_args(&self.control, &self.alias, "exit"),
            Duration::from_secs(5),
        )
        .await;
        let child = self.master.lock().expect("ssh master poisoned").take();
        if let Some(mut child) = child {
            let _ = child.start_kill();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(unix)]
impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshSession")
            .field("alias", &self.alias)
            .finish()
    }
}

#[cfg(unix)]
impl Drop for SshSession {
    fn drop(&mut self) {
        if !self.closed.load(Ordering::Relaxed) && self.control.exists() {
            let _ = std::process::Command::new("ssh")
                .args(control_args(&self.control, &self.alias, "exit"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn quotes_for_the_remote_shell() {
        assert_eq!(shell_quote("plain-arg_1.2/x:y@z"), "plain-arg_1.2/x:y@z");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(
            shell_quote("$HOME;rm -rf /|`x`\n"),
            "'$HOME;rm -rf /|`x`\n'"
        );
    }

    /// `ssh` hands the command line to the remote login shell, which parses it
    /// once. `sh -c` here plays that shell.
    #[cfg(unix)]
    #[test]
    fn quoted_arguments_survive_the_remote_shell() {
        let args: Vec<String> = [
            "a b",
            "it's",
            "$HOME",
            "x;y",
            "`z`",
            "",
            "line\nbreak",
            "{}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut printf_args = vec!["%s\\0".to_string()];
        printf_args.extend(args.iter().cloned());
        let line = remote_command("printf", &printf_args);
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&line)
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        let got: Vec<String> = stdout
            .strip_suffix('\0')
            .unwrap()
            .split('\0')
            .map(str::to_string)
            .collect();
        assert_eq!(got, args);
    }

    #[test]
    fn builds_ssh_arguments_with_batch_mode_and_separator() {
        let control = Path::new("/tmp/sw-1-0/c");
        let master = master_args(control, "devbox");
        assert_eq!(master[..2], ["-M".to_string(), "-N".to_string()]);
        assert!(master.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(
            master
                .windows(2)
                .any(|w| w == ["-o", "StreamLocalBindUnlink=yes"])
        );
        assert_eq!(
            master[master.len() - 2..],
            ["--".to_string(), "devbox".to_string()]
        );

        let run = run_args(control, "devbox", "docker version");
        assert_eq!(
            run[run.len() - 3..],
            ["--".to_string(), "devbox".into(), "docker version".into()]
        );

        let fwd = forward_args(
            control,
            "devbox",
            Path::new("/tmp/sw-1-0/f0.sock"),
            "/var/run/docker.sock",
        );
        assert!(fwd.windows(2).any(|w| w == ["-O", "forward"]));
        assert!(
            fwd.windows(2)
                .any(|w| w == ["-L", "/tmp/sw-1-0/f0.sock:/var/run/docker.sock"])
        );

        let exit = control_args(control, "devbox", "exit");
        assert!(exit.windows(2).any(|w| w == ["-O", "exit"]));
        assert_eq!(exit.last().unwrap(), "devbox");
    }

    #[cfg(unix)]
    #[test]
    fn socket_paths_fit_the_macos_limit() {
        let dir = private_dir().unwrap();
        let longest = dir.join("f99.sock");
        assert!(longest.as_os_str().len() < 100, "{}", longest.display());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_unsafe_remote_sockets() {
        assert!(validate_remote_socket("/var/run/docker.sock").is_ok());
        assert!(validate_remote_socket("docker.sock").is_err());
        assert!(validate_remote_socket("").is_err());
        assert!(validate_remote_socket("/a:b").is_err());
        assert!(validate_remote_socket("/a:8080").is_err());
    }

    #[test]
    fn maps_ssh_errors_to_hints() {
        assert!(
            ssh_hint("Host key verification failed.", "devbox").contains("run `ssh devbox` once")
        );
        assert!(
            ssh_hint("devbox: Permission denied (publickey).", "devbox")
                .to_lowercase()
                .contains("key or agent")
        );
        assert!(
            ssh_hint("ssh: Could not resolve hostname devbox", "devbox")
                .contains("Could not resolve")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn open_rejects_invalid_aliases_without_running_ssh() {
        let err = SshSession::open("-oProxyCommand=touch /tmp/pwned", Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("must not start with '-'"), "{err}");
    }
}
