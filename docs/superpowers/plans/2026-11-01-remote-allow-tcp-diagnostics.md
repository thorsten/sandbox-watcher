# Remote allow list, TCP Docker contexts, connection diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the allow list work for remote containers, accept `tcp://` Docker endpoints on remote hosts, and add an on-demand diagnostics page that lists every runtime probe with its outcome.

**Architecture:** `ssh.rs` learns a second forward target (`host:port`) behind a local Unix socket, so the Docker adapter never changes. `RemoteSession` gains `run`, exposed up to the Tauri layer as `run_on_host`, and a new `allowlist_remote.rs` builds POSIX scripts that read and replace the remote `config.toml` under `flock` with a SHA-256 compare-and-swap, while the TOML edit stays local in `allowlist::inject_allowed_domains`. `detect.rs` and `live.rs` get traced variants that record every probe; `diagnostics.rs` assembles them with the remote host probes into a `DiagnosticsReport` rendered by a Settings section.

**Tech Stack:** Rust 2024 (tokio, serde, toml_edit, base64), Tauri 2, React 19 + TypeScript, Vitest with static rendering, GitHub Actions with sshd and socat on Linux.

**Spec:** `docs/superpowers/specs/2026-11-01-remote-allow-tcp-diagnostics-design.md`

## Global Constraints

- Every value interpolated into a remote shell command goes through `ssh::shell_quote`; file content travels base64-encoded. Scripts are built only in `sandbox-core`.
- Symlink rules of `allowlist::resolve` apply remotely: project target refuses a symlinked `.sandbox` dir or file; global target follows a symlinked file only to a regular file.
- The remote apply aborts with `changed` when the file's SHA-256 differs from the preview's.
- Only `https://` endpoints are rejected as TLS; plain `tcp://host:port` is forwarded.
- Diagnostics run on demand only. The refresh loop (`LiveRuntime::overview`) keeps calling `detect` and `connect` and must not slow down.
- The diagnostics report contains the values of `DOCKER_HOST`, `CONTAINER_HOST` and `SANDBOX_WATCHER_RUNTIME` and nothing else from the environment.
- Remote hosts stay unsupported from a Windows client; new `SshSession` code is `#[cfg(unix)]`.
- `src/types.ts` mirrors every serialised Rust type by hand (snake_case fields, kebab-case enums). After a type change: `SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-core --test contracts`, and for `AllowPreview` `SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-watcher contracts` on a machine that builds the shell, then `pnpm check:contracts`, commit fixtures.
- Checks before every commit: `cargo test -p sandbox-core`, `cargo clippy -p sandbox-core --all-targets -- -D warnings`, `cargo fmt --all --check`, `pnpm lint && pnpm format:check && pnpm test`. The pre-commit hook runs lint, format and contracts.
- Persisted text (code comments, docs, commit messages) in plain English, Conventional Commits.

## Review Focus

- A remote project root containing a space or a single quote (`/home/dev/my proj`, `/home/dev/it's`): preview and apply must resolve the right file. Test in Task 4 (`scripts_quote_every_value`).
- Preview on a remote host whose `sha256sum` is missing but `shasum` exists (macOS remote): the hash line must still be produced. Test in Task 4 (`hash_command_falls_back_to_shasum`) and Task 7 (conformance runs the real script).
- Apply when the remote file is missing at preview time and still missing at apply time: hash of empty content must match and the file must be created with the directory. Test in Task 7 (`remote_allow_creates_missing_global_config`).
- A `tcp://` endpoint with an IPv6 literal (`tcp://[::1]:2375`): must forward as `local.sock:[::1]:2375`, not split on the colons. Test in Task 1 (`forward_args_for_tcp_ipv6`).
- Diagnostics while a remote host is disabled: the host appears with state `disabled` and no connection attempt. Test in Task 9 (`disabled_hosts_are_listed_not_tested`).

---

### Task 1: `ForwardTarget` in `ssh.rs`

**Files:**

- Modify: `crates/sandbox-core/src/ssh.rs:134-160` (`forward_args`, `validate_remote_socket`), `:313-335` (`SshSession::forward`)
- Test: `crates/sandbox-core/src/ssh.rs` tests module

**Interfaces:**

- Produces: `pub enum ForwardTarget { UnixSocket(String), Tcp { host: String, port: u16 } }`, `ForwardTarget::parse(endpoint: &str) -> Result<ForwardTarget, String>`, `ForwardTarget::to_forward_spec(&self) -> String`, `pub fn forward_args(control: &Path, alias: &str, local: &Path, target: &ForwardTarget) -> Vec<String>`, `SshSession::forward(&self, target: &ForwardTarget) -> Result<PathBuf>`.
- `validate_remote_socket` is removed; its callers use `ForwardTarget::parse`.

- [ ] **Step 1: Write the failing tests**

Replace `rejects_unsafe_remote_sockets` in the tests module of `crates/sandbox-core/src/ssh.rs` with:

```rust
    #[test]
    fn parses_forward_targets() {
        use ForwardTarget::*;
        let ok = |s: &str| ForwardTarget::parse(s).unwrap();
        assert_eq!(ok("/var/run/docker.sock"), UnixSocket("/var/run/docker.sock".into()));
        assert_eq!(ok("unix:///run/podman/podman.sock"), UnixSocket("/run/podman/podman.sock".into()));
        assert_eq!(ok("tcp://127.0.0.1:2375"), Tcp { host: "127.0.0.1".into(), port: 2375 });
        assert_eq!(ok("tcp://docker.internal:2375"), Tcp { host: "docker.internal".into(), port: 2375 });
        assert_eq!(ok("tcp://[::1]:2375"), Tcp { host: "[::1]".into(), port: 2375 });
        assert_eq!(ok("tcp://host:2376"), Tcp { host: "host".into(), port: 2376 });
    }

    #[test]
    fn rejects_unsafe_forward_targets() {
        for bad in [
            "relative/path.sock",
            "/path:with:colons",
            "tcp://a b:2375",
            "tcp://host",
            "tcp://host:0",
            "tcp://host:70000",
            "tcp://:2375",
            "tcp://host:2375/path",
            "tcp://user@host:2375",
            "ssh://host",
            "",
        ] {
            assert!(ForwardTarget::parse(bad).is_err(), "{bad} should be rejected");
        }
        let err = ForwardTarget::parse("https://host:2376").unwrap_err();
        assert!(err.contains("TLS Docker endpoints are not supported"), "{err}");
    }

    #[test]
    fn forward_args_for_tcp_ipv6() {
        let target = ForwardTarget::parse("tcp://[::1]:2375").unwrap();
        let args = forward_args(Path::new("/tmp/c"), "box", Path::new("/tmp/f0.sock"), &target);
        let l = args.iter().position(|a| a == "-L").unwrap();
        assert_eq!(args[l + 1], "/tmp/f0.sock:[::1]:2375");
        assert_eq!(args.iter().filter(|a| *a == "-L").count(), 1);
    }

    #[test]
    fn forward_args_for_unix_socket_unchanged() {
        let target = ForwardTarget::parse("/var/run/docker.sock").unwrap();
        let args = forward_args(Path::new("/tmp/c"), "box", Path::new("/tmp/f0.sock"), &target);
        let l = args.iter().position(|a| a == "-L").unwrap();
        assert_eq!(args[l + 1], "/tmp/f0.sock:/var/run/docker.sock");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sandbox-core ssh::tests`
Expected: compile error, `ForwardTarget` not found.

- [ ] **Step 3: Implement**

In `crates/sandbox-core/src/ssh.rs` replace `forward_args` and `validate_remote_socket` with:

```rust
/// Where a forwarded local socket leads on the remote host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardTarget {
    /// Absolute path, no ':'.
    UnixSocket(String),
    /// Plain TCP as seen from the remote host. `host` keeps IPv6 brackets.
    Tcp { host: String, port: u16 },
}

impl ForwardTarget {
    /// Accepts `unix://<path>`, a bare absolute path, `tcp://<host>:<port>`
    /// and `tcp://[<v6>]:<port>`. Everything else is an error with a reason
    /// fit for a probe line.
    pub fn parse(endpoint: &str) -> std::result::Result<ForwardTarget, String> {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            return Err("empty endpoint".into());
        }
        if endpoint.starts_with("https://") {
            return Err(format!(
                "{endpoint}: TLS Docker endpoints are not supported"
            ));
        }
        if let Some(rest) = endpoint.strip_prefix("tcp://") {
            return parse_tcp(rest).map_err(|reason| format!("{endpoint}: {reason}"));
        }
        let path = endpoint.strip_prefix("unix://").unwrap_or(endpoint);
        if !path.starts_with('/') {
            return Err(format!("{endpoint}: not a Unix socket path or tcp:// endpoint"));
        }
        if path.contains(':') {
            return Err(format!("{endpoint}: socket path must not contain ':'"));
        }
        Ok(ForwardTarget::UnixSocket(path.to_string()))
    }

    /// The remote half of ssh's `-L` argument.
    pub fn to_forward_spec(&self) -> String {
        match self {
            ForwardTarget::UnixSocket(path) => path.clone(),
            ForwardTarget::Tcp { host, port } => format!("{host}:{port}"),
        }
    }
}

fn parse_tcp(rest: &str) -> std::result::Result<ForwardTarget, String> {
    let (host, port) = if let Some(after) = rest.strip_prefix('[') {
        let (v6, tail) = after
            .split_once(']')
            .ok_or("unterminated IPv6 literal")?;
        if v6.is_empty() || !v6.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
            return Err("invalid IPv6 literal".into());
        }
        let port = tail.strip_prefix(':').ok_or("missing port")?;
        (format!("[{v6}]"), port)
    } else {
        let (host, port) = rest.rsplit_once(':').ok_or("missing port")?;
        if host.is_empty() {
            return Err("missing host".into());
        }
        if host.len() > 253
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err("host may contain letters, digits, '.', '-' and '_' only".into());
        }
        (host.to_string(), port)
    };
    let port: u16 = port
        .parse()
        .ok()
        .filter(|p| *p > 0)
        .ok_or("port must be 1 to 65535")?;
    Ok(ForwardTarget::Tcp { host, port })
}

pub fn forward_args(
    control: &Path,
    alias: &str,
    local: &Path,
    target: &ForwardTarget,
) -> Vec<String> {
    let mut args = vec!["-S".into(), control.display().to_string()];
    args.extend(client_options());
    args.extend([
        "-O".into(),
        "forward".into(),
        "-L".into(),
        format!("{}:{}", local.display(), target.to_forward_spec()),
        "--".into(),
        alias.into(),
    ]);
    args
}
```

Change `SshSession::forward`:

```rust
    /// Forward a remote socket or TCP port to a new local Unix socket and
    /// return its path. OpenSSH accepts a Unix listener with a TCP destination.
    pub async fn forward(&self, target: &ForwardTarget) -> Result<PathBuf> {
        let n = self.forwards.fetch_add(1, Ordering::Relaxed);
        let local = self.dir.join(format!("f{n}.sock"));
        let out = ssh(
            &forward_args(&self.control, &self.alias, &local, target),
            Duration::from_secs(10),
        )
        .await?;
        if out.exit_code != 0 {
            return Err(Error::RuntimeUnavailable {
                runtime: format!("ssh {}", self.alias),
                reason: format!(
                    "cannot forward {}: {}",
                    target.to_forward_spec(),
                    ssh_hint(&out.stderr, &self.alias)
                ),
            });
        }
        Ok(local)
    }
