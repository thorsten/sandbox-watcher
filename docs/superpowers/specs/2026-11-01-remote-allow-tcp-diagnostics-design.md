# Remote allow list, TCP Docker contexts, connection diagnostics: design

Date: 2026-11-01. Target release: 0.8.0. Status: approved in conversation, awaiting spec review.

## Goal

Close the remote-hosts chapter and make connection failures explainable inside the app.

1. The allow list works for containers on remote hosts the same way it works locally.
2. A remote host whose Docker daemon listens on `tcp://` is usable, not skipped.
3. A diagnostics page shows every runtime candidate the app tried, locally and per remote
   host, with the result and the reason, modelled on `sandbox doctor`.

Success: the README no longer says "the allow list is not available for remote hosts" or
"TCP Docker contexts are not supported", and a user who sees "no runtime found" can read why
without opening a terminal.

## Decisions

| Question                      | Decision                                                                                                                                          |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Remote allow list targets     | Both, like local: the global `config.toml` and the project `.sandbox/config.toml` on the remote machine, chosen in the Allow dialog.              |
| Remote file I/O               | A POSIX shell script per operation, run through the existing SSH master. TOML editing stays local and reuses `inject_allowed_domains`.            |
| Lost-update protection remote | Compare-and-swap on a content hash under `flock`. The write refuses when the file changed since the preview.                                      |
| TCP Docker over SSH           | Forward a local Unix socket to the remote `host:port` with OpenSSH's `-L <local.sock>:<host>:<port>`. Plain TCP only; TLS endpoints are rejected. |
| Diagnostics scope             | Every probe, in order, for every runtime and every host. On demand only, never on the refresh loop.                                               |
| Diagnostics placement         | A `Diagnostics` section in Settings after Remote hosts, plus a link from the runtime banner when nothing is connected.                            |
| Windows                       | Unchanged: remote hosts stay unsupported from a Windows client. The local diagnostics page works on Windows.                                      |

Rejected alternatives:

- SFTP for remote config files: a second channel and an SFTP client for one small file, and
  no locking.
- A helper binary on the remote host: nothing of ours may be installed there.
- Forwarding a local TCP port for `tcp://` endpoints: `ssh -O forward -L 0:...` does not
  report the allocated port for local forwards, and a fixed port can collide.
- Collecting diagnostics on every refresh: the probes run `docker context inspect`,
  `podman machine inspect` and SSH commands; that is too expensive for a 2 s loop.

## 1. Allow list on remote hosts

### Today

`preview_allow` and `apply_allow` in `src-tauri/src/lib.rs` call `require_local` first.
`allowlist::apply` resolves paths, locks the directory, reads, injects, writes a temp file and
renames, all on the local filesystem. `NetworkPanel.tsx` hides the checkboxes and the Allow
button when the container key has a host prefix.

### Design

The pure parts stay: `entries_for`, `validate_entry`, `existing_entries`,
`inject_allowed_domains`, `ConfigTarget`, `AllowResult`. The filesystem parts get a remote
twin in a new module `crates/sandbox-core/src/allowlist_remote.rs`.

**Session access.** `RemoteSession` gains

```rust
fn run(&self, command: &str, timeout: Duration) -> BoxFuture<'_, crate::Result<CommandOutput>>;
```

implemented by `SshSession::run` and by the test `FakeSession` with a scripted reply.
`RemoteHosts` gains `pub async fn run_on(&self, alias: &str, command: &str, timeout) ->
Result<CommandOutput, String>` which errors with "host is not connected" when there is no live
session. `Watcher` exposes it as `run_on_host`. Only `sandbox-core` builds the commands; the
Tauri layer never concatenates shell text.

**Remote paths.** Resolved on the host by the script, in the same precedence as
`config::config_path`:

```sh
cfg="${SANDBOX_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/sandbox}/config.toml"
```

for the global target, and `<root>/.sandbox/config.toml` for the project target, where
`<root>` is the project path derived from the container's mounts as today
(`actions::project_path` with the Windows flag `false`, remote paths are POSIX). Every path
is passed through `ssh::shell_quote`. `remote::detect_remote` is changed to read the sandbox
config from the same `cfg` expression, so the pin and the allow list agree on the file.

