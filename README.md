<p align="center">
  <img src="media/logo.svg" alt="Sandbox Watcher logo" width="128" height="128">
</p>

# Sandbox Watcher

A cross-platform desktop app that shows what is happening in your running
[MaibornWolff Sandbox](https://github.com/MaibornWolff/sandbox) containers:
which projects have a sandbox up, which agents run inside, what they reach
on the network, and what they consume.

Status: 0.7.0, all milestones implemented. macOS releases after 0.4.0 are signed and notarised. See [`docs/MILESTONES.md`](docs/MILESTONES.md)
for the plan and [`docs/adr/`](docs/adr/) for decisions.

## What you see

Containers sit on the left, grouped by project, as compact cards with their
agent sessions. The right side shows the selected container:

- **Activity:** what the agent is doing right now. The last messages and
  tool calls from the agent's own session transcript, with the call in flight
  highlighted, for Claude Code, Codex, OpenCode and Pi. For every session,
  the processes running under it. Plus the project's git changes.
- **Files:** every file the agent read, created, edited or deleted, from its
  whole transcript, changed files first. Click one to open it in your editor.
- **Turns:** one row per prompt you typed, with its duration, tokens, cost,
  tool calls and the files it changed. Read from the whole transcript when
  you ask for it.
- **Network:** blocked and allowed destinations from sandbox's own logs.
- **Resources:** CPU and memory sparklines, network and disk I/O.
- **Logs:** the container log with follow and filter.
- **Mounts:** project, persisted, runtime and extra mounts, ports and env names.
- **History:** every session and container the watcher has seen, with
  duration, tokens and cost, by agent and by project. Stored locally, pruned
  after a configurable number of days.
- **Allow-list triage:** tick blocked hosts in the Network tab and add them to
  sandbox's `allow_network` list in the global or project config, the same
  edit `sandbox network allow` makes, after a preview of the exact lines.
- **Tokens and cost:** per agent session from its own transcript. Pi and
  OpenCode report cost; Claude Code and Codex are estimated from an editable
  price table in Settings.

## Editor

"Editor" on a container and every file in the Files list open in the editor
chosen in Settings: the system default app, Visual Studio Code, Cursor, Zed,
the first installed JetBrains IDE (PhpStorm, IntelliJ IDEA, WebStorm, …) or a
custom command with `{path}`. Only paths inside the container's project
directory are opened.

## Install

Download the latest build from the
[releases page](https://github.com/thorsten/sandbox-watcher/releases/latest)
or the [project site](https://thorsten.github.io/sandbox-watcher/).

- **macOS:** open the `.dmg` and drag the app to Applications. The app is
  signed and notarised by Apple. Releases up to 0.4.0 are unsigned; for those,
  run `xattr -dr com.apple.quarantine "/Applications/Sandbox Watcher.app"`
  once, or right-click the app and choose _Open_.
- **Windows:** not signed yet. Run the `.msi` or `-setup.exe`; on the SmartScreen prompt choose
  _More info_, then _Run anyway_.
- **Linux:** install the `.deb` or `.rpm`, or `chmod +x` the `.AppImage`.

The app lives in the tray or menu bar. Closing the window keeps it running;
quit from the tray menu. Settings (refresh interval, runtime, terminal,
notifications, launch at login, theme) are in the window's Settings panel and
stored as `settings.json` in the app's config directory.

If no containers show up, see [Troubleshooting](docs/TROUBLESHOOTING.md).

## Layout

| Path                  | What                                                                                                                               |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `crates/sandbox-core` | Rust library: domain model, `RuntimeAdapter` contract, sandbox conventions, runtime detection, Docker/Podman adapter, mock adapter |
| `src-tauri`           | Tauri 2 shell exposing core functions as commands                                                                                  |
| `src`                 | React + TypeScript UI                                                                                                              |
| `fixtures/docker`     | Docker Engine API responses used by tests                                                                                          |
| `docs`                | Milestone plan and ADRs                                                                                                            |

## Develop

Requirements: Rust stable, Node 24, pnpm 12, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
pnpm install
pnpm tauri dev                 # live mode: Docker or Podman via the detected socket
pnpm tauri:dev:mock            # demo data from the mock adapter
```

Checks:

```sh
cargo test -p sandbox-core
# Adapter conformance against a real Docker daemon (needs a prepared container,
# see the core job in .github/workflows/ci.yml):
SANDBOX_WATCHER_DOCKER_TESTS=1 cargo test -p sandbox-core --test conformance
cargo clippy --all-targets -- -D warnings
pnpm lint && pnpm format:check && pnpm check:contracts && pnpm test && pnpm build
```

Further checks (Rust/TypeScript contracts, dependency audits, pinned GitHub
Actions, performance budgets) and how to run them locally are described in
[`docs/CHECKS.md`](docs/CHECKS.md) and [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md).

## Runtimes

Sandbox Watcher talks to the same runtimes sandbox does and finds them the
same way: `DOCKER_HOST`, then the usual sockets, then the binary on `PATH`.
When sandbox's user config sets `runtime`, only that runtime is used; set
`SANDBOX_WATCHER_RUNTIME` to override. Otherwise every runtime that answers
is shown, with a runtime chip on each container when there is more than one.

| Runtime           | How it is reached                                                                                                               | Notes                                                                                       |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| Docker            | Engine API over the socket, named pipe or `DOCKER_HOST`; the active `docker context` when only the binary is found              | Full support: events, stats, logs                                                           |
| Podman            | Docker-compatible API over the rootless or system socket, `CONTAINER_HOST`, or the machine socket from `podman machine inspect` | Full support                                                                                |
| Apple `container` | The `container` CLI (`list`, `inspect`, `exec`, `logs`, `stop`)                                                                 | No event stream and no stats; the Resources tab is hidden and the list refreshes by polling |

Conformance against real Docker and Podman runs in CI on Linux. Apple
`container` can only be verified on an Apple silicon Mac running macOS 26:

```sh
container run -d --name sandbox-watcher-test \
  --label sandbox.project=watcher-test --label sandbox.hash=abcdef123456 \
  alpine:3 sh -c 'mkdir -p /tmp/sandbox-sessions && touch /tmp/.sandbox-ready && sleep 600'
SANDBOX_WATCHER_DOCKER_TESTS=1 SANDBOX_WATCHER_RUNTIME=apple-container \
  cargo test -p sandbox-core --test conformance -- --nocapture
```

## Remote hosts

Watch containers on another machine over SSH.

- Add an SSH alias from `~/.ssh/config` under Settings, then Remote hosts.
  Use key or agent authentication only; the app cannot prompt for passwords.
- Or pick hosts from your Docker contexts: Settings, Remote hosts, lists every
  `ssh://` context. Contexts with a port or path need an alias in
  `~/.ssh/config` first; the list offers the entry to copy.
- Accept the host key once in a terminal with `ssh <alias>`. The app never
  accepts host keys.
- Docker, Podman and Apple `container` are supported on the remote machine.
  Only Unix-socket endpoints are forwarded; TCP Docker contexts are not.
- Remote containers can be watched and stopped. Terminal, folder, editor,
  reveal and the allow list are not available for them.
- Windows is not supported.

Conformance over SSH runs in CI on Linux against `localhost`
(`SANDBOX_WATCHER_SSH_HOST=localhost`). Remote Apple `container` is checked by
hand against a second Mac; record the date and macOS version here when you
repeat it: not yet verified.

## Updates

The app checks GitHub Releases on start and every six hours, shows the new
version's notes in a banner, and installs on click. Downloads are verified
against the public key in `src-tauri/tauri.conf.json` before installing.
Supported for the macOS `.app`, the Windows installers and the Linux
AppImage; `.deb` and `.rpm` installs update through the package manager.

The release workflow signs the update artifacts with the
`TAURI_SIGNING_PRIVATE_KEY` secret (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
if the key has one). Losing that key means existing installs can no longer
verify updates.

## Builds and releases

GitHub Actions builds everything. `CI` runs on every push and pull request:
Rust tests on macOS, Linux and Windows, the Docker conformance test on
Linux, web lint, tests and build, the contract and performance checks, and
an unsigned Tauri debug build per OS. `Dependency audit` checks Cargo and
pnpm dependencies on pull requests and weekly. All actions are pinned to
commit SHAs and updated by Dependabot.
`Release` runs on tags `v*` (or manually) and publishes installers for macOS
(Apple silicon), Linux and Windows. macOS bundles are signed and notarised
once the secrets below exist; Windows and Linux bundles are unsigned. `Pages`
compiles `site/theme.ts` with `pnpm build:site` and deploys the `site/`
folder to GitHub Pages.

Both workflows create a CycloneDX SBOM (`sbom` job) from `Cargo.lock` and
`pnpm-lock.yaml`. CI keeps it as the `sbom` workflow artifact; releases
attach it as `sandbox-watcher-vX.Y.Z.cdx.json`. It includes development
dependencies, so it lists more than the installers ship.

### macOS signing

Needs a paid Apple Developer Program membership and a **Developer ID
Application** certificate, exported from Keychain Access as `.p12`. Add these
repository secrets; signing turns on as soon as `APPLE_CERTIFICATE` is set:

| Secret                       | Value                                                |
| ---------------------------- | ---------------------------------------------------- |
| `APPLE_CERTIFICATE`          | `base64 -i certificate.p12`                          |
| `APPLE_CERTIFICATE_PASSWORD` | Password of the `.p12` export                        |
| `APPLE_SIGNING_IDENTITY`     | `Developer ID Application: Your Name (TEAMID)`       |
| `APPLE_ID`                   | Apple ID used for notarisation                       |
| `APPLE_PASSWORD`             | App-specific password from appleid.apple.com         |
| `APPLE_TEAM_ID`              | Team ID from the developer account's Membership page |

Instead of `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID`, notarisation can
use an App Store Connect API key: `APPLE_API_ISSUER` (issuer ID),
`APPLE_API_KEY` (key ID) and `APPLE_API_PRIVATE_KEY` (contents of the `.p8`
file). The key wins when both are set.

## Changelog

`CHANGELOG.md` is generated from the commit history by `pnpm changelog`.
Release notes for a tag come from the same script and land in the GitHub
release automatically.

To cut a release, run `pnpm release 0.1.1`. It bumps the version in
`Cargo.toml`, `Cargo.lock`, `package.json` and `tauri.conf.json`, regenerates
the changelog, commits and tags. Push `main` and the tag; the `Release`
workflow builds and publishes the installers.

## Commits

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
and are checked by commitlint. Husky installs the hooks on `pnpm install`:
`pre-commit` runs lint and format check, `commit-msg` runs commitlint.
Pull requests are checked again in CI.

```sh
git commit -m "feat(core): add docker adapter"
git commit -m "fix(ui): keep stopped containers visible"
```

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and the
[code of conduct](CODE_OF_CONDUCT.md). Report security problems privately as
described in [`SECURITY.md`](SECURITY.md).

## License

MIT, see [`LICENSE`](LICENSE).

## Privacy

The app talks to the local container runtime and to remote hosts you add,
only through your own `ssh`, and to nothing else, with one exception: the update check asks GitHub Releases for a new version on start
and every six hours, which sends your IP address to GitHub. It can be turned
off in Settings. The app never reads environment variable values from
containers, only their names, and it sends no container, session or usage
data anywhere.