```

Fix the existing `forward_args` test (around `:457`) to build a `ForwardTarget::UnixSocket` first. `remote.rs` will not compile yet; Task 2 fixes it. To keep this task green, make the minimal change in `remote.rs:182-207` now:

```rust
            RuntimeKind::Docker | RuntimeKind::Podman => {
                match crate::ssh::ForwardTarget::parse(&candidate.endpoint) {
                    Err(reason) => {
                        detection.probes.push(format!("{}: {reason}", candidate.kind));
                        None
                    }
                    Ok(target) => match session.forward(&target).await {
                        Ok(local) => match DockerAdapter::connect(
                            candidate.kind,
                            &local.display().to_string(),
                        ) {
                            Ok(a) => Some(Arc::new(a) as Arc<dyn RuntimeAdapter>),
                            Err(e) => {
                                detection.probes.push(format!("{}: {e}", candidate.kind));
                                None
                            }
                        },
                        Err(e) => {
                            detection.probes.push(format!("{}: {e}", candidate.kind));
                            None
                        }
                    },
                }
            }
```

- [ ] **Step 4: Run tests, clippy, fmt**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/src/ssh.rs crates/sandbox-core/src/remote.rs
git commit -m "feat(core): forward tcp:// Docker endpoints over ssh"
```

---

### Task 2: Remote detection reads the config like sandbox, docs and CI for TCP

**Files:**

- Modify: `crates/sandbox-core/src/remote.rs:62-70` (config read), `:249-344` tests
- Modify: `crates/sandbox-core/src/endpoints.rs:77-79` (doc comment)
- Modify: `README.md:144-145`, `docs/MILESTONES.md:246`
- Modify: `.github/workflows/ci.yml` core job, after "Docker conformance over SSH"
- Modify: `crates/sandbox-core/tests/conformance.rs:235-258`

**Interfaces:**

- Produces: `pub const REMOTE_CONFIG_PATH_SH: &str` in `remote.rs`, the shell expression for the sandbox config path on a host, reused by Task 4.

- [ ] **Step 1: Write the failing test**

In `remote.rs` tests, next to the existing config-pin test, add:

```rust
    #[tokio::test]
    async fn config_read_honours_sandbox_config_dir() {
        let fake = Arc::new(FakeRunner::default());
        fake.on("sh", |args| args[1].contains("command -v"), "/usr/bin/docker\n");
        fake.on("sh", |args| args[1].contains("config.toml"), "runtime = \"podman\"\n");
        let runner = CommandRunner::Fake(fake.clone());
        detect_remote(&runner).await;
        let script = fake.calls().into_iter().find(|c| c.contains("config.toml")).unwrap();
        assert!(script.contains("${SANDBOX_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/sandbox}/config.toml"), "{script}");
    }
```

Check how the existing tests in `remote.rs:249-344` drive `FakeRunner` (`crates/sandbox-core/src/runner.rs`, `FakeRunner::on` and `calls`) and match their helper names exactly; adapt the test if the helpers are named differently.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sandbox-core remote::tests::config_read_honours_sandbox_config_dir`
Expected: FAIL on the `contains` assertion.

- [ ] **Step 3: Implement**

In `remote.rs` add above `detect_remote`:

```rust
/// The sandbox config file on a host, as sandbox resolves it: an explicit
/// `SANDBOX_CONFIG_DIR`, else `$XDG_CONFIG_HOME` or `~/.config`.
pub const REMOTE_CONFIG_PATH_SH: &str =
    "${SANDBOX_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/sandbox}/config.toml";
```

and replace the `cat` script with:

```rust
    let config = sh(
        runner,
        &format!("cat \"{REMOTE_CONFIG_PATH_SH}\" 2>/dev/null; true"),
    )
```

In `endpoints.rs:77-79` change the doc comment to "TCP endpoints are forwarded by the caller; TLS ones are rejected there."

README `:144-145`: replace "Only Unix-socket endpoints are forwarded; TCP Docker contexts are not." with "Unix sockets and plain `tcp://` endpoints are forwarded; TLS Docker endpoints are not supported." `docs/MILESTONES.md:246`: same sentence.

- [ ] **Step 4: Conformance over a TCP endpoint**

In `tests/conformance.rs`, inside the `SANDBOX_WATCHER_SSH_HOST` branch before `connect_host`, nothing changes: the remote `docker context inspect` fallback reads `DOCKER_HOST` from the remote login environment. CI sets it through `~/.ssh/environment`. Add to `.github/workflows/ci.yml` after the "Docker conformance over SSH" step:

```yaml
- name: Docker conformance over SSH to a tcp:// endpoint
  if: runner.os == 'Linux'
  env:
    SANDBOX_WATCHER_DOCKER_TESTS: "1"
    SANDBOX_WATCHER_TEST_SLUG: watcher-test
    SANDBOX_WATCHER_SSH_HOST: localhost
  # socat exposes the Docker socket on plain TCP; sshd hands the remote
  # shell DOCKER_HOST through ~/.ssh/environment so detection picks it.
  run: |
    sudo apt-get install -y socat
    nohup socat TCP-LISTEN:2375,bind=127.0.0.1,fork,reuseaddr UNIX-CONNECT:/var/run/docker.sock >/tmp/socat.log 2>&1 &
    for i in $(seq 1 20); do (echo > /dev/tcp/127.0.0.1/2375) 2>/dev/null && break || sleep 0.5; done
    echo "PermitUserEnvironment yes" | sudo tee -a /etc/ssh/sshd_config
    sudo systemctl restart ssh
    echo "DOCKER_HOST=tcp://127.0.0.1:2375" > ~/.ssh/environment
    ssh -o BatchMode=yes localhost 'echo $DOCKER_HOST' | grep -q tcp://
    docker start sandbox-watcher-test
    for i in $(seq 1 20); do
      docker exec sandbox-watcher-test test -f /tmp/.sandbox-ready && break || sleep 0.5
    done
    cargo test -p sandbox-core --test conformance -- --nocapture
    rm ~/.ssh/environment
```

Place it before "Remove the test container". `endpoints::docker_host_from_context` runs `docker context inspect`, which reports `DOCKER_HOST` when set, so the remote candidate becomes `tcp://127.0.0.1:2375`. Add an assertion in the SSH branch of `real_runtime_adapter_conforms` after `connect_host`:

```rust
        if let Ok(expected) = std::env::var("SANDBOX_WATCHER_EXPECT_REMOTE_ENDPOINT") {
            assert!(
                detection.probes.iter().any(|p| p.contains(&expected)),
                "expected a probe mentioning {expected}, got {:?}",
                detection.probes
            );
        }
```

(bind `detection` instead of `_` in the destructuring) and set `SANDBOX_WATCHER_EXPECT_REMOTE_ENDPOINT: tcp://127.0.0.1:2375` in the new CI step's `env`.

- [ ] **Step 5: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings && cargo fmt --all --check && pnpm format:check && pnpm check:actions`
Expected: pass. The CI step itself is verified on push.

- [ ] **Step 6: Commit**

```bash
git add crates/sandbox-core/src/remote.rs crates/sandbox-core/src/endpoints.rs crates/sandbox-core/tests/conformance.rs .github/workflows/ci.yml README.md docs/MILESTONES.md
git commit -m "feat(core): tcp:// Docker endpoints on remote hosts, config path like sandbox"
```

---

### Task 3: Run commands on a connected host

**Files:**

- Modify: `crates/sandbox-core/src/remote_hosts.rs:34-56` (trait and impl), `:283-320` (`RemoteHosts` methods), tests `:689-720` (`FakeSession`)
- Modify: `crates/sandbox-core/src/live.rs` (new method on `LiveRuntime`)
- Modify: `src-tauri/src/watcher.rs:161` (new method on `Watcher`)

**Interfaces:**

- Produces:
  - `RemoteSession::run(&self, command: &str, timeout: Duration) -> BoxFuture<'_, crate::Result<CommandOutput>>`
  - `RemoteHosts::run_on(&self, alias: &str, command: &str, timeout: Duration) -> Result<CommandOutput, String>` (async)
  - `LiveRuntime::run_on_host(&self, alias: &str, command: &str, timeout: Duration) -> Result<CommandOutput, String>` (async)
  - `Watcher::run_on_host(&self, alias: &str, command: &str, timeout: Duration) -> Result<CommandOutput, String>` (async). In mock mode returns `Err("no remote shell in mock mode")`.

- [ ] **Step 1: Write the failing test**

In `remote_hosts.rs` tests add a scripted reply to `FakeSession`:

```rust
        /// Replies for `run`, popped front to back.
        replies: StdMutex<Vec<CommandOutput>>,
```

and the trait method:

```rust
        fn run(&self, _command: &str, _timeout: Duration) -> BoxFuture<'_, crate::Result<CommandOutput>> {
            let reply = self.replies.lock().unwrap();
            let next = if reply.is_empty() {
                CommandOutput { exit_code: 127, stdout: String::new(), stderr: "no reply scripted".into() }
            } else {
                reply[0].clone()
            };
            drop(reply);
            if !self.replies.lock().unwrap().is_empty() {
                self.replies.lock().unwrap().remove(0);
            }
            Box::pin(async move { Ok(next) })
        }
```

Then the test:

```rust
    #[tokio::test]
    async fn run_on_reaches_the_connected_session_only() {
        let script = Arc::new(Script::default());
        let hosts = RemoteHosts::with_connector(Some(script.connector()));
        hosts.set_hosts(vec![host("devbox")]);
        refresh_until_connected(&hosts).await; // the helper the existing tests use to drive refresh
        script.last_session().replies.lock().unwrap().push(CommandOutput {
            exit_code: 0,
            stdout: "hello\n".into(),
            stderr: String::new(),
        });
        let out = hosts.run_on("devbox", "echo hello", Duration::from_secs(5)).await.unwrap();
        assert_eq!(out.stdout, "hello\n");
        let err = hosts.run_on("other", "echo", Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(err, "other is not connected");
    }
```

Use whatever helper the existing tests use to drive one refresh with the gate released (look at the first test in the module and copy its sequence).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sandbox-core remote_hosts::tests::run_on_reaches_the_connected_session_only`
Expected: compile error, `run` missing in trait.

- [ ] **Step 3: Implement**

`remote_hosts.rs`, trait:

```rust
pub trait RemoteSession: Send + Sync {
    fn check(&self) -> BoxFuture<'_, bool>;
    fn close(&self) -> BoxFuture<'_, ()>;
    fn last_error(&self) -> Option<String>;
    /// Run a command line through the master connection.
    fn run(&self, command: &str, timeout: Duration) -> BoxFuture<'_, crate::Result<CommandOutput>>;
}

#[cfg(unix)]
impl RemoteSession for crate::ssh::SshSession {
    // existing three methods ...
    fn run(&self, command: &str, timeout: Duration) -> BoxFuture<'_, crate::Result<CommandOutput>> {
        let command = command.to_string();
        Box::pin(async move { crate::ssh::SshSession::run(self, &command, timeout).await })
    }
}
```

Add `use crate::runner::CommandOutput;` at the top. On `RemoteHosts`:

```rust
    /// Run a command on a connected host. Takes the connections lock, so it
    /// waits for a running refresh round; callers hold no other lock.
    pub async fn run_on(
        &self,
        alias: &str,
        command: &str,
        timeout: Duration,
    ) -> std::result::Result<CommandOutput, String> {
        let session = {
            let list = self.connections.lock().await;
            list.iter()
                .find(|c| c.config.alias == alias && c.is_connected())
                .and_then(|c| c.session.clone())
        };
        let session = session.ok_or_else(|| format!("{alias} is not connected"))?;
        session.run(command, timeout).await.map_err(|e| e.to_string())
    }
```

`live.rs`:

```rust
    /// Run a shell command line on a connected remote host.
    pub async fn run_on_host(
        &self,
        alias: &str,
        command: &str,
        timeout: Duration,
    ) -> std::result::Result<CommandOutput, String> {
        self.remotes.run_on(alias, command, timeout).await
    }