**Preview script** (`allowlist_remote::preview_script(target, root)`), one call per target.
It prints a one-line status followed by the file content:

| First line | Meaning                                                                                                                           |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `path <p>` | Resolved file path, always first.                                                                                                 |
| `missing`  | The file does not exist. Counts as empty, like `read_config`.                                                                     |
| `absent`   | Project target only: `<root>/.sandbox` is not a directory. Target hidden.                                                         |
| `symlink`  | Project: dir or file is a symlink. Global: file is a symlink to a non-regular file. Target shown as unavailable with that reason. |
| `sha <h>`  | SHA-256 of the file content; the content follows on the next lines.                                                               |

The script uses only `test`, `readlink`, `sha256sum` with `shasum -a 256` as fallback, and
`cat`. Symlink rules mirror `allowlist::resolve`: the project target refuses a symlinked
`.sandbox` directory or file; the global target follows a symlinked file only when the target
is a regular file, and then writes the target.

`AllowPreview` gains `global_hash: Option<String>`, `project_hash: Option<String>` and
`global_unavailable: Option<String>`, `project_unavailable: Option<String>` (reasons). For
local containers the hashes are computed locally too, so the UI has one shape.

**Apply script** (`allowlist_remote::apply_script(target, root, expected_hash, new_content)`).
Runs under `flock` on the config directory, with a `.config.toml.lock` file fallback when
`flock` is missing. Steps, matching `allowlist::apply`:

