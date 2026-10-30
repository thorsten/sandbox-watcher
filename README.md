# Sandbox Watcher

A cross-platform desktop app that shows what is happening in your running
[MaibornWolff Sandbox](https://github.com/MaibornWolff/sandbox) containers:
which projects have a sandbox up, which agents run inside, what they reach
on the network, and what they consume.

Status: milestone M0 (foundation). See [`docs/MILESTONES.md`](docs/MILESTONES.md)
for the plan and [`docs/adr/`](docs/adr/) for decisions.

## Layout

| Path | What |
|---|---|
| `crates/sandbox-core` | Rust library: domain model, `RuntimeAdapter` contract, sandbox conventions, runtime detection, mock adapter |
| `src-tauri` | Tauri 2 shell exposing core functions as commands |
| `src` | React + TypeScript UI |
| `fixtures/docker` | Docker Engine API responses used by tests |
| `docs` | Milestone plan and ADRs |

## Develop

Requirements: Rust stable, Node 24, pnpm 12, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
pnpm install
pnpm tauri dev                 # live mode: detects runtimes, lists containers from M1 on
pnpm tauri:dev:mock            # demo data from the mock adapter
```

Checks:

```sh
cargo test -p sandbox-core
cargo clippy --all-targets -- -D warnings
pnpm lint && pnpm test && pnpm build
```

## Privacy

The app only talks to the local container runtime. It never reads
environment variable values from containers, only their names, and it sends
nothing anywhere.