```

(`use crate::runner::CommandOutput;`). `watcher.rs`:

```rust
    /// Run a shell command line on a connected remote host.
    pub async fn run_on_host(
        &self,
        alias: &str,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandOutput, String> {
        match &self.mode {
            Mode::Mock(_) => Err("no remote shell in mock mode".into()),
            Mode::Live(live) => live.run_on_host(alias, command, timeout).await,
        }
    }
```

(`use sandbox_core::runner::CommandOutput;`).

- [ ] **Step 4: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings && cargo fmt --all --check && cargo check -p sandbox-watcher`
Expected: pass. (`cargo check -p sandbox-watcher` needs the Tauri system libraries; if unavailable locally, rely on CI's app job.)

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/src/remote_hosts.rs crates/sandbox-core/src/live.rs src-tauri/src/watcher.rs
git commit -m "feat(core): run commands on a connected remote host"
```

---

### Task 4: `allowlist_remote.rs`: scripts and reply parsing

**Files:**

- Create: `crates/sandbox-core/src/allowlist_remote.rs`
- Modify: `crates/sandbox-core/src/lib.rs` (add `pub mod allowlist_remote;`)
- Modify: `crates/sandbox-core/Cargo.toml` (add `base64 = "0.22"`, already in `Cargo.lock` transitively)

**Interfaces:**

- Consumes: `ssh::shell_quote`, `remote::REMOTE_CONFIG_PATH_SH`, `allowlist::ConfigTarget`.
- Produces:

  ```rust
  pub fn preview_script(target: ConfigTarget, project_root: Option<&str>) -> String;
  pub enum PreviewReply { Absent, Symlink { path: String, reason: String }, Content { path: String, hash: String, content: String } }
  pub fn parse_preview(output: &CommandOutput) -> Result<PreviewReply, String>;
  pub fn apply_script(target: ConfigTarget, project_root: Option<&str>, expected_hash: &str, new_content: &str) -> String;
  pub enum ApplyReply { Ok, Changed, Refused(String) }
  pub fn parse_apply(output: &CommandOutput) -> Result<ApplyReply, String>;
  pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
  ```

  A missing file is reported as `Content { hash: EMPTY_SHA256, content: "" }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::CommandOutput;

    fn out(stdout: &str) -> CommandOutput {
        CommandOutput { exit_code: 0, stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn scripts_quote_every_value() {
        let root = "/home/dev/it's a proj\nline2";
        let s = preview_script(ConfigTarget::Project, Some(root));
        assert!(s.contains("'/home/dev/it'\\''s a proj\nline2'"), "{s}");
        assert!(!s.contains("/home/dev/it's a proj/"), "unquoted root leaked: {s}");
        let a = apply_script(ConfigTarget::Project, Some(root), EMPTY_SHA256, "x = 1\n");
        assert!(a.contains("'/home/dev/it'\\''s a proj\nline2'"), "{a}");
        // Content is base64, so it never meets the shell.
        assert!(!a.contains("x = 1"), "{a}");
        assert!(a.contains("eCA9IDEK"), "{a}");
    }

    #[test]
    fn global_script_uses_sandbox_config_precedence() {
        let s = preview_script(ConfigTarget::Global, None);
        assert!(s.contains(crate::remote::REMOTE_CONFIG_PATH_SH), "{s}");
    }

    #[test]
    fn hash_command_falls_back_to_shasum() {
        let s = preview_script(ConfigTarget::Global, None);
        assert!(s.contains("sha256sum"), "{s}");
        assert!(s.contains("shasum -a 256"), "{s}");
    }

    #[test]
    fn parses_preview_replies() {
        assert!(matches!(parse_preview(&out("absent\n")).unwrap(), PreviewReply::Absent));
        match parse_preview(&out("symlink /p/.sandbox is a symbolic link\n")).unwrap() {
            PreviewReply::Symlink { path, reason } => {
                assert_eq!(path, "/p/.sandbox");
                assert!(reason.contains("symbolic link"));
            }
            other => panic!("{other:?}"),
        }
        match parse_preview(&out("path /h/.config/sandbox/config.toml\nmissing\n")).unwrap() {
            PreviewReply::Content { path, hash, content } => {
                assert_eq!(path, "/h/.config/sandbox/config.toml");
                assert_eq!(hash, EMPTY_SHA256);
                assert_eq!(content, "");
            }
            other => panic!("{other:?}"),
        }
        match parse_preview(&out("path /p/.sandbox/config.toml\nsha abc\n[x]\na = 1\n")).unwrap() {
            PreviewReply::Content { hash, content, .. } => {
                assert_eq!(hash, "abc");
                assert_eq!(content, "[x]\na = 1\n");
            }
            other => panic!("{other:?}"),
        }
        let failed = CommandOutput { exit_code: 1, stdout: String::new(), stderr: "sh: boom".into() };
        assert_eq!(parse_preview(&failed).unwrap_err(), "sh: boom");
        assert!(parse_preview(&out("garbage\n")).is_err());
    }

    #[test]
    fn parses_apply_replies() {
        assert!(matches!(parse_apply(&out("ok\n")).unwrap(), ApplyReply::Ok));
        assert!(matches!(parse_apply(&out("changed\n")).unwrap(), ApplyReply::Changed));
        match parse_apply(&out("refused /p/.sandbox is a symbolic link\n")).unwrap() {
            ApplyReply::Refused(r) => assert!(r.contains("symbolic link")),
            other => panic!("{other:?}"),
        }
        let failed = CommandOutput { exit_code: 1, stdout: String::new(), stderr: "mv: denied".into() };
        assert_eq!(parse_apply(&failed).unwrap_err(), "mv: denied");
    }

    #[test]
    fn scripts_run_under_local_sh() {
        // The real scripts against a temp dir; skipped where sh is missing.
        if std::process::Command::new("sh").arg("-c").arg("true").status().is_err() {
            return;
        }
        let dir = tempdir();
        std::fs::create_dir(dir.join(".sandbox")).unwrap();
        let root = dir.to_str().unwrap();
        let run = |script: &str| {
            let o = std::process::Command::new("sh").arg("-c").arg(script).output().unwrap();
            CommandOutput {
                exit_code: o.status.code().unwrap_or(-1) as i64,
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            }
        };
        let p = parse_preview(&run(&preview_script(ConfigTarget::Project, Some(root)))).unwrap();
        let PreviewReply::Content { hash, content, path } = p else { panic!("{p:?}") };
        assert_eq!(content, "");
        assert_eq!(hash, EMPTY_SHA256);
        assert!(path.ends_with("/.sandbox/config.toml"));

        let a = run(&apply_script(ConfigTarget::Project, Some(root), &hash, "allow_network = [\"a\"]\n"));
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        assert_eq!(std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap(), "allow_network = [\"a\"]\n");

        // Stale hash: refused, file untouched.
        let a = run(&apply_script(ConfigTarget::Project, Some(root), EMPTY_SHA256, "x = 1\n"));
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Changed), "{a:?}");
        assert_eq!(std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap(), "allow_network = [\"a\"]\n");

        // Symlinked .sandbox: preview says symlink, apply refuses.
        let other = tempdir();
        std::os::unix::fs::symlink(dir.join(".sandbox"), other.join(".sandbox")).unwrap();
        let p = parse_preview(&run(&preview_script(ConfigTarget::Project, Some(other.to_str().unwrap())))).unwrap();
        assert!(matches!(p, PreviewReply::Symlink { .. }), "{p:?}");
        let a = run(&apply_script(ConfigTarget::Project, Some(other.to_str().unwrap()), EMPTY_SHA256, "x = 1\n"));
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Refused(_)), "{a:?}");
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sw-allow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
```

Mark `scripts_run_under_local_sh` with `#[cfg(unix)]`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sandbox-core allowlist_remote`
Expected: compile error, module missing.

- [ ] **Step 3: Implement**

`crates/sandbox-core/Cargo.toml` dependencies: `base64 = "0.22"`. `lib.rs`: `pub mod allowlist_remote;`. The module:

```rust
//! The allow list on a remote host: POSIX scripts that read and replace the
//! sandbox config through the SSH master, so `allowlist` can keep editing the
//! TOML locally. The write is a compare-and-swap on the file's SHA-256 under
//! `flock`, which replaces the lock span `allowlist::apply` holds locally.
//!
//! Every interpolated value is quoted with `ssh::shell_quote`; the new file
//! content travels base64-encoded.

use base64::Engine;

use crate::allowlist::ConfigTarget;
use crate::remote::REMOTE_CONFIG_PATH_SH;
use crate::runner::CommandOutput;
use crate::ssh::shell_quote;

/// SHA-256 of the empty string: what a missing file hashes to.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// What the preview script found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewReply {
    /// Project target without a `.sandbox` directory.
    Absent,
    /// A symlink where the rules forbid one.
    Symlink { path: String, reason: String },
    /// The file, or an empty content with [`EMPTY_SHA256`] when it is missing.
    Content { path: String, hash: String, content: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyReply {
    Ok,
    /// The file changed since the preview; nothing was written.
    Changed,
    /// Path rules refused the write.
    Refused(String),
}

/// Shell functions shared by both scripts. `resolve` sets `f` (file) and
/// `d` (directory) or prints `absent` / `symlink <path> <reason>` and exits.
fn prelude(target: ConfigTarget, project_root: Option<&str>) -> String {
    let locate = match target {
        ConfigTarget::Global => format!("f=\"{REMOTE_CONFIG_PATH_SH}\"; d=$(dirname \"$f\")"),
        ConfigTarget::Project => {
            let root = shell_quote(project_root.unwrap_or(""));
            format!("d={root}/.sandbox; f=\"$d\"/config.toml")
        }
    };
    let rules = match target {
        ConfigTarget::Project => r#"
  [ -d "$d" ] || [ -L "$d" ] || { echo absent; exit 0; }
  [ -L "$d" ] && { echo "symlink $d is a symbolic link; refusing to write the project config"; exit 0; }
  [ -L "$f" ] && { echo "symlink $f is a symbolic link; refusing to write the project config"; exit 0; }
"#,
        ConfigTarget::Global => r#"
  if [ -L "$f" ]; then
    real=$(readlink -f "$f" 2>/dev/null) || real=
    [ -n "$real" ] && [ -f "$real" ] || { echo "symlink $f does not point to a regular file"; exit 0; }
    f=$real; d=$(dirname "$f")
  elif [ -e "$f" ] && [ ! -f "$f" ]; then
    echo "symlink $f is not a regular file"; exit 0
  fi
"#,
    };
    format!(
        r#"set -u
hash() {{ if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi; }}
{locate}
{rules}"#
    )
}

/// Print `path <f>` then either `missing` or `sha <hash>` and the content.
pub fn preview_script(target: ConfigTarget, project_root: Option<&str>) -> String {
    format!(
        r#"{}
echo "path $f"
if [ -e "$f" ]; then echo "sha $(hash < "$f")"; cat "$f"; else echo missing; fi
"#,
        prelude(target, project_root)
    )
}

/// Under a directory lock, re-hash the file, refuse with `changed` on a
/// mismatch, otherwise replace it atomically and print `ok`.
pub fn apply_script(
    target: ConfigTarget,
    project_root: Option<&str>,
    expected_hash: &str,
    new_content: &str,
) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(new_content);
    let expected = shell_quote(expected_hash);
    let encoded = shell_quote(&encoded);
    let mkdir = match target {
        ConfigTarget::Global => "mkdir -p \"$d\" || { echo \"refused cannot create $d\"; exit 0; }",
        ConfigTarget::Project => "",
    };
    format!(
        r#"{prelude}
{mkdir}
expected={expected}
write() {{
  if [ -e "$f" ]; then cur=$(hash < "$f"); else cur={empty}; fi
  [ "$cur" = "$expected" ] || {{ echo changed; return 0; }}
  tmp=$(mktemp "$d/.config.toml.XXXXXX") || {{ echo "refused cannot create a temporary file in $d"; return 0; }}
  if ! printf %s {encoded} | base64 -d > "$tmp"; then rm -f "$tmp"; echo "refused cannot write $tmp"; return 0; fi
  if [ -e "$f" ]; then
    chmod --reference="$f" "$tmp" 2>/dev/null || chmod "$(stat -c %a "$f" 2>/dev/null || stat -f %Lp "$f" 2>/dev/null)" "$tmp" 2>/dev/null || true
  fi
  if mv -f "$tmp" "$f"; then echo ok; else rm -f "$tmp"; echo "refused cannot replace $f"; fi
}}
if command -v flock >/dev/null 2>&1; then
  exec 9< "$d" && flock 9 && write
else
  lock="$d/.config.toml.lock"
  until mkdir "$lock" 2>/dev/null; do sleep 0.1; done
  write
  rmdir "$lock"
fi
"#,
        prelude = prelude(target, project_root),
        empty = EMPTY_SHA256,
    )
}

fn failed(output: &CommandOutput) -> Option<String> {
    (output.exit_code != 0).then(|| {
        let text = output.stderr.trim();
        if text.is_empty() {
            format!("remote script exited with {}", output.exit_code)
        } else {
            text.to_string()
        }
    })
}

pub fn parse_preview(output: &CommandOutput) -> std::result::Result<PreviewReply, String> {
    if let Some(e) = failed(output) {
        return Err(e);
    }
    let mut lines = output.stdout.splitn(3, '\n');
    let first = lines.next().unwrap_or("");
    if first == "absent" {
        return Ok(PreviewReply::Absent);
    }
    if let Some(rest) = first.strip_prefix("symlink ") {
        let (path, reason) = rest.split_once(' ').unwrap_or((rest, ""));
        return Ok(PreviewReply::Symlink { path: path.into(), reason: reason.into() });
    }
    let path = first
        .strip_prefix("path ")
        .ok_or_else(|| format!("unexpected preview reply: {first}"))?
        .to_string();
    let second = lines.next().unwrap_or("");
    if second == "missing" {
        return Ok(PreviewReply::Content { path, hash: EMPTY_SHA256.into(), content: String::new() });
    }
    let hash = second
        .strip_prefix("sha ")
        .ok_or_else(|| format!("unexpected preview reply: {second}"))?
        .to_string();
    Ok(PreviewReply::Content { path, hash, content: lines.next().unwrap_or("").to_string() })
}

pub fn parse_apply(output: &CommandOutput) -> std::result::Result<ApplyReply, String> {
    if let Some(e) = failed(output) {
        return Err(e);
    }
    let line = output.stdout.lines().last().unwrap_or("").trim();
    match line {
        "ok" => Ok(ApplyReply::Ok),
        "changed" => Ok(ApplyReply::Changed),
        _ => match line.strip_prefix("refused ").or_else(|| line.strip_prefix("symlink ")) {
            Some(reason) => Ok(ApplyReply::Refused(reason.to_string())),
            None if line == "absent" => Ok(ApplyReply::Refused("this project has no .sandbox directory".into())),
            None => Err(format!("unexpected apply reply: {line}")),
        },
    }
}
```

Note on `cat "$f"`: the content after `sha` is everything up to the end of stdout; `splitn(3, '\n')` hands it over untouched, including a missing trailing newline.

- [ ] **Step 4: Run checks**

Run: `cargo test -p sandbox-core allowlist_remote && cargo clippy -p sandbox-core --all-targets -- -D warnings && cargo fmt --all --check`
Expected: pass, including `scripts_run_under_local_sh`.

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/Cargo.toml Cargo.lock crates/sandbox-core/src/lib.rs crates/sandbox-core/src/allowlist_remote.rs
git commit -m "feat(core): remote allow list scripts with compare-and-swap"
```

---

### Task 5: Tauri `preview_allow` and `apply_allow` for remote containers

**Files:**

- Modify: `src-tauri/src/lib.rs:455-566` (`AllowPreview`, `preview_allow`, `apply_allow`)
- Modify: `src-tauri/src/contracts.rs:83-103` (`allow_preview` samples)
- Modify: `src/types.ts:438-444` (`AllowPreview`)
- Regenerate: `fixtures/contracts/AllowPreview.json`

**Interfaces:**

- Consumes: `Watcher::run_on_host`, `allowlist_remote::{preview_script, parse_preview, apply_script, parse_apply, PreviewReply, ApplyReply}`, `allowlist::{inject_allowed_domains, existing_entries, validate_entry}`.
- Produces: `AllowPreview` with new fields `global_hash: Option<String>`, `project_hash: Option<String>`, `global_unavailable: Option<String>`, `project_unavailable: Option<String>`. `apply_allow` gains the parameter `expected_hash: Option<String>` (ignored for local containers). Error string for a stale hash starts with `changed:`.

- [ ] **Step 1: Extend the contract samples (the failing test)**

In `src-tauri/src/contracts.rs` `allow_preview`, add the four fields to both samples: first sample `global_hash: Some("e3b0c442...".into())`, `project_hash: Some("9f86d081...".into())`, `global_unavailable: None`, `project_unavailable: None`; second sample `global_hash: None`, `project_hash: None`, `global_unavailable: Some("/Users/dev/.config/sandbox/config.toml does not point to a regular file".into())`, `project_unavailable: Some("this project has no .sandbox directory".into())`. Add a third sample where everything is `None`/empty except `global_path` set and `global_unavailable: None`.

- [ ] **Step 2: Implement**

`AllowPreview`:

```rust
#[derive(serde::Serialize)]
struct AllowPreview {
    entries: Vec<String>,
    global_path: Option<String>,
    project_path: Option<String>,
    already_in_global: Vec<String>,
    already_in_project: Vec<String>,
    /// SHA-256 of the previewed files (remote only); handed back to apply.
    global_hash: Option<String>,
    project_hash: Option<String>,
    /// Why a target cannot be written (symlink rules, missing directory).
    global_unavailable: Option<String>,
    project_unavailable: Option<String>,
}
```

Split `preview_allow` into the entry computation plus two branches:

```rust
const REMOTE_ALLOW_TIMEOUT: Duration = Duration::from_secs(15);

fn merged_entries(selections: &[AllowSelection]) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for sel in selections {
        for e in entries_for(&sel.host, &sel.ports.iter().copied().collect()) {
            if !entries.contains(&e) {
                entries.push(e);
            }
        }
    }
    entries
}