1. Resolve the path with the same rules as the preview; abort on `absent` or `symlink`.
2. For the global target, `mkdir -p` the directory.
3. Hash the current content (`missing` hashes as the empty string's hash). Abort with
   `changed` when it differs from `expected_hash`.
4. Write `new_content` to a temp file created with `mktemp` in the same directory. The
   content travels as a single base64 argument and is decoded with `base64 -d`; sandbox
   configs are far below the argument length limit and base64 avoids every quoting issue.
5. Copy the mode of the old file with `chmod --reference` where that exists, otherwise
   `chmod "$(stat -c %a)"` or `stat -f %Lp`. If all fail the temp file keeps the umask
   mode, which is what sandbox itself produces for a new file.
6. `mv -f` the temp file over the config. Remove the temp file on any failure.
7. Print `ok`.

The new content is produced locally from the previewed content with `inject_allowed_domains`.
The entries actually added and the ones already present are computed locally as well, so
`AllowResult { path, added, already_present }` is unchanged. A `changed` reply surfaces as
"The config on <alias> changed since the preview. Open the preview again." and the dialog
re-runs the preview.

**Tauri.** `preview_allow` and `apply_allow` branch on `parse_container_key`. Local keeps the
current code path. Remote calls the scripts through `run_on_host`. `require_local` stays for
the four open-and-reveal actions.

**UI.** `NetworkPanel.tsx` drops the `remote` gate. `AllowDialog.tsx` shows the alias in the
path lines, renders `*_unavailable` reasons as muted text with the radio disabled, and handles
the `changed` error by reloading the preview.

### Testing

- Unit: script generation is deterministic and every interpolated value is quoted
  (`shell_quote`); a root with a space, a quote and a newline round-trips. Hash of the
  empty string matches the script's `missing` convention.
- Unit: `FakeSession` scripted replies for `missing`, `absent`, `symlink`, `sha`, `changed`,
  `ok`, and a non-zero exit, each mapped to the right `AllowPreview` or error.
- Conformance (`tests/conformance.rs`, `SANDBOX_WATCHER_SSH_HOST=localhost` in CI): create a
  temp project with `.sandbox/config.toml`, preview, apply, re-read the file and compare
  with the local `apply` on the same input; then modify the file and assert `changed`;
  then a symlinked `.sandbox` and assert `symlink`. Runs the real scripts under `sh`,
  `dash` on the CI image.
- Contract fixtures for `AllowPreview` regenerated.

## 2. TCP Docker contexts over SSH

### Today

`remote::connect_host` strips `unix://` and skips any endpoint that does not start with `/`,
with the probe "is not a Unix socket; only sockets are forwarded". `ssh::validate_remote_socket`
rejects `:`.

### Design

`ssh.rs` gets a `ForwardTarget` enum:

```rust
pub enum ForwardTarget {
    UnixSocket(String),      // absolute path, no ':'
    Tcp { host: String, port: u16 },
}
```

`ForwardTarget::parse(endpoint)` accepts `unix://<path>`, a bare absolute path,
`tcp://<host>:<port>` and `tcp://[<v6>]:<port>`. The host grammar is strict: letters,
digits, `.`, `-`, `_`, or a bracketed IPv6 literal, at most 253 bytes; the port is 1 to 65535. Anything else is an error. `https://` is rejected with "TLS Docker endpoints are not
supported", since bollard would need the client certificates from the remote machine. A
daemon that speaks TLS on a `tcp://` endpoint fails at the first ping; that error is shown
with the hint "the daemon may require TLS, which is not supported".

`SshSession::forward(target)` builds the `-L` argument as `<local.sock>:<path>` or
`<local.sock>:<host>:<port>`. OpenSSH forwards a Unix-domain listener to a TCP destination
since 6.7, so the adapter side is unchanged: `DockerAdapter::connect` still receives a local
socket path.

`connect_host` replaces the "not a Unix socket" branch with `ForwardTarget::parse`; a parse
error becomes the probe line. `endpoints::resolve_remote`'s doc comment and the README line
change accordingly.

### Testing

- Unit: `ForwardTarget::parse` table covering every accepted and rejected form, including
  `tcp://127.0.0.1:2375`, `tcp://[::1]:2375`, `tcp://host:2376` (rejected), `tcp://a b:1`,
  missing port, port 0, port 70000, `ssh://` (rejected).
- Unit: `forward_args` for a TCP target produces exactly one `-L` with three fields.
- Conformance: CI starts `socat TCP-LISTEN:2375,fork UNIX-CONNECT:/var/run/docker.sock` on
  the runner and runs the SSH conformance suite a second time with
  `SANDBOX_WATCHER_SSH_DOCKER_HOST=tcp://127.0.0.1:2375`, which the test exports as
  `DOCKER_HOST` for the remote `docker context inspect` fallback. Reuses the existing sshd
  setup.

## 3. Connection diagnostics

### Today

`detect::detect` returns hits only. `live::connect` drops resolve and connect errors with
`.ok()`. `RuntimeStatus` carries candidates and failures of the last round, and
`HostStatus.probes` carries human-readable lines per remote host. `RuntimeBanner.tsx` shows
them in a `<details>`.

### Design

**Core.** New module `crates/sandbox-core/src/diagnostics.rs`.

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum ProbeOutcome {
    Found { endpoint: String },
    Missing,
    Skipped { reason: String },
    Failed { reason: String },
}

pub struct Probe {
    pub kind: RuntimeKind,
    pub candidate: String,   // "DOCKER_HOST", "/var/run/docker.sock", "docker on PATH", "docker context inspect", ...
    pub outcome: ProbeOutcome,
}

pub struct EnvironmentReport {
    pub os: String,
    pub docker_host: Option<String>,
    pub container_host: Option<String>,
    pub runtime_override: Option<String>,   // SANDBOX_WATCHER_RUNTIME
    pub config_runtime: Option<RuntimeKind>, // sandbox config pin
    pub config_path: String,
    pub binaries: Vec<BinaryCheck>,          // docker, podman, container on PATH
}

pub struct BinaryCheck {
    pub name: String,
    pub found: bool,
}

pub struct HostReport {
    pub alias: String,
    pub status: HostStatus,                 // existing type, probes included
}

pub struct DiagnosticsReport {
    pub generated_at: String,               // RFC 3339
    pub app_version: String,
    pub environment: EnvironmentReport,
    pub local: Vec<Probe>,
    pub hosts: Vec<HostReport>,
}
```

`detect.rs` gains `detect_traced(env) -> (Vec<DetectedRuntime>, Vec<Probe>)`; `detect`
becomes a thin wrapper that discards the trace, so the refresh loop is unchanged. Every
candidate the current code checks becomes one `Probe`: environment variables, each socket
path per OS, each binary on PATH, and the OS gate for Apple `container`.

`live.rs` gains `connect_traced(candidate) -> (Option<Arc<dyn RuntimeAdapter>>, Vec<Probe>)`
recording the `endpoints::resolve` result (which `docker context` or `podman machine` was
used, or why it failed) and the `DockerAdapter::connect` error. `connect` wraps it.

`diagnostics::run(hosts: &[RemoteHost]) -> DiagnosticsReport` runs `detect_traced`, applies
the pin as `Skipped { reason: "pinned to <kind> by <source>" }`, calls `connect_traced` for
the remaining candidates, pings each adapter and records the version, then calls
`remote_hosts::test_remote_host` for every enabled host concurrently with the existing 20 s
timeout. Mock mode returns a fixed report.

`diagnostics::render_text(&DiagnosticsReport) -> String` produces the copyable report:
one block per section, one line per probe, values of `DOCKER_HOST` and `CONTAINER_HOST`
included since they are endpoints, nothing else from the environment.

**Tauri.** `run_diagnostics() -> DiagnosticsReport` and `diagnostics_text() -> String`
commands. Both run outside the refresh lock.

**UI.** `src/components/DiagnosticsSection.tsx` as a `settings-section` after Remote hosts:
a Run button, then Environment as a two-column list, Local probes as a table (runtime,
candidate, outcome, reason) with found rows first, one table per host, and Copy report. While
running the button is disabled and shows "Running…"; a report older than the current run stays
visible until replaced. `RuntimeStatusBanner` gets a "Diagnostics" link that opens Settings
scrolled to the section when the status is not-detected, detected-only or unavailable.

### Testing

- Unit: `detect_traced` with a fake environment yields one probe per candidate in order, and
  the hits equal `detect`'s result for the same environment (property across the existing
  detection tests).
- Unit: `connect_traced` with a candidate whose endpoint cannot be reached records a
  `Failed` probe with the adapter error.
- Unit: `render_text` snapshot for a report with every outcome variant and one host.
- Contract fixtures: `DiagnosticsReport` with every `ProbeOutcome` variant and both states of
  every `Option`; `enums.json` gains `ProbeOutcome`.
- UI: a static-render test that a report with a failed probe shows its reason, and that the
  banner link appears only when nothing is connected.

## Security

- Every value interpolated into a remote script passes `ssh::shell_quote`; file content is
  base64. The scripts are built in `sandbox-core` behind functions with unit tests on
  quoting; the Tauri layer passes typed values only.
- The remote allow list writes only the two paths sandbox itself would write, derived on the
  host from its environment and from the container's mounts. No path comes from the webview.
- Symlink rules are the same as local. The project target refuses symlinks; the global target
  follows a symlinked file to a regular file only.
- The compare-and-swap aborts when the file changed between preview and apply, so a
  concurrent `sandbox network allow` on the host cannot be overwritten.
- `ForwardTarget` validates host and port before they reach the `-L` argument, so a crafted
  `docker context` endpoint cannot add fields to the forward.
- The diagnostics report contains endpoint values and probe outcomes. It never contains
  environment variable values other than `DOCKER_HOST`, `CONTAINER_HOST` and
  `SANDBOX_WATCHER_RUNTIME`, and nothing from inside containers.

## Documentation

- README: remove "the allow list is not available for remote hosts", replace "TCP Docker
  contexts are not supported" with "TLS Docker endpoints are not supported", add a
  Diagnostics paragraph under Troubleshooting pointing at Settings.
- `docs/TROUBLESHOOTING.md`: first step becomes "open Settings, Diagnostics, Run".
- `docs/MILESTONES.md` backlog: mark the two remote follow-ups shipped in 0.8.0, drop the
  diagnostics risk mitigation's "planned".
- AGENTS.md rules: extend the allow list rule with the remote twin and its compare-and-swap.
- `src/types.ts` and contract fixtures for `AllowPreview`, `DiagnosticsReport`,
  `ProbeOutcome`.

## Out of scope

- Remote open, editor, terminal, reveal. They need the files on this machine.
- TLS Docker endpoints and client certificates.
- Windows as the client for remote hosts.
- Running diagnostics automatically or sending them anywhere.
