# Working on Sandbox Watcher

Guidance for AI coding agents and new contributors. Keep it short and current.

## What this is

A Tauri 2 desktop app that shows what is happening in running
[MaibornWolff Sandbox](https://github.com/MaibornWolff/sandbox) containers.
The plan and acceptance criteria per milestone are in `docs/MILESTONES.md`;
decisions are in `docs/adr/`. Read both before changing architecture.

## Layout

| Path                  | Role                                                                                                                                                                |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/sandbox-core` | All logic: domain model, `RuntimeAdapter` trait, Docker/Podman/Apple adapters, sandbox conventions, probes, parsers, stats hub, actions. Source of truth for types. |
| `src-tauri`           | Thin Tauri shell: commands call core and return serialisable state. No logic here.                                                                                  |
| `src`                 | React + TypeScript UI. `src/types.ts` mirrors the Rust types by hand.                                                                                               |
| `fixtures/`           | Recorded or hand-written runtime responses used by tests. Each folder has a README saying which.                                                                    |
| `scripts/`            | Helpers such as fixture recording.                                                                                                                                  |

## Rules that matter

- **Sandbox conventions live in one place.** Labels, marker paths, container
  tools paths and log formats come from a specific sandbox release and are
  documented in `crates/sandbox-core/src/sandbox.rs` with the source file they
  were read from. When you touch them, bump `TESTED_SANDBOX_VERSION` and
  re-check the upstream file. Nothing of this is a public API of sandbox.
- **Never read environment variable values** from containers, only names.
  Never send data anywhere. The app only talks to the local runtime.
- **Features talk to the `RuntimeAdapter` trait only.** New runtime support
  means a new adapter plus the conformance suite in
  `crates/sandbox-core/tests/conformance.rs`, not special cases elsewhere.
- **Paths and destructive actions are decided on the Rust side.** The webview
  never passes a path to open; commands derive it from the container's own
  mounts. Stopping asks for confirmation in the UI.
- **Our own execs must not trigger refreshes.** The session probe carries
  `PROBE_SENTINEL`, diagnostics carry `DIAGNOSTIC_COMMAND_PREFIX`; see
  `is_relevant` in `live.rs` before adding another exec.
- **Keep `src/types.ts` in sync** when a serialised Rust type changes.
  Serialisation is kebab-case for enums, flat for `ContainerOverview`.

## Commands

```sh
pnpm install                     # also installs the git hooks
pnpm tauri:dev:mock              # app with demo data, no runtime needed
pnpm tauri dev                   # live against Docker / Podman / Apple container

cargo test -p sandbox-core
cargo clippy -p sandbox-core --all-targets -- -D warnings
cargo fmt --all --check
pnpm lint && pnpm format:check && pnpm test && pnpm build
```

Conformance against a real runtime needs a prepared container; the recipe is
in `.github/workflows/ci.yml` (Docker and Podman) and in the README (Apple).

## Verification expectations

- Run the Rust and web checks above before every commit; the pre-commit hook
  runs the web ones. Commit messages follow Conventional Commits and are
  checked by commitlint.
- If you change `src-tauri`, compile it. On Linux without GTK/WebKit, a
  `cargo clippy -p sandbox-watcher --target aarch64-apple-darwin` cross-check
  works if the C compiler for one Objective-C helper is stubbed; CI builds
  the real thing on all three OSes.
- Cross-check Windows when touching platform code:
  `cargo clippy -p sandbox-core --all-targets --target x86_64-pc-windows-msvc`.
  bollard, for example, has no `connect_with_unix` there.
- A milestone counts as done only after it ran on a real machine against
  real sandbox containers. Say explicitly what was verified where.

## Tooling

oxlint and oxfmt for TypeScript (not Biome, not ESLint, not Prettier). rustfmt
and clippy with warnings as errors for Rust. pnpm with `supportedArchitectures`
set, because the project folder is used from macOS and a Linux sandbox.

## Activity view coupling

`crates/sandbox-core/src/activity.rs` reads each agent's own session store
inside the container and parses its shapes. None of these formats are ours,
and they were written from the agents' known layouts, not from captured
files; when a timeline stays empty for a real session, capture a sample and
fix the parser:

| Agent       | Location in the container                                                               | Shape                                                                                                 |
| ----------- | --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Claude Code | `~/.claude/projects/<cwd>/<session>.jsonl`                                              | `assistant`/`user` lines with `tool_use`/`tool_result` content                                        |
| Codex       | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`                                          | `response_item` envelopes with `message`, `function_call`, `function_call_output`, `custom_tool_call` |
| Pi          | `~/.pi/agent/sessions/<cwd>/<session>.jsonl`                                            | `message` lines with `toolCall` parts and `toolResult` role                                           |
| OpenCode    | `~/.local/share/opencode/storage/message/<session>/*.json` plus `part/<message>/*.json` | message and part JSON files; tool parts carry `state.status`                                          |

A transcript is only shown when a session of that agent is running in the
container, so an old file never reads as current activity.

## Known gaps

- Apple `container` adapter is tested only against sandbox's fixture JSON.
- Fixtures under `fixtures/docker` and `fixtures/sandbox-logs` are
  hand-written from documented formats, not recordings.
- Persisted data size per project (M4 optional) is not implemented.