fn present_in(entries: &[String], content: &str) -> Vec<String> {
    let existing = existing_entries(content).unwrap_or_default();
    entries.iter().filter(|e| existing.contains(e)).cloned().collect()
}

/// One target on a remote host: path, hash and content, or why not.
struct RemoteTarget {
    path: Option<String>,
    hash: Option<String>,
    content: String,
    unavailable: Option<String>,
}

async fn remote_target(
    state: &State<'_, App>,
    alias: &str,
    target: ConfigTarget,
    root: Option<&str>,
) -> Result<RemoteTarget, String> {
    use sandbox_core::allowlist_remote::{PreviewReply, parse_preview, preview_script};
    if target == ConfigTarget::Project && root.is_none() {
        return Ok(RemoteTarget { path: None, hash: None, content: String::new(), unavailable: Some("no project directory".into()) });
    }
    let out = state.run_on_host(alias, &preview_script(target, root), REMOTE_ALLOW_TIMEOUT).await?;
    Ok(match parse_preview(&out)? {
        PreviewReply::Absent => RemoteTarget { path: None, hash: None, content: String::new(), unavailable: Some("this project has no .sandbox directory".into()) },
        PreviewReply::Symlink { path, reason } => RemoteTarget { path: Some(path), hash: None, content: String::new(), unavailable: Some(reason) },
        PreviewReply::Content { path, hash, content } => RemoteTarget { path: Some(path), hash: Some(hash), content, unavailable: None },
    })
}

#[tauri::command]
async fn preview_allow(
    state: State<'_, App>,
    container_key: String,
    selections: Vec<AllowSelection>,
) -> Result<AllowPreview, String> {
    let entries = merged_entries(&selections);
    let (host, _) = sandbox_core::host::parse_container_key(&container_key);
    let project_root = project_root_of(&state, &container_key).await?;
    match host {
        Host::Local => {
            let global = global_config_path();
            let project = project_root.filter(|r| project_config_available(r)).map(|r| project_config_path(&r));
            let present = |path: &Option<std::path::PathBuf>| -> Vec<String> {
                let Some(path) = path else { return vec![] };
                std::fs::read_to_string(path).map(|c| present_in(&entries, &c)).unwrap_or_default()
            };
            Ok(AllowPreview {
                already_in_global: present(&global),
                already_in_project: present(&project),
                entries,
                global_path: global.map(|p| p.display().to_string()),
                project_path: project.map(|p| p.display().to_string()),
                global_hash: None,
                project_hash: None,
                global_unavailable: None,
                project_unavailable: None,
            })
        }
        Host::Remote(alias) => {
            let root = project_root.as_ref().map(|p| p.to_string_lossy().into_owned());
            let global = remote_target(&state, &alias, ConfigTarget::Global, None).await?;
            let project = remote_target(&state, &alias, ConfigTarget::Project, root.as_deref()).await?;
            Ok(AllowPreview {
                already_in_global: present_in(&entries, &global.content),
                already_in_project: present_in(&entries, &project.content),
                entries,
                global_path: global.path,
                project_path: project.path,
                global_hash: global.hash,
                project_hash: project.hash,
                global_unavailable: global.unavailable,
                project_unavailable: project.unavailable,
            })
        }
    }
}
```

(`use sandbox_core::host::Host;`, `use std::time::Duration;`.) `apply_allow`:

```rust
#[tauri::command]
async fn apply_allow(
    state: State<'_, App>,
    container_key: String,
    target: ConfigTarget,
    entries: Vec<String>,
    expected_hash: Option<String>,
) -> Result<AllowResult, String> {
    let (host, _) = sandbox_core::host::parse_container_key(&container_key);
    let Host::Remote(alias) = host else {
        // existing local body, unchanged
        ...
    };
    use sandbox_core::allowlist::{inject_allowed_domains, validate_entry};
    use sandbox_core::allowlist_remote::{ApplyReply, PreviewReply, apply_script, parse_apply, parse_preview, preview_script};
    for e in &entries {
        validate_entry(e)?;
    }
    let expected = expected_hash.ok_or("preview is required before writing to a remote host")?;
    let root = project_root_of(&state, &container_key).await?.map(|p| p.to_string_lossy().into_owned());
    if target == ConfigTarget::Project && root.is_none() {
        return Err("no project directory".into());
    }
    // Re-read, so the content we edit is the one the hash describes.
    let out = state.run_on_host(&alias, &preview_script(target, root.as_deref()), REMOTE_ALLOW_TIMEOUT).await?;
    let (path, hash, content) = match parse_preview(&out)? {
        PreviewReply::Content { path, hash, content } => (path, hash, content),
        PreviewReply::Absent => return Err("this project has no .sandbox directory".into()),
        PreviewReply::Symlink { path, reason } => return Err(format!("{path}: {reason}")),
    };
    if hash != expected {
        return Err(format!("changed: the config on {alias} changed since the preview"));
    }
    let existing = existing_entries(&content)?;
    let (already_present, to_add): (Vec<String>, Vec<String>) =
        entries.into_iter().partition(|e| existing.contains(e));
    if to_add.is_empty() {
        return Ok(AllowResult { path, added: vec![], already_present });
    }
    let new_content = inject_allowed_domains(&content, &to_add)?;
    let out = state.run_on_host(&alias, &apply_script(target, root.as_deref(), &hash, &new_content), REMOTE_ALLOW_TIMEOUT).await?;
    match parse_apply(&out)? {
        ApplyReply::Ok => Ok(AllowResult { path, added: to_add, already_present }),
        ApplyReply::Changed => Err(format!("changed: the config on {alias} changed since the preview")),
        ApplyReply::Refused(reason) => Err(format!("{path}: {reason}")),
    }
}
```

Keep the duplicate removal that the local `apply` does (`entries` deduplicated in order) by deduplicating `to_add` before injecting. `src/types.ts`:

```ts
export interface AllowPreview {
  entries: string[];
  global_path: string | null;
  project_path: string | null;
  already_in_global: string[];
  already_in_project: string[];
  /** SHA-256 of the previewed file on a remote host; null for local containers. */
  global_hash: string | null;
  project_hash: string | null;
  /** Why the target cannot be written, or null when it can. */
  global_unavailable: string | null;
  project_unavailable: string | null;
}
```

`src/api.ts` `applyAllow` gains `expectedHash: string | null` as fourth argument, passed as `expectedHash` (Tauri converts to `expected_hash`).

- [ ] **Step 3: Regenerate fixtures and check**

Run: `SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-watcher contracts && pnpm check:contracts && cargo clippy -p sandbox-watcher --all-targets -- -D warnings && pnpm build`
Expected: fixture rewritten, contracts ok, `tsc` clean. (The shell contract test needs the Tauri system libraries; if they are not available locally, write the fixture by hand matching the three samples and let CI's app job confirm.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/contracts.rs src/types.ts src/api.ts fixtures/contracts/AllowPreview.json
git commit -m "feat: allow list for containers on remote hosts"
```

