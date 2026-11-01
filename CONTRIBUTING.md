# Contributing

Thanks for helping with Sandbox Watcher. Bug reports, fixes and features are
all welcome. By taking part you agree to the [code of conduct](CODE_OF_CONDUCT.md).

## Before you start

- **Bugs:** open an issue with the app version, OS, container runtime and
  sandbox version, and what you expected versus what happened.
- **Security problems:** do not open an issue; follow [SECURITY.md](SECURITY.md).
- **Features and larger changes:** open an issue first so the approach can be
  agreed before you spend time on it. The plan per milestone is in
  [`docs/MILESTONES.md`](docs/MILESTONES.md), decisions are in
  [`docs/adr/`](docs/adr/).

## Set up

Requirements: Rust stable, Node 24, pnpm, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
pnpm install                     # also installs the git hooks
pnpm tauri:dev:mock              # app with demo data, no runtime needed
pnpm tauri dev                   # live against Docker / Podman / Apple container
```

## Rules that matter

[`AGENTS.md`](AGENTS.md) lists the project's rules and applies to everyone,
human or not. The short version:

- Logic lives in `crates/sandbox-core`; `src-tauri` stays a thin shell.
- Sandbox conventions (labels, paths, log formats) live in
  `crates/sandbox-core/src/sandbox.rs` only. If you change them, re-check the
  upstream files and run `pnpm check:upstream --update`.
- Never read environment variable values from containers, and never send data
  anywhere.
- Paths and destructive actions are decided on the Rust side, and stopping
  asks for confirmation.
- Keep `src/types.ts` in sync with serialised Rust types.

## Checks

Run these before you push; the pre-commit hook runs the web ones.

```sh
cargo fmt --all --check
cargo clippy -p sandbox-core --all-targets -- -D warnings
cargo test -p sandbox-core
pnpm lint && pnpm format:check && pnpm check:contracts && pnpm test && pnpm build
```

New behaviour needs tests. UI tests run in Node with static rendering; see
the "UI tests" section in `AGENTS.md`. If you change a serialised Rust type,
regenerate `fixtures/contracts/` as described in
[`docs/CHECKS.md`](docs/CHECKS.md), which also covers the dependency audits,
the pinned GitHub Actions and the performance budgets.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
  with a lower-case subject, for example `fix(ui): keep stopped containers visible`.
  commitlint checks them locally and in CI.
- Keep a pull request to one topic. Describe what changed, why, and how you
  tested it, including which runtime and OS you ran it against.
- CI must be green. It runs the Rust tests on macOS, Linux and Windows, the
  conformance tests against Docker and Podman, the web checks, the
  Rust/TypeScript contract check, the performance budgets and, when a
  manifest or lock file changes, the dependency audits.

Contributions are released under the [MIT License](LICENSE).
