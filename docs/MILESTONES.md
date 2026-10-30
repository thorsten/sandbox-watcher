# Sandbox Watcher: Milestone Plan

A cross-platform desktop app that shows what is happening in your running
[MaibornWolff Sandbox](https://github.com/MaibornWolff/sandbox) containers:
which projects have a sandbox up, which agents are running inside, what they
are doing on the network, and how much they consume.

Status: draft, 2026-10-30. Based on sandbox `0.73.1`.

---

## 1. What we can observe (integration notes)

Everything below was derived from the sandbox source and docs. It defines the
surface the watcher builds on. Sandbox has no API or event stream of its own,
so the watcher talks to the container runtime directly and reads sandbox's
conventions.

| Signal | Where it comes from | Notes |
|---|---|---|
| Sandbox containers | Runtime list filtered by label `sandbox.project=<slug>` | Works across all projects at once. `sandbox status` only shows the current project. |
| Container name | `sandbox-<project-slug>`, suffixed `-2`, `-3`, … for extra containers | Slug is derived from the project root path. |
| Config identity | Label `sandbox.hash` (12 hex chars) | Changes when image, mounts, env, version, or runtime change. Useful to flag "stale container". |
| Image | Container image reference (`localhost/` prefix stripped) | Layered user/project images. |
| Uptime / state | `inspect` → `startedAt`, `state` | States: running, exited, dead, stopping. |
| Active sessions | Marker files in `/tmp/sandbox-sessions/<pid>` inside the container, plus `/proc/<pid>/cmdline` | Requires `exec` into the container. Stale markers are cleaned by sandbox. Command tells us the agent (claude, codex, opencode, pi, shell, …). |
| Readiness | `/tmp/.sandbox-ready` inside the container | Distinguishes "starting" from "ready". |
| Network traffic | `exec` `/usr/local/bin/sandbox-container-tools network diagnostic <source>` and `network state` as root | Sources are squid proxy log, dnsmasq log, firewall (iptables) log. Sandbox aggregates blocked/allowed destinations with counts and last-seen. |
| Resource usage | Runtime stats API (CPU, memory, net I/O, block I/O) | Docker and Podman expose this. Apple `container` needs a fallback. |
| Container logs | Runtime logs API | Startup diagnostics, proxy and firewall bootstrap. |
| Mounts, ports, env names | `inspect` | Shows project path, extra host mounts, read-only flags. Env values are secrets, show names only. |
| User config | `~/.config/sandbox/config.toml` or `%APPDATA%/sandbox/config.toml` | Runtime choice, `allow_network`, persist paths. Read-only for us. |
| Persistent data | `{data}/<project-slug>/`, `{data}/global/`, named volumes | Size on disk per project is a nice-to-have. |
| Lifecycle events | Runtime event stream (start, die, stop, exec_create…) | Enables live updates without polling. |
| Control | `sandbox stop`, `sandbox stop --all`, or runtime stop | Containers also stop themselves after an idle period. |

Runtimes: Docker (macOS, Linux, Windows via Rancher Desktop/WSL2), Podman
(all three), Apple `container` (Apple silicon, macOS 26+). Sandbox picks the
first installed one unless `runtime` is set in the config.

## 2. Product scope

In scope for 1.0:

- Desktop app for macOS, Linux, Windows with a tray or menu-bar presence.
- Read-only overview of all sandbox containers on the local machine.
- Per-container drill-down: sessions, network activity, resources, logs, mounts.
- Live updates and notifications.
- A small set of safe actions: stop container, stop all, open a shell, open the project.

Explicitly out of scope for 1.0: remote hosts, mobile, editing sandbox
configuration, allow-listing domains from the app, token or cost tracking.
These are listed in the backlog.

## 3. Milestones

Effort is a rough guide for one developer working part-time. Each milestone
ends with a tagged pre-release so it can be used and judged on its own.

### M0: Foundation and decisions (1 to 2 weeks)

Goal: a running empty app on all three platforms with CI, and the core
architecture decided.

Deliverables:

- Tech stack decision recorded in `docs/adr/`. Recommended default: Tauri 2
  with a TypeScript UI (React or Svelte) and a Rust core. Reasons: small
  binaries, native tray and notifications, good Windows story, and the Rust
  side can talk to Docker and Podman through their HTTP API (`bollard`).
  Alternative if the team prefers one language: Electron with `dockerode`.
- Repository scaffold: app, lint, format, unit tests, GitHub Actions matrix
  build for macOS, Linux, Windows.
- `RuntimeAdapter` trait or interface: `list_containers(label_filter)`,
  `inspect`, `exec`, `stats`, `logs`, `events`, `stop`. Everything above is
  implemented against this contract so runtimes stay pluggable.
- Domain model: `SandboxContainer`, `Session`, `NetworkEntry`, `ResourceSample`.
- Fixture recordings of real Docker responses for a running sandbox, used in
  tests from here on.

Acceptance: `cargo test` / `npm test` green on all three OSes in CI, app opens
and shows "no runtime detected" or "no sandbox containers" correctly.

### M1: Container overview, Docker only (2 weeks)

Goal: see every sandbox container on the machine in one list.

Deliverables:

- Docker adapter over the Engine API (Unix socket, Windows named pipe).
  Honour `DOCKER_HOST` and Docker contexts so Colima and Rancher Desktop work.
- Discovery by label `sandbox.project`, grouped by project slug.
- List view per container: name, project slug, image, state, uptime, config
  hash, "stale hash" hint when two containers of one project differ.
- Manual refresh plus polling at a configurable interval.
- Empty, loading, and error states (runtime not running, permission denied on
  the socket).

Acceptance: with two projects running under sandbox, the app shows both
containers with correct name, image, and uptime. Stopping one via
`sandbox stop` is reflected within one polling interval.

### M2: Sessions and live updates (2 weeks)

Goal: know which agents are running inside each container, in real time.

Deliverables:

- Session reader: `exec` into the container, list
  `/tmp/sandbox-sessions/`, read `/proc/<pid>/cmdline`, drop dead PIDs.
  Reuse sandbox's own command shape so behaviour matches `sandbox status`.
- Agent classification from the command line: Claude Code, Codex, OpenCode,
  Pi, shell, other. Show an icon and the raw command.
- Readiness state from `/tmp/.sandbox-ready`: starting vs. ready.
- Event stream subscription (`docker events`) to refresh on start, stop,
  die, and exec events instead of polling alone. Polling stays as fallback.
- Dashboard header: total containers, total sessions, sessions per agent.

Acceptance: starting `sandbox run claude` in a project shows a new session
within two seconds. Exiting it removes the session. A shell session and an
agent session in the same container are both listed.

### M3: Network activity (2 to 3 weeks)

Goal: see what a sandbox tried to reach and what was blocked.

Deliverables:

- Collector that runs `sandbox-container-tools network diagnostic <source>`
  and `network state` as root inside the container and parses the proxy,
  DNS, and firewall outputs. Port sandbox's parsers
  (`blocked-log-parsing`, `dns-log-parsing`, `proxy-log-parsing`) or call
  them through a small compatibility layer and pin the sandbox version they
  were taken from.
- Per-container network tab: destination, port, blocked or allowed, count,
  last seen. Filter by status, search by host.
- Dashboard badge for "blocked in the last 5 minutes".
- Known caveats surfaced in the UI: repeated blocked traffic may be
  collapsed, IPv6 can be missing, hostnames may differ for shared IPs.
- Version check: if the container's sandbox runtime is newer than what the
  parsers were tested with, show a warning instead of silently showing
  nothing.

Acceptance: an agent that calls a non-allow-listed domain appears as blocked
within one refresh. The list matches `sandbox network logs --status=all` for
the same container.

### M4: Resources, logs, and mounts (2 weeks)

Goal: understand what a container costs and how it is wired to the host.

Deliverables:

- Streaming stats: CPU percent, memory used and limit, network and block
  I/O, with a short in-memory history for sparklines per container.
- Container log tail with follow, search, and copy. Default to the last
  200 lines.
- Mounts view from `inspect`: host path, container path, read-only, and
  whether it is the project, a persisted path, or runtime files. Clicking a
  host path opens it in the file manager.
- Environment: forwarded variable names only, never values.
- Optional: persisted data size per project from `{data}/<slug>`, computed
  lazily on demand.

Acceptance: CPU and memory update at least every two seconds while an agent
runs a build. Mounts match `docker inspect` for the same container.

### M5: Multi-runtime and platform parity (2 to 3 weeks)

Goal: everything above works with Podman and Apple `container`, and on
Windows.

Deliverables:

- Podman adapter via its Docker-compatible socket, including rootless socket
  discovery (`XDG_RUNTIME_DIR`, `podman machine` on macOS and Windows).
- Apple `container` adapter via the CLI (`container ls --format json`,
  `container inspect`, `container exec`, `container logs`). Stats are
  degraded or hidden if the CLI does not expose them.
- Runtime selection: read `runtime` from the sandbox config, otherwise probe
  in sandbox's order (Docker, Podman, Apple). Allow a manual override and
  show all detected runtimes at once if more than one has sandbox containers.
- Windows: named pipe for Docker, Rancher Desktop and WSL2 path handling,
  `%APPDATA%/sandbox/config.toml`.
- Adapter conformance test suite run against each runtime in CI where
  possible (Docker and Podman on Linux runners, Apple manually).

Acceptance: the M1 to M4 acceptance checks pass on Docker, Podman, and Apple
`container`. A Windows build connects to Rancher Desktop out of the box.

### M6: Actions (1 to 2 weeks)

Goal: the few things you want to do right from the overview, done safely.

Deliverables:

- Stop container and stop all, with confirmation that lists the sessions
  that will be interrupted. Implemented through the runtime adapter so it
  behaves like `sandbox stop` (sessions end, container is removed or kept
  according to the container's `removeOnExit`).
- Open a terminal in the project directory with `sandbox` prefilled, and
  open the project folder in the file manager or editor.
- Copy helpers: container name, project path, `sandbox network logs`
  command for the project.
- Nothing that edits sandbox config or the allow-list in 1.0.

Acceptance: stop from the app behaves identically to `sandbox stop` for
single and shared-session containers. Every destructive action asks first
and can be disabled in settings.

### M7: Tray, notifications, settings, release 1.0 (2 to 3 weeks)

Goal: the app becomes something you leave running.

Deliverables:

- Tray or menu-bar icon with a compact summary (containers, sessions) and a
  quick list of projects. Start minimised to tray option.
- Notifications, each individually switchable: session started or ended,
  container started or idle-stopped, burst of blocked requests, container
  died unexpectedly, stale container detected.
- Settings: refresh interval, runtime override, notification toggles, launch
  at login, theme.
- Packaging and signing: `.dmg` with notarisation, `.msi` or `.exe` with
  code signing, `.AppImage` and `.deb`. Auto-update channel.
- Privacy statement in the README: the app never reads env values, never
  sends data anywhere, and only talks to the local runtime.
- Docs: install, supported runtimes, troubleshooting (socket permissions,
  rootless Podman, Apple `container` limits).

Acceptance: 1.0 tagged, installers for all three OSes published from CI, a
fresh machine with Docker and one sandbox project gets a correct overview
with no configuration.

## 4. Backlog after 1.0

- Timeline and history: persist sessions and events locally, show when
  which agent ran where and for how long.
- Blocked-request triage: suggest allow-list entries and hand them to
  `sandbox network allow` with the user's confirmation.
- Agent-level insight: parse agent logs or hook into agent telemetry where
  available (tokens, turns, files touched).
- Remote hosts over SSH or TCP Docker contexts.
- CLI or TUI mode sharing the same core (`sandbox-watcher --tui`).
- Upstream contribution: propose `sandbox status --json --all` so the watcher
  can drop its own session reader and stay version-proof.
- Mobile companion fed by the desktop app.

## 5. Risks and mitigations

- Coupling to sandbox internals. Labels, marker paths, and the container-tools
  CLI are not a public API and can change between releases. Mitigation: pin
  the tested sandbox version range, run the integration tests against the
  latest release in CI weekly, and pursue the `--json` upstream contribution.
- `exec` requires root in the container for network diagnostics. Mitigation:
  make the network tab opt-in per runtime and degrade gracefully when exec
  is denied.
- Apple `container` has no stats API and a different CLI surface. Mitigation:
  feature-flag per adapter, hide what is not available.
- Rootless Podman and Windows socket paths vary a lot. Mitigation: a
  "connection" diagnostics page that shows what was probed and why it
  failed, modelled on `sandbox doctor`.
- Three platforms multiply packaging and signing effort. Mitigation: do
  packaging early in M0 as an unsigned build so only signing is left for M7.

## 6. Open decisions

Decisions with a recommended default. Override before M0 ends.

1. Tauri 2 + TypeScript UI (recommended) versus Electron. Decide on team
   language comfort; both reach the same 1.0.
2. UI framework: React or Svelte. Recommended: whichever the team already
   uses; no feature depends on it.
3. Talk to Docker through the HTTP API (recommended, structured, streaming
   stats and events) versus shelling out to the `docker` CLI (simpler, but
   requires parsing and polling).
4. Whether M6 actions ship in 1.0 or move behind a flag. Recommended: ship,
   with confirmation dialogs on by default.