---

### Task 6: UI: allow list for remote containers

**Files:**

- Modify: `src/components/NetworkPanel.tsx:39,85,124` (drop the `remote` gate)
- Modify: `src/components/AllowDialog.tsx`
- Modify: `src/components/__tests__/NetworkPanel.test.tsx`, `src/components/__tests__/RemoteHosts.test.tsx`
- Modify: `README.md:146-147`

- [ ] **Step 1: Write the failing tests**

In `src/components/__tests__/RemoteHosts.test.tsx` add:

```tsx
it("offers the allow list for remote containers", () => {
  const report = networkReport({
    entries: [blockedEntry({ destination: "api.example.com", port: 443 })],
  });
  const { html } = render(
    <NetworkPanel containerKey="devbox/b" report={report} now={NOW} onChanged={noop} />,
  );
  expect(html).toContain('aria-label="Select api.example.com"');
});
```

Use the fixture helpers in `src/test/fixtures.ts` for a report and a blocked entry; if none exist, add `networkReport` and `blockedEntry` there following the pattern of `container()`. In `NetworkPanel.test.tsx`, if a test asserts the checkbox is hidden for remote keys, invert it.

Add an `AllowDialog` test file `src/components/__tests__/AllowDialog.test.tsx`:

```tsx
import { describe, expect, it, vi } from "vitest";
import { render } from "../../test/render";
import { AllowDialog } from "../AllowDialog";

describe("AllowDialog", () => {
  it("renders before the preview arrives", () => {
    const { text } = render(
      <AllowDialog containerKey="devbox/b" selections={[]} onClose={vi.fn()} />,
    );
    expect(text).toContain("Checking configuration");
  });
});
```

Static rendering cannot await the preview; the reason rendering is covered by extracting a pure component. Add to `AllowDialog.tsx` an exported `TargetChoice` component and test it:

```tsx
it("shows why a target is unavailable and disables it", () => {
  const { html, text } = render(
    <TargetChoice
      target="project"
      selected="global"
      path="/home/dev/p/.sandbox/config.toml"
      unavailable="/home/dev/p/.sandbox is a symbolic link; refusing to write the project config"
      onSelect={vi.fn()}
    />,
  );
  expect(html).toContain("disabled");
  expect(text).toContain("is a symbolic link");
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `pnpm test -- AllowDialog RemoteHosts`
Expected: FAIL (`TargetChoice` not exported; checkbox missing for remote key).

- [ ] **Step 3: Implement**

`NetworkPanel.tsx`: delete `const remote = containerKey.includes("/");` and the two `!remote &&` guards. `AllowDialog.tsx`:

```tsx
export function TargetChoice({
  target,
  selected,
  path,
  unavailable,
  onSelect,
}: {
  target: ConfigTarget;
  selected: ConfigTarget;
  path: string | null;
  unavailable: string | null;
  onSelect: (t: ConfigTarget) => void;
}) {
  const label = target === "global" ? "Global config" : "Project config";
  const fallback = target === "global" ? "not available" : "no .sandbox directory in this project";
  return (
    <label className="check">
      <input
        type="radio"
        name="target"
        checked={selected === target}
        disabled={!path || unavailable !== null}
        onChange={() => onSelect(target)}
      />
      {label} <code className="muted">{path ?? fallback}</code>
      {unavailable && <span className="muted small"> {unavailable}</span>}
    </label>
  );
}
```

Replace the two radio labels with `<TargetChoice target="global" ... />` and `<TargetChoice target="project" ... />`, passing `preview.global_unavailable` and `preview.project_unavailable`. Default target: `project` when global is unusable (`!p.global_path || p.global_unavailable`) and project is usable. `apply` passes the hash:

```tsx
const hash = target === "global" ? preview?.global_hash : preview?.project_hash;
setResult(await applyAllow(containerKey, target, toAdd, hash ?? null));
```

and on an error message starting with `changed:` reload the preview instead of showing the raw error:

```tsx
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (message.startsWith("changed:")) {
        setError(`${message.slice("changed:".length).trim()}. The preview was refreshed; check the entries and try again.`);
        setPreview(await previewAllow(containerKey, selections));
      } else {
        setError(message);
      }
    }
```

The Add button is also disabled when the chosen target has `unavailable`. Show the alias in the heading for remote keys: `Allow blocked destinations on <alias>` where `alias = containerKey.includes("/") ? containerKey.split("/")[0] : null`.

README `:146-147`: "Remote containers can be watched and stopped, and their allow list edited. Terminal, folder, editor and reveal are not available for them."

- [ ] **Step 4: Run checks**

Run: `pnpm lint && pnpm format:check && pnpm test && pnpm build`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/components/NetworkPanel.tsx src/components/AllowDialog.tsx src/components/__tests__ src/test/fixtures.ts README.md
git commit -m "feat(ui): allow list dialog for remote containers"
```

---

### Task 7: Conformance: remote allow list over SSH in CI

**Files:**

- Modify: `crates/sandbox-core/tests/conformance.rs` (new test)
- Modify: `.github/workflows/ci.yml` (env for the new test in the "Docker conformance over SSH" step)

- [ ] **Step 1: Write the test**

```rust
/// The remote allow list scripts against a real sshd, compared with the
/// local writer on the same input.
#[cfg(unix)]
#[tokio::test]
async fn remote_allow_list_matches_local_apply() {
    let Ok(alias) = std::env::var("SANDBOX_WATCHER_SSH_HOST") else {
        eprintln!("skipped: set SANDBOX_WATCHER_SSH_HOST");
        return;
    };
    use sandbox_core::allowlist::{ConfigTarget, apply, inject_allowed_domains};
    use sandbox_core::allowlist_remote::{ApplyReply, PreviewReply, apply_script, parse_apply, parse_preview, preview_script};
    use sandbox_core::ssh::SshSession;

    let session = SshSession::open(&alias, Duration::from_secs(20)).await.expect("ssh");
    let run = |script: String| async { session.run(&script, Duration::from_secs(15)).await.expect("run") };

    // localhost: the remote filesystem is ours, so a temp dir works for both sides.
    let dir = std::env::temp_dir().join(format!("sw-remote-allow-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".sandbox")).unwrap();
    std::fs::write(dir.join(".sandbox/config.toml"), "allow_network = [\"a.example\"]\n").unwrap();
    let root = dir.to_str().unwrap();

    let p = parse_preview(&run(preview_script(ConfigTarget::Project, Some(root))).await).unwrap();
    let PreviewReply::Content { hash, content, path } = p else { panic!("{p:?}") };
    assert_eq!(path, dir.join(".sandbox/config.toml").to_str().unwrap());

    let entries = vec!["b.example".to_string(), "c.example:8443".to_string()];
    let new_content = inject_allowed_domains(&content, &entries).unwrap();
    let a = parse_apply(&run(apply_script(ConfigTarget::Project, Some(root), &hash, &new_content)).await).unwrap();
    assert_eq!(a, ApplyReply::Ok);
    let remote_written = std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap();

    // Same input through the local writer.
    let local = std::env::temp_dir().join(format!("sw-local-allow-{}", std::process::id()));
    std::fs::create_dir_all(local.join(".sandbox")).unwrap();
    std::fs::write(local.join(".sandbox/config.toml"), "allow_network = [\"a.example\"]\n").unwrap();
    apply(&local.join(".sandbox/config.toml"), ConfigTarget::Project, &entries).unwrap();
    assert_eq!(remote_written, std::fs::read_to_string(local.join(".sandbox/config.toml")).unwrap());

    // Stale hash is refused and the file is untouched.
    let a = parse_apply(&run(apply_script(ConfigTarget::Project, Some(root), &hash, "x = 1\n")).await).unwrap();
    assert_eq!(a, ApplyReply::Changed);
    assert_eq!(std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap(), remote_written);

    // Symlinked .sandbox is refused.
    let linked = std::env::temp_dir().join(format!("sw-linked-allow-{}", std::process::id()));
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(dir.join(".sandbox"), linked.join(".sandbox")).unwrap();
    let p = parse_preview(&run(preview_script(ConfigTarget::Project, Some(linked.to_str().unwrap()))).await).unwrap();
    assert!(matches!(p, PreviewReply::Symlink { .. }), "{p:?}");

    session.close().await;
    for d in [dir, local, linked] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// A missing global config is created with its directory.
#[cfg(unix)]
#[tokio::test]
async fn remote_allow_creates_missing_global_config() {
    let Ok(alias) = std::env::var("SANDBOX_WATCHER_SSH_HOST") else { return; };
    use sandbox_core::allowlist::ConfigTarget;
    use sandbox_core::allowlist_remote::{ApplyReply, EMPTY_SHA256, PreviewReply, apply_script, parse_apply, parse_preview, preview_script};
    use sandbox_core::ssh::SshSession;
    let session = SshSession::open(&alias, Duration::from_secs(20)).await.expect("ssh");
    let dir = std::env::temp_dir().join(format!("sw-global-allow-{}", std::process::id()));
    // SANDBOX_CONFIG_DIR is read on the remote side; prefix the script.
    let with_env = |s: String| format!("SANDBOX_CONFIG_DIR={} ; export SANDBOX_CONFIG_DIR; {s}", sandbox_core::ssh::shell_quote(dir.to_str().unwrap()));
    let p = parse_preview(&session.run(&with_env(preview_script(ConfigTarget::Global, None)), Duration::from_secs(15)).await.unwrap()).unwrap();
    let PreviewReply::Content { hash, content, .. } = p else { panic!("{p:?}") };
    assert_eq!(hash, EMPTY_SHA256);
    assert_eq!(content, "");
    let a = parse_apply(&session.run(&with_env(apply_script(ConfigTarget::Global, None, &hash, "allow_network = [\"x.example\"]\n")), Duration::from_secs(15)).await.unwrap()).unwrap();
    assert_eq!(a, ApplyReply::Ok);
    assert_eq!(std::fs::read_to_string(dir.join("config.toml")).unwrap(), "allow_network = [\"x.example\"]\n");
    session.close().await;
    let _ = std::fs::remove_dir_all(dir);
}
```

Mind that the real ssh login environment is what the scripts see; the `with_env` prefix is how the test controls `SANDBOX_CONFIG_DIR` without touching sshd config.

- [ ] **Step 2: Run locally (skips without the env), then in CI**

Run: `cargo test -p sandbox-core --test conformance`
Expected: both new tests print "skipped" and pass. CI's "Docker conformance over SSH" step already runs the whole `conformance` test binary with `SANDBOX_WATCHER_SSH_HOST=localhost`, so no workflow change is needed.

- [ ] **Step 3: Commit**

```bash
git add crates/sandbox-core/tests/conformance.rs
git commit -m "test(core): remote allow list conformance over ssh"
```

---

### Task 8: Traced local detection

**Files:**

- Create: `crates/sandbox-core/src/diagnostics.rs` (types only in this task)
- Modify: `crates/sandbox-core/src/detect.rs:70-198`
- Modify: `crates/sandbox-core/src/lib.rs` (`pub mod diagnostics;`)

**Interfaces:**

