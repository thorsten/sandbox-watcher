# Remote hosts over SSH: design

Date: 2026-10-31. Target release: 0.6.0. Status: approved in conversation, awaiting spec review.

## Goal

Watch sandbox containers on a remote dev VM or workstation from the desktop app, the same
way local containers are watched today. The typical setup is a Mac running the watcher and a
Linux VM (or a second Mac) where the user runs `sandbox` over SSH.

Success means: a host added by its SSH alias shows its containers, sessions, network
activity, logs, stats and history next to the local ones, and its containers can be stopped
from the app. Nothing else on the remote machine is changed.

## Decisions

| Question        | Decision                                                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Main use case   | One or a few remote dev machines reached over SSH.                                                                             |
| Remote actions  | Watch plus stop only. Host-side actions (open, editor, terminal, allow list) are unavailable.                                  |
| Remote runtimes | Docker, Podman and Apple `container`.                                                                                          |
| Transport       | One SSH master connection per host: socket forwarding for Docker and Podman, CLI over SSH for Apple `container`.               |
| Platforms       | macOS and Linux. Windows is out of scope (the system `ssh` multiplexing used here is Unix-only, and Windows work is deferred). |

Rejected alternatives:

- Per-runtime transports (bollard's `connect_with_ssh` via `docker system dial-stdio` for
  Docker, socket forward for Podman, CLI for Apple): three connection paths with three
  failure modes, and bollard would open its own SSH session next to ours.
- CLI over SSH for every runtime: loses streaming events and stats for Docker and Podman and
  needs new adapters instead of reusing `docker_compat.rs`.

## Architecture

### Configuration

`Settings` gains `remote_hosts: Vec<RemoteHost>`:

```rust
pub struct RemoteHost {
    /// Alias from ~/.ssh/config, or user@host. Validated, see Security.
    pub alias: String,
    /// Shown in badges and notifications; defaults to the alias.
    pub display_name: Option<String>,
    pub enabled: bool,
}
```

No user names, ports, keys or passwords are stored by the app. They come from the user's SSH
configuration, and authentication goes through keys or the SSH agent.

`runtime_override` stays local-only. A remote host uses the `runtime` key of its own sandbox
`config.toml`, otherwise the probe order Docker, Podman, Apple `container`.

### New units in `sandbox-core`

**`ssh.rs`: SSH session manager.** One `SshSession` per enabled host.

- Start: `ssh -M -N -S <control> -o BatchMode=yes -o ControlPersist=no -- <alias>`, where
  `<control>` is inside a per-process temporary directory created with mode `0700`.
- `run(args, timeout) -> CommandOutput`: `ssh -S <control> -o BatchMode=yes -- <alias> <quoted args>`.
- `forward(remote_socket) -> local_socket`: `ssh -S <control> -O forward -L <local>:<remote> -- <alias>`;
  `<local>` is a new socket path in the same private directory.
- `close()`: `ssh -S <control> -O exit -- <alias>`, then remove the control and forward sockets.
- `state() -> HostState`: `Connecting`, `Connected`, `Unreachable { reason, since }` or `Disabled`.

**`CommandRunner`.** `enum CommandRunner { Local, Ssh(Arc<SshSession>) }` with one async
`run(binary, args, timeout) -> Result<CommandOutput>`. `endpoints.rs` (socket discovery) and
`AppleContainerAdapter::run` (`apple.rs`) use it instead of spawning `tokio::process::Command`
themselves. Local behaviour is unchanged.

**Remote detection.** For a connected host, run the existing probes through the SSH runner:
`docker context inspect`, `podman info` / `CONTAINER_HOST`, `container system status`, plus
reading the remote sandbox `config.toml` for `runtime`. Docker and Podman sockets are forwarded
and handed to the existing Docker-compatible adapter. Apple `container` gets an
`AppleContainerAdapter` with the SSH runner.

### Identity

Today adapters are keyed by `RuntimeKind` (`live.rs`) and Tauri commands find an adapter from
`container_id` alone (`watcher.rs`, `adapter_for`). Apple `container` IDs are names, so IDs can
collide across machines.

- New `Host` enum: `Local` or `Remote(String)` (the alias). New `RuntimeId { host, kind }`.
- `Live` keys adapters and event tasks by `RuntimeId`.
- `ContainerOverview` gains `host: Host`, serialised flat as `host` (`"local"` or the alias).
- Every Tauri command addresses a container by its container key: `<id>` for local
  containers, `<alias>/<id>` for remote ones (aliases cannot contain `/`). The Rust side
  parses the key with `parse_container_key`; `adapter_for(key)` returns the adapter and the
  raw id. Stats, network reports, notifications and history are keyed the same way.
- History keys become `<host>/<container id>` for remote containers. Local keys stay as they
  are, so existing history files load unchanged.
- `src/types.ts` mirrors the new fields.

The `RuntimeAdapter` trait, the adapters, the conformance suite, and all probes and parsers
that run through `exec` stay unchanged.

## UI and actions

Available for remote containers: overview, sessions, turns, tokens and cost, files touched
(as a list), network, logs, stats, history, notifications, stop and stop all (with the same
confirmation; stop all spans every host and groups the listed sessions by host).

Unavailable for remote containers, hidden in the UI with a "remote host" tooltip and refused
on the Rust side with `Err("not available for remote hosts")` when `host` is not `Local`:
`open_project`, `open_in_editor`, `open_terminal`, `reveal_mount`, `preview_allow`,
`apply_allow`, and opening touched files.

Copy helpers: container name and project path work as-is (the path is the remote one). The
network logs command is copied as `ssh <alias> sandbox network logs …` for remote containers.

Display:

- Host badge with the display name next to the runtime badge; local containers have none.
- Host filter next to the runtime filter.
- Tray: one summary over all hosts; the quick list groups projects by host.
- Notification titles carry the host: "devbox: session ended in billing-api".

Settings get a "Remote hosts" section: add an alias, "Test connection" (master connection
plus remote detection, with the probe report shown like the runtime banner), enable or
disable each host, remove a host.

## Lifecycle and failure handling

- On start, master connections open in the background; local runtimes do not wait for them.
- A refresh queries all hosts in parallel (the existing `join_all` in `overview.rs`), each
  host with its own timeout, default 10 s, so a slow host never delays the local overview.
- Docker and Podman events arrive through the forwarded socket as locally. Apple `container`
  is polled as locally.
- When the master connection drops, its forwards drop with it. Reconnect uses exponential
  backoff from 5 s to 5 min, reset on wake from sleep and on "Test connection". Each
  reconnect re-runs remote detection, since remote socket paths can change.
- On quit, every session is closed with `ssh -O exit` and the private directory is removed.

Errors are shown in a per-host banner modelled on `sandbox doctor`:

| Condition                        | Message                                                                           |
| -------------------------------- | --------------------------------------------------------------------------------- |
| `Host key verification failed`   | Add the host to `known_hosts` first: run `ssh <alias>` once in a terminal.        |
| `Permission denied (publickey…)` | Key or agent authentication is required; the watcher cannot prompt for passwords. |
| No runtime found                 | Lists every probe that ran and its result.                                        |
| Remote runtime socket denied     | Same hint as locally (user not in the `docker` group, rootless socket path).      |
| Timeout                          | Host did not answer within the timeout; shows the last successful refresh.        |

While a host is unreachable, its containers stay visible from the last snapshot, greyed out
with "last seen <time>". Notifications for that host are suppressed until it is connected
again, so a dropped connection does not fire "container died" or "session ended".

## Security

- Aliases must match `^[A-Za-z0-9._@-]+$` and must not start with `-`. Every `ssh` call puts
  `--` before the alias. This prevents option injection such as `-oProxyCommand=…`.
- The app never accepts host keys and always runs with `BatchMode=yes`.
- Remote commands are run by the remote login shell, so every argument is quoted by one
  `shell_quote` helper. Unit tests cover spaces, single and double quotes, `$`, `` ` ``, `;`,
  `|`, newlines and empty strings.
- Container execs for Docker and Podman go through the API over the forwarded socket, not
  through a remote shell.
- Control and forward sockets live in a private `0700` directory owned by the user and are
  removed on quit.
- "Never read environment variable values" still holds; probes are unchanged.

Documentation changes: the `AGENTS.md` rule "The app only talks to the local runtime" and the
README privacy section become: the app talks to the local runtime and to remote hosts the
user adds, only through the user's own `ssh`, and sends data nowhere else.

## Testing

Unit tests in `sandbox-core`:

- `shell_quote`, alias validation, and the argument lists built for master, `run`,
  `-O forward` and `-O exit`.
- Host state machine and backoff, driven by a fake `CommandRunner`; no real SSH.
- `endpoints.rs` and `AppleContainerAdapter` against the fake runner with hand-written
  canned remote output inline in the tests (Ruling R5).
- Identity: overview merge with the same container ID on two hosts; history keys; existing
  local history files still load.
- Remote guard: every host-side command refuses a remote host.

Conformance in CI (Linux): install `openssh-server`, create a throwaway key, add `localhost`
to `known_hosts`, then run the existing conformance suite against Docker and Podman through
`SshSession` and a forwarded socket, enabled by `SANDBOX_WATCHER_SSH_HOST=localhost`. A test
also closes the master with `ssh -O exit` and checks the reconnect. Remote Apple `container`
is verified by hand against a second Mac and recorded in the README.

UI (Vitest, static rendering): host badge and filter, hidden actions for remote containers,
unreachable state with "last seen", the settings section in its initial state. Mock mode
gains a demo remote host `devbox` with one container.

## Order of work

Each step ends with all checks green and its own commit.

1. `Host`, `RuntimeId` and host identity through core, Tauri commands and `src/types.ts`;
   local only, no behaviour change.
2. `CommandRunner`; `endpoints.rs` and `apple.rs` moved onto it; no behaviour change.
3. `ssh.rs`: session, forward, quoting, state machine, backoff.
4. Remote detection and wiring in `live.rs`; `remote_hosts` setting.
5. UI: badges, filter, hidden actions, settings section, unreachable state, mock host.
6. CI SSH conformance; README, `AGENTS.md`, `docs/TROUBLESHOOTING.md`; mark the backlog item
   in `docs/MILESTONES.md`.

## Out of scope

- Windows.
- Password, passphrase or 2FA prompts.
- Host-side actions on remote hosts: open project, editor, terminal, reveal mount, allow list.
- Jump hosts beyond what `ProxyJump` in the user's SSH config already provides.
- Docker contexts with `ssh://` endpoints as an automatic source of hosts.