- Produces in `diagnostics.rs`:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "kebab-case", tag = "outcome")]
  pub enum ProbeOutcome { Found { endpoint: String }, Missing, Skipped { reason: String }, Failed { reason: String } }
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Probe { pub kind: RuntimeKind, pub candidate: String, #[serde(flatten)] pub outcome: ProbeOutcome }
  ```
- Produces in `detect.rs`: `pub fn detect_traced(env: &Environment<'_>) -> (Vec<DetectedRuntime>, Vec<Probe>)`; `detect` keeps its signature and returns `detect_traced(env).0`.

- [ ] **Step 1: Write the failing tests**

In `detect.rs` tests:

```rust
    #[test]
    fn trace_lists_every_candidate_in_order() {
        use crate::diagnostics::ProbeOutcome;
        let exists = |p: &Path| p == Path::new("/home/me/.docker/desktop/docker.sock");
        let on_path = |b: &str| b == "podman";
        let (found, trace) = detect_traced(&env("linux", None, &exists, &on_path));
        assert_eq!(found, detect(&env("linux", None, &exists, &on_path)));
        let names: Vec<&str> = trace.iter().map(|p| p.candidate.as_str()).collect();
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
        assert!(matches!(trace[7].outcome, ProbeOutcome::Skipped { .. }), "{:?}", trace[7]);
        assert_eq!(trace.iter().filter(|p| matches!(p.outcome, ProbeOutcome::Missing)).count(), 5);
    }

    #[test]
    fn trace_stops_a_kind_at_its_first_hit() {
        let yes = |_: &Path| true;
        let all = |_: &str| true;
        let (_, trace) = detect_traced(&env("macos", Some("tcp://x:1"), &yes, &all));
        let docker: Vec<_> = trace.iter().filter(|p| p.kind == RuntimeKind::Docker).collect();
        assert_eq!(docker.len(), 1);
        assert_eq!(docker[0].candidate, "DOCKER_HOST");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sandbox-core detect::tests`
Expected: compile error.

- [ ] **Step 3: Implement**

`diagnostics.rs` with the two types above plus `use crate::model::RuntimeKind; use serde::{Deserialize, Serialize};`. `detect.rs`: refactor each `detect_*` to push probes:

```rust
use crate::diagnostics::{Probe, ProbeOutcome};

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

fn hit(trace: &mut Vec<Probe>, kind: RuntimeKind, candidate: &str, endpoint: &str, source: &str) -> Option<DetectedRuntime> {
    trace.push(Probe { kind, candidate: candidate.into(), outcome: ProbeOutcome::Found { endpoint: endpoint.into() } });
    Some(DetectedRuntime { kind, endpoint: endpoint.into(), source: source.into() })
}

fn miss(trace: &mut Vec<Probe>, kind: RuntimeKind, candidate: &str) {
    trace.push(Probe { kind, candidate: candidate.into(), outcome: ProbeOutcome::Missing });
}

fn detect_docker(env: &Environment<'_>, trace: &mut Vec<Probe>) -> Option<DetectedRuntime> {
    let kind = RuntimeKind::Docker;
    if let Some(host) = &env.docker_host {
        return hit(trace, kind, "DOCKER_HOST", host, "DOCKER_HOST");
    }
    miss(trace, kind, "DOCKER_HOST");
    let candidates = /* unchanged list */;
    for (path, source) in candidates {
        let shown = path.to_string_lossy().into_owned();
        if (env.path_exists)(&path) {
            return hit(trace, kind, &shown, &shown, source);
        }
        miss(trace, kind, &shown);
    }
    if (env.binary_on_path)("docker") {
        return hit(trace, kind, "docker on PATH", "docker", "docker binary on PATH, no socket found");
    }
    miss(trace, kind, "docker on PATH");
    None
}
```

Same shape for `detect_podman` (`CONTAINER_HOST`, sockets, `podman on PATH`) and for Apple:

```rust
fn detect_apple_container(env: &Environment<'_>, trace: &mut Vec<Probe>) -> Option<DetectedRuntime> {
    let kind = RuntimeKind::AppleContainer;
    if env.os != "macos" {
        trace.push(Probe { kind, candidate: "container on PATH".into(), outcome: ProbeOutcome::Skipped { reason: format!("Apple container exists on macOS only, this is {}", env.os) } });
        return None;
    }
    if (env.binary_on_path)("container") {
        return hit(trace, kind, "container on PATH", "container", "container binary on PATH");
    }
    miss(trace, kind, "container on PATH");
    None
}
```

The existing tests keep passing because `detect`'s results are unchanged.

- [ ] **Step 4: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings && cargo fmt --all --check`

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/src/diagnostics.rs crates/sandbox-core/src/detect.rs crates/sandbox-core/src/lib.rs
git commit -m "feat(core): record every detection probe"
```

---

### Task 9: Traced connect and the diagnostics report

**Files:**

- Modify: `crates/sandbox-core/src/live.rs:103-114` (`connect`), `endpoints.rs:103-111` (`resolve` reason)
- Modify: `crates/sandbox-core/src/diagnostics.rs` (report types, `run`, `render_text`)
- Modify: `crates/sandbox-core/src/mock.rs` (`demo_diagnostics()`)
- Modify: `crates/sandbox-core/src/remote_hosts.rs` (public `test_remote_host` stays; add `pub fn host_statuses_for(hosts: &[RemoteHost])` helper is not needed, see below)

**Interfaces:**

- Produces in `endpoints.rs`: `pub async fn resolve_traced(candidate: &DetectedRuntime) -> Result<(String, String), String>` returning `(endpoint, how)` such as `("unix:///Users/me/.colima/default/docker.sock", "docker context inspect")`, or the reason. `resolve` wraps it.
- Produces in `live.rs`: `pub async fn connect_traced(candidate: &DetectedRuntime) -> (Option<Arc<dyn RuntimeAdapter>>, Vec<Probe>)`; `connect` wraps it.
- Produces in `diagnostics.rs`:
  ```rust
  pub struct BinaryCheck { pub name: String, pub found: bool }
  pub struct EnvironmentReport { pub os: String, pub docker_host: Option<String>, pub container_host: Option<String>, pub runtime_override: Option<String>, pub config_runtime: Option<RuntimeKind>, pub config_path: String, pub binaries: Vec<BinaryCheck> }
  pub struct HostReport { pub alias: String, pub status: HostStatus }
  pub struct DiagnosticsReport { pub generated_at: DateTime<Utc>, pub app_version: String, pub environment: EnvironmentReport, pub local: Vec<Probe>, pub hosts: Vec<HostReport> }
  pub async fn run(app_version: &str, preferred: Option<RuntimeKind>, hosts: &[RemoteHost]) -> DiagnosticsReport;
  pub fn render_text(report: &DiagnosticsReport) -> String;
  ```
  All report types derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`.
- Produces in `mock.rs`: `pub fn demo_diagnostics(app_version: &str) -> DiagnosticsReport`.

- [ ] **Step 1: Write the failing tests**

In `diagnostics.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{HostState, HostStatus};

    fn sample() -> DiagnosticsReport {
        DiagnosticsReport {
            generated_at: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap(),
            app_version: "0.8.0".into(),
            environment: EnvironmentReport {
                os: "linux".into(),
                docker_host: Some("tcp://127.0.0.1:2375".into()),
                container_host: None,
                runtime_override: None,
                config_runtime: Some(RuntimeKind::Docker),
                config_path: "/home/me/.config/sandbox/config.toml".into(),
                binaries: vec![
                    BinaryCheck { name: "docker".into(), found: true },
                    BinaryCheck { name: "podman".into(), found: false },
                    BinaryCheck { name: "container".into(), found: false },
                ],
            },
            local: vec![
                Probe { kind: RuntimeKind::Docker, candidate: "DOCKER_HOST".into(), outcome: ProbeOutcome::Found { endpoint: "tcp://127.0.0.1:2375".into() } },
                Probe { kind: RuntimeKind::Docker, candidate: "connect tcp://127.0.0.1:2375".into(), outcome: ProbeOutcome::Failed { reason: "connection refused".into() } },
                Probe { kind: RuntimeKind::Podman, candidate: "CONTAINER_HOST".into(), outcome: ProbeOutcome::Missing },
                Probe { kind: RuntimeKind::Podman, candidate: "podman on PATH".into(), outcome: ProbeOutcome::Skipped { reason: "pinned to docker by sandbox config".into() } },
            ],
            hosts: vec![HostReport {
                alias: "devbox".into(),
                status: HostStatus {
                    alias: "devbox".into(),
                    display_name: "Dev VM".into(),
                    state: HostState::Unreachable { reason: "Permission denied".into(), since: chrono::Utc.with_ymd_and_hms(2026, 11, 1, 11, 0, 0).unwrap() },
                    runtimes: vec![],
                    probes: vec!["docker: not found on PATH".into()],
                },
            }],
        }
    }

    #[test]
    fn text_report_has_every_section_and_outcome() {
        let text = render_text(&sample());
        let expected = "\
Sandbox Watcher 0.8.0 diagnostics, 2026-11-01 12:00:00 UTC

Environment
  OS: linux
  DOCKER_HOST: tcp://127.0.0.1:2375
  CONTAINER_HOST: (not set)
  SANDBOX_WATCHER_RUNTIME: (not set)
  sandbox config: /home/me/.config/sandbox/config.toml, runtime = docker
  on PATH: docker yes, podman no, container no

Local runtimes
  docker  DOCKER_HOST                    found  tcp://127.0.0.1:2375
  docker  connect tcp://127.0.0.1:2375   failed connection refused
  podman  CONTAINER_HOST                 missing
  podman  podman on PATH                 skipped pinned to docker by sandbox config

Remote host devbox (Dev VM)
  state: unreachable since 2026-11-01 11:00:00 UTC: Permission denied
  docker: not found on PATH
";
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn disabled_hosts_are_listed_not_tested() {
        let hosts = vec![crate::settings::RemoteHost { alias: "off".into(), display_name: None, enabled: false }];
        let report = run("0.8.0", None, &hosts).await;
        assert_eq!(report.hosts.len(), 1);
        assert_eq!(report.hosts[0].status.state, HostState::Disabled);
        assert!(report.hosts[0].status.probes.is_empty());
    }
}
```

The column widths in the text come from padding `kind` to 7 and `candidate` to 30; keep the sample's candidates under 30 characters.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sandbox-core diagnostics::tests`
Expected: compile error.

- [ ] **Step 3: Implement**

`endpoints.rs`:

```rust
/// `resolve` with the reason: which CLI answered, or why nothing did.
pub async fn resolve_traced(candidate: &DetectedRuntime) -> std::result::Result<(String, String), String> {
    match (candidate.kind, candidate.endpoint.as_str()) {
        (RuntimeKind::Docker, "docker") => Ok(match docker_host_from_context(&CommandRunner::Local).await {
            Some(host) => (host, "docker context inspect".into()),
            None => ("docker".into(), "docker context inspect gave no endpoint; bollard defaults".into()),
        }),
        (RuntimeKind::Podman, "podman") => podman_socket(&CommandRunner::Local)
            .await
            .map(|s| (s, "podman machine inspect or podman info".into()))
            .ok_or_else(|| "podman machine inspect and podman info gave no socket".to_string()),
        _ => Ok((candidate.endpoint.clone(), candidate.source.clone())),
    }
}

pub async fn resolve(candidate: &DetectedRuntime) -> Option<String> {
    resolve_traced(candidate).await.ok().map(|(e, _)| e)
}
```

`live.rs`:

```rust
pub async fn connect(candidate: &DetectedRuntime) -> Option<Arc<dyn RuntimeAdapter>> {
    connect_traced(candidate).await.0
}

/// `connect` with the resolve and connect steps recorded.
pub async fn connect_traced(candidate: &DetectedRuntime) -> (Option<Arc<dyn RuntimeAdapter>>, Vec<Probe>) {
    let mut trace = Vec::new();
    let endpoint = match endpoints::resolve_traced(candidate).await {
        Ok((endpoint, how)) => {
            if endpoint != candidate.endpoint {
                trace.push(Probe { kind: candidate.kind, candidate: how, outcome: ProbeOutcome::Found { endpoint: endpoint.clone() } });
            }
            endpoint
        }
        Err(reason) => {
            trace.push(Probe { kind: candidate.kind, candidate: format!("resolve {}", candidate.endpoint), outcome: ProbeOutcome::Failed { reason } });
            return (None, trace);
        }
    };
    let step = format!("connect {endpoint}");
    let adapter: Option<Arc<dyn RuntimeAdapter>> = match candidate.kind {
        RuntimeKind::Docker | RuntimeKind::Podman => match DockerAdapter::connect(candidate.kind, &endpoint) {
            Ok(a) => Some(Arc::new(a)),
            Err(e) => {
                trace.push(Probe { kind: candidate.kind, candidate: step, outcome: ProbeOutcome::Failed { reason: e.to_string() } });
                return (None, trace);
            }
        },
        RuntimeKind::AppleContainer => Some(Arc::new(AppleContainerAdapter::new(endpoint.clone()))),
    };
    (adapter, trace)
}
```

`DockerAdapter::connect` succeeds lazily for most endpoints, so `diagnostics::run` adds a ping. `diagnostics.rs`:

```rust
pub async fn run(app_version: &str, preferred: Option<RuntimeKind>, hosts: &[RemoteHost]) -> DiagnosticsReport {
    let env = Environment::current();
    let (found, mut local) = crate::detect::detect_traced(&env);
    let config_path = crate::config::config_path().map(|p| p.display().to_string()).unwrap_or_default();
    let environment = EnvironmentReport {
        os: env.os.to_string(),
        docker_host: env.docker_host.clone(),
        container_host: env.container_host.clone(),
        runtime_override: std::env::var("SANDBOX_WATCHER_RUNTIME").ok(),
        config_runtime: crate::config::load().runtime,
        config_path,
        binaries: ["docker", "podman", "container"].iter().map(|b| BinaryCheck { name: b.to_string(), found: (env.binary_on_path)(b) }).collect(),
    };
    let pin_source = if std::env::var("SANDBOX_WATCHER_RUNTIME").is_ok() { "SANDBOX_WATCHER_RUNTIME" } else { "sandbox config" };
    for candidate in &found {
        if preferred.is_some_and(|p| p != candidate.kind) {
            local.push(Probe { kind: candidate.kind, candidate: format!("use {}", candidate.endpoint), outcome: ProbeOutcome::Skipped { reason: format!("pinned to {} by {pin_source}", preferred.unwrap()) } });
            continue;
        }
        let (adapter, steps) = crate::live::connect_traced(candidate).await;
        local.extend(steps);
        if let Some(adapter) = adapter {
            let outcome = match adapter.ping().await {
                Ok(info) => ProbeOutcome::Found { endpoint: format!("{} {}", info.endpoint, info.version.unwrap_or_default()).trim().to_string() },
                Err(e) => ProbeOutcome::Failed { reason: e.to_string() },
            };
            local.push(Probe { kind: candidate.kind, candidate: "ping".into(), outcome });
        }
    }
    let mut reports = Vec::new();
    let tests = hosts.iter().filter(|h| h.enabled).map(|h| crate::remote_hosts::test_remote_host(&h.alias));
    let tested: Vec<HostStatus> = futures::future::join_all(tests).await;
    let mut tested = tested.into_iter();
    for h in hosts {
        let status = if h.enabled {
            let mut s = tested.next().expect("one status per enabled host");
            s.display_name = h.display_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| h.alias.clone());
            s
        } else {
            HostStatus { alias: h.alias.clone(), display_name: h.display_name.clone().unwrap_or_else(|| h.alias.clone()), state: HostState::Disabled, runtimes: vec![], probes: vec![] }
        };
        reports.push(HostReport { alias: h.alias.clone(), status });
    }
    DiagnosticsReport { generated_at: Utc::now(), app_version: app_version.into(), environment, local, hosts: reports }
}
```

`config::config_path` signature: check `config.rs:25-40` and call it the way `allowlist::global_config_path` does (it takes the env pieces); build the arguments from the process env the same way `src-tauri/src/lib.rs:470-481` does. `render_text` writes exactly the format in the test: header line, blank line, `Environment` block, `Local runtimes` block with `format!("  {:<7} {:<30} {}", kind, candidate, outcome)` where outcome renders as `found <endpoint>`, `missing`, `skipped <reason>`, `failed <reason>`, then one block per host with `state:` line (`connected`, `connecting`, `disabled`, or `unreachable since <time>: <reason>`) followed by the probes indented by two spaces. Timestamps `%Y-%m-%d %H:%M:%S UTC`.

`mock.rs`:

```rust
pub fn demo_diagnostics(app_version: &str) -> crate::diagnostics::DiagnosticsReport {
    // Fixed values so the mock app shows a stable page.
    ...
}
```

with one found Docker probe, one missing Podman probe, one skipped Apple probe and the `devbox` host as connected with one probe line.

- [ ] **Step 4: Run checks**

Run: `cargo test -p sandbox-core && cargo clippy -p sandbox-core --all-targets -- -D warnings && cargo fmt --all --check`

- [ ] **Step 5: Commit**

```bash
git add crates/sandbox-core/src/diagnostics.rs crates/sandbox-core/src/live.rs crates/sandbox-core/src/endpoints.rs crates/sandbox-core/src/mock.rs
git commit -m "feat(core): connection diagnostics report"
```

---

### Task 10: Tauri commands and contracts for diagnostics

**Files:**

- Modify: `src-tauri/src/lib.rs` (two commands, registration)
- Modify: `crates/sandbox-core/tests/contracts.rs` (new `diagnostics_report` test, `ProbeOutcome` enum)
- Modify: `src/types.ts`, `src/api.ts`
- Create: `fixtures/contracts/DiagnosticsReport.json` (generated), update `fixtures/contracts/enums.json`

- [ ] **Step 1: Contract test**

In `contracts.rs`:

```rust
use sandbox_core::diagnostics::{BinaryCheck, DiagnosticsReport, EnvironmentReport, HostReport, Probe, ProbeOutcome};

fn probe_outcomes() -> Vec<ProbeOutcome> {
    vec![
        ProbeOutcome::Found { endpoint: "/var/run/docker.sock".into() },
        ProbeOutcome::Missing,
        ProbeOutcome::Skipped { reason: "pinned to docker by sandbox config".into() },
        ProbeOutcome::Failed { reason: "permission denied".into() },
    ]
}

#[test]
fn diagnostics_report() {
    let probes: Vec<Probe> = probe_outcomes()
        .into_iter()
        .enumerate()
        .map(|(i, outcome)| Probe { kind: runtime_kinds()[i % 3], candidate: format!("candidate {i}"), outcome })
        .collect();
    let host = |state: HostState| HostReport {
        alias: "devbox".into(),
        status: HostStatus { alias: "devbox".into(), display_name: "Dev VM".into(), state, runtimes: vec![RuntimeKind::Docker], probes: vec!["docker: unix:///var/run/docker.sock".into()] },
    };
    contract(
        "DiagnosticsReport",
        vec![
            DiagnosticsReport {
                generated_at: at(0),
                app_version: "0.8.0".into(),
                environment: EnvironmentReport {
                    os: "macos".into(),
                    docker_host: Some("unix:///Users/dev/.colima/default/docker.sock".into()),
                    container_host: Some("unix:///tmp/podman.sock".into()),
                    runtime_override: Some("docker".into()),
                    config_runtime: Some(RuntimeKind::Docker),
                    config_path: "/Users/dev/.config/sandbox/config.toml".into(),
                    binaries: vec![BinaryCheck { name: "docker".into(), found: true }, BinaryCheck { name: "podman".into(), found: false }],
                },
                local: probes,
                hosts: vec![host(HostState::Connected), host(HostState::Unreachable { reason: "Permission denied".into(), since: at(1) })],
            },
            DiagnosticsReport {
                generated_at: at(2),
                app_version: "0.8.0".into(),
                environment: EnvironmentReport { os: "linux".into(), docker_host: None, container_host: None, runtime_override: None, config_runtime: None, config_path: String::new(), binaries: vec![] },
                local: vec![],
                hosts: vec![host(HostState::Disabled), host(HostState::Connecting)],
            },
        ],
    );
}
```

In `enums()`: `enums.insert("ProbeOutcome", enum_values(&probe_outcomes(), Some("outcome")));`.

- [ ] **Step 2: Types and API**

`src/types.ts`:

```ts
export type ProbeOutcome =
  | { outcome: "found"; endpoint: string }
  | { outcome: "missing" }
  | { outcome: "skipped"; reason: string }
  | { outcome: "failed"; reason: string };

export type Probe = ProbeOutcome & { kind: RuntimeKind; candidate: string };

export interface BinaryCheck {
  name: string;
  found: boolean;
}

export interface EnvironmentReport {
  os: string;
  docker_host: string | null;
  container_host: string | null;
  runtime_override: string | null;
  config_runtime: RuntimeKind | null;
  config_path: string;
  binaries: BinaryCheck[];
}

export interface HostReport {
  alias: string;
  status: HostStatus;
}

export interface DiagnosticsReport {
  generated_at: string;
  app_version: string;
  environment: EnvironmentReport;
  local: Probe[];
  hosts: HostReport[];
}
```

`src/api.ts`:

```ts
export function runDiagnostics(): Promise<DiagnosticsReport> {
  return invoke<DiagnosticsReport>("run_diagnostics");
}

export function diagnosticsText(report: DiagnosticsReport): Promise<string> {
  return invoke<string>("diagnostics_text", { report });
}
```

- [ ] **Step 3: Commands**

`src-tauri/src/lib.rs`:

```rust
/// Probe every runtime candidate and every remote host now. On demand only.
#[tauri::command]
async fn run_diagnostics(state: State<'_, App>) -> Result<DiagnosticsReport, String> {
    let version = env!("CARGO_PKG_VERSION");
    if matches!(state.mode, Mode::Mock(_)) {
        return Ok(sandbox_core::mock::demo_diagnostics(version));
    }
    let settings = state.settings();
    let preferred = settings.runtime_override.or_else(sandbox_core::live::preferred_runtime);
    Ok(sandbox_core::diagnostics::run(version, preferred, &settings.remote_hosts).await)
}

/// The report as plain text, for copying into an issue.
#[tauri::command]
fn diagnostics_text(report: DiagnosticsReport) -> String {
    sandbox_core::diagnostics::render_text(&report)
}
```

Register both in `generate_handler!`. `DiagnosticsReport` needs `Deserialize` for the second command (it has it).

- [ ] **Step 4: Regenerate fixtures and check**

Run: `SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-core --test contracts && pnpm check:contracts && cargo clippy -p sandbox-core --all-targets -- -D warnings && pnpm build`
Expected: new fixture file, `enums.json` gains `ProbeOutcome`, contracts ok.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs crates/sandbox-core/tests/contracts.rs src/types.ts src/api.ts fixtures/contracts/DiagnosticsReport.json fixtures/contracts/enums.json
git commit -m "feat: diagnostics commands and contract"
```

---

### Task 11: Diagnostics section in Settings and the banner link

**Files:**

- Create: `src/components/DiagnosticsSection.tsx`
- Modify: `src/components/SettingsPanel.tsx:6-12` (props), `:337-341` (render after `RemoteHostsSection`)
- Modify: `src/components/RuntimeBanner.tsx` (optional `onDiagnostics` prop)
- Modify: `src/App.tsx:224-232`
- Create: `src/components/__tests__/DiagnosticsSection.test.tsx`
- Modify: `src/components/__tests__/RuntimeBanner.test.tsx`, `SettingsPanel.test.tsx`
- Modify: `src/styles.css` or wherever `.settings-section` lives (table style `.diagnostics-table`)

**Interfaces:**

- `DiagnosticsSection` props: `{ initialReport?: DiagnosticsReport | null }` (for tests); exports `DiagnosticsReportView({ report })` pure component.
- `RuntimeBanner` gains `onDiagnostics?: () => void`; shown as a button "Diagnostics" in the not-detected, detected-only and unavailable banners.
- `SettingsPanel` gains `focusDiagnostics?: boolean`; when true, the section scrolls into view on mount.

- [ ] **Step 1: Write the failing tests**

`src/components/__tests__/DiagnosticsSection.test.tsx`:

```tsx
import { describe, expect, it } from "vitest";
import { render } from "../../test/render";
import type { DiagnosticsReport } from "../../types";
import { DiagnosticsReportView } from "../DiagnosticsSection";

const report: DiagnosticsReport = {
  generated_at: "2026-11-01T12:00:00Z",
  app_version: "0.8.0",
  environment: {
    os: "linux",
    docker_host: null,
    container_host: null,
    runtime_override: null,
    config_runtime: "docker",
    config_path: "/home/me/.config/sandbox/config.toml",
    binaries: [
      { name: "docker", found: true },
      { name: "podman", found: false },
    ],
  },
  local: [
    {
      kind: "docker",
      candidate: "/var/run/docker.sock",
      outcome: "found",
      endpoint: "/var/run/docker.sock",
    },
    {
      kind: "docker",
      candidate: "connect /var/run/docker.sock",
      outcome: "failed",
      reason: "permission denied",
    },
    { kind: "podman", candidate: "CONTAINER_HOST", outcome: "missing" },
  ],
  hosts: [
    {
      alias: "devbox",
      status: {
        alias: "devbox",
        display_name: "Dev VM",
        state: "unreachable",
        reason: "Permission denied",
        since: "2026-11-01T11:00:00Z",
        runtimes: [],
        probes: ["docker: not found on PATH"],
      },
    },
  ],
};

describe("DiagnosticsReportView", () => {
  it("shows reasons for failed probes and the host probes", () => {
    const { text } = render(<DiagnosticsReportView report={report} />);
    expect(text).toContain("permission denied");
    expect(text).toContain("Dev VM");
    expect(text).toContain("docker: not found on PATH");
    expect(text).toContain("runtime = docker");
  });

  it("puts found probes first", () => {
    const { html } = render(<DiagnosticsReportView report={report} />);
    expect(html.indexOf("/var/run/docker.sock</code>")).toBeLessThan(
      html.indexOf("CONTAINER_HOST"),
    );
  });
});
```

`RuntimeBanner.test.tsx`:

```tsx
it("links to diagnostics only when nothing is connected", () => {
  const notDetected: RuntimeStatus = { status: "not-detected" };
  const { text } = render(<RuntimeBanner runtime={notDetected} onDiagnostics={() => undefined} />);
  expect(text).toContain("Diagnostics");
  const ok = render(<RuntimeBanner runtime={connected()} onDiagnostics={() => undefined} />);
  expect(ok.text).not.toContain("Diagnostics");
});
```

`SettingsPanel.test.tsx` "renders every section": add `"Diagnostics"` to the list and `expect(text).toContain("Run diagnostics")`.

- [ ] **Step 2: Run to verify they fail**

Run: `pnpm test -- Diagnostics RuntimeBanner SettingsPanel`
Expected: FAIL.

- [ ] **Step 3: Implement**

`DiagnosticsSection.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";
import { diagnosticsText, runDiagnostics } from "../api";
import { RUNTIME_LABELS, type DiagnosticsReport, type Probe } from "../types";

function outcomeText(p: Probe): { label: string; detail: string } {
  switch (p.outcome) {
    case "found":
      return { label: "found", detail: p.endpoint };
    case "missing":
      return { label: "missing", detail: "" };
    case "skipped":
      return { label: "skipped", detail: p.reason };
    case "failed":
      return { label: "failed", detail: p.reason };
  }
}

const ORDER = { found: 0, failed: 1, skipped: 2, missing: 3 } as const;

export function DiagnosticsReportView({ report }: { report: DiagnosticsReport }) {
  const env = report.environment;
  const local = [...report.local].toSorted((a, b) => ORDER[a.outcome] - ORDER[b.outcome]);
  return (
    <div className="diagnostics">
      <h4>Environment</h4>
      <dl className="diagnostics-env">
        <dt>OS</dt>
        <dd>{env.os}</dd>
        <dt>DOCKER_HOST</dt>
        <dd>{env.docker_host ?? <span className="muted">not set</span>}</dd>
        <dt>CONTAINER_HOST</dt>
        <dd>{env.container_host ?? <span className="muted">not set</span>}</dd>
        <dt>SANDBOX_WATCHER_RUNTIME</dt>
        <dd>{env.runtime_override ?? <span className="muted">not set</span>}</dd>
        <dt>sandbox config</dt>
        <dd>
          <code>{env.config_path || "none"}</code>
          {env.config_runtime && <span>, runtime = {env.config_runtime}</span>}
        </dd>
        <dt>on PATH</dt>
        <dd>{env.binaries.map((b) => `${b.name} ${b.found ? "yes" : "no"}`).join(", ")}</dd>
      </dl>
      <h4>Local runtimes</h4>
      <table className="diagnostics-table">
        <thead>
          <tr>
            <th>Runtime</th>
            <th>Candidate</th>
            <th>Outcome</th>
            <th>Detail</th>
          </tr>
        </thead>
        <tbody>
          {local.map((p, i) => {
            const o = outcomeText(p);
            return (
              <tr key={`${p.kind}-${p.candidate}-${i}`}>
                <td>{RUNTIME_LABELS[p.kind]}</td>
                <td>
                  <code>{p.candidate}</code>
                </td>
                <td>
                  <span
                    className={`badge badge-${p.outcome === "found" ? "ok" : p.outcome === "failed" ? "err" : "muted"}`}
                  >
                    {o.label}
                  </span>
                </td>
                <td>{o.detail}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {report.hosts.map((h) => (
        <section key={h.alias}>
          <h4>
            Remote host {h.status.display_name}
            {h.status.display_name !== h.alias && <span className="muted"> ({h.alias})</span>}
          </h4>
          <p>
            {h.status.state === "unreachable"
              ? `unreachable since ${new Date(h.status.since).toLocaleString()}: ${h.status.reason}`
              : h.status.state}
          </p>
          {h.status.probes.length > 0 && (
            <ul>
              {h.status.probes.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </div>
  );
}

export function DiagnosticsSection({
  initialReport = null,
  focus = false,
}: {
  initialReport?: DiagnosticsReport | null;
  focus?: boolean;
}) {
  const [report, setReport] = useState<DiagnosticsReport | null>(initialReport);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    if (focus) ref.current?.scrollIntoView({ block: "start" });
  }, [focus]);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      setReport(await runDiagnostics());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  const copy = async () => {
    if (!report) return;
    await navigator.clipboard.writeText(await diagnosticsText(report));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <section className="settings-section" id="diagnostics" ref={ref}>
      <h3>Diagnostics</h3>
      <p className="muted small">
        Probes every place a runtime could be, locally and on each remote host, and shows what
        answered. Nothing is sent anywhere; copy the report into an issue yourself.
      </p>
      <div className="settings-row">
        <button type="button" onClick={() => void run()} disabled={running}>
          {running ? "Running…" : "Run diagnostics"}
        </button>
        {report && (
          <button type="button" onClick={() => void copy()}>
            {copied ? "Copied" : "Copy report"}
          </button>
        )}
        {report && (
          <span className="muted small">{new Date(report.generated_at).toLocaleString()}</span>
        )}
      </div>
      {error && <p className="notice notice-err">{error}</p>}
      {report && <DiagnosticsReportView report={report} />}
    </section>
  );
}
```

`SettingsPanel.tsx`: add `focusDiagnostics?: boolean` to props and render `<DiagnosticsSection focus={focusDiagnostics} />` after `RemoteHostsSection`. `RuntimeBanner.tsx`: add `onDiagnostics?: () => void` to both components' props and, inside the `not-detected`, `detected-only` and `unavailable` cases, append:

```tsx
{
  onDiagnostics && (
    <p>
      <button type="button" onClick={onDiagnostics}>
        Diagnostics
      </button>
    </p>
  );
}
```

`App.tsx`: add state `const [focusDiagnostics, setFocusDiagnostics] = useState(false);`, pass `onDiagnostics={() => { setFocusDiagnostics(true); setShowSettings(true); }}` to the `RuntimeBanner` at `:232`, pass `focusDiagnostics={focusDiagnostics}` to `SettingsPanel`, and reset it in `onClose`. Add `.diagnostics-table` styles next to `.network-table` (same look) and `.diagnostics-env { display: grid; grid-template-columns: max-content 1fr; gap: 0.2rem 1rem; }`.

- [ ] **Step 4: Run checks**

Run: `pnpm lint && pnpm format:check && pnpm test && pnpm build`

- [ ] **Step 5: Commit**

```bash
git add src/components/DiagnosticsSection.tsx src/components/SettingsPanel.tsx src/components/RuntimeBanner.tsx src/App.tsx src/components/__tests__ src/styles.css
git commit -m "feat(ui): connection diagnostics in Settings"
```

---

### Task 12: Documentation

**Files:**

- Modify: `README.md` (Remote hosts section, Troubleshooting pointer), `docs/TROUBLESHOOTING.md` (first step), `docs/MILESTONES.md` (backlog, risks), `AGENTS.md` (allow list rule)

- [ ] **Step 1: Edit**

README "Remote hosts": the bullet list already changed in Tasks 2 and 6; add "- The Allow dialog edits the remote host's own sandbox config through SSH, with the same path and symlink rules as locally, and refuses to write when the file changed since the preview." Under "If no containers show up" (`README.md:70`): "If no containers show up, open Settings, Diagnostics, Run diagnostics, or see [Troubleshooting](docs/TROUBLESHOOTING.md)."

`docs/TROUBLESHOOTING.md`: new first paragraph under the intro: "Start with Settings, Diagnostics, Run diagnostics. It lists every socket, variable and binary the app checked, locally and per remote host, and why each failed. Copy report puts the same text on the clipboard for an issue."

`docs/MILESTONES.md`: backlog lines for remote hosts: "~~Remote hosts follow-ups~~: shipped in 0.8.0: `ssh://` context import (0.7.0), allow list on remote hosts, plain `tcp://` endpoints. TLS Docker endpoints are not supported." Risks: "Mitigation: the Diagnostics section in Settings (0.8.0) shows what was probed and why it failed."

AGENTS.md allow list rule: append "On remote hosts `allowlist_remote.rs` runs the same rules as POSIX scripts over the SSH master; the TOML edit stays local and the write is a SHA-256 compare-and-swap under `flock`. Keep both paths in step."

- [ ] **Step 2: Format and commit**

Run: `pnpm format:check` (fix with `pnpm exec oxfmt <file>` if needed).

```bash
git add README.md docs/TROUBLESHOOTING.md docs/MILESTONES.md AGENTS.md
git commit -m "docs: remote allow list, tcp endpoints and diagnostics"
```

---

## Self-review notes

- Spec coverage: §1 Tasks 3 to 7; §2 Tasks 1 and 2; §3 Tasks 8 to 11; Security constraints enforced in Tasks 1 and 4 tests; Documentation Task 12. The spec's `AllowPreview` local hashes are `None` (the UI ignores them for local containers), a simplification over "computed locally too": no `sha2` dependency is needed because the host computes every hash it later compares.
- Review Focus tests: quoting (Task 4), shasum fallback (Task 4 asserts both commands are present; Task 7 runs the real script on CI's Linux where `sha256sum` exists, the macOS branch is exercised by the user's own remote Mac), missing global file (Task 7), IPv6 (Task 1), disabled host (Task 9).
- Type consistency: `run_on_host(alias, command, timeout) -> Result<CommandOutput, String>` in Tasks 3 and 5; `ForwardTarget::parse` in Tasks 1 and 2; `Probe`/`ProbeOutcome` field names in Tasks 8 to 11; `expectedHash` camelCase on the API side maps to `expected_hash` in Task 5.
