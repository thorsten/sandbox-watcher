# ADR 0001: Tech stack

Status: accepted, 2026-10-30

## Context

Sandbox Watcher must run on macOS, Linux and Windows, sit in the tray for
long periods, and talk to a local container runtime (Docker, Podman, Apple
`container`). See `docs/MILESTONES.md` section 6 for the options considered.

## Decision

- **Shell:** Tauri 2. Native tray, notifications and auto-update, small
  binaries, and WebView from the OS instead of a bundled Chromium.
- **Core:** Rust library crate `sandbox-core` (`crates/sandbox-core`). It owns
  the domain model, the `RuntimeAdapter` contract and all runtime logic. The
  Tauri app (`src-tauri`) is a thin layer that exposes core functions as
  commands. A future CLI or TUI can reuse the same crate.
- **Runtime access:** HTTP API over the local socket or named pipe via
  `bollard` for Docker and Podman. Structured responses, streaming stats and
  events. Apple `container` gets a CLI-based adapter in M5.
- **UI:** React 19 with TypeScript, built by Vite. Tests with Vitest. Lint
  and format with Biome.
- **Package manager:** pnpm. Node 24, matching sandbox's own requirement.

## Consequences

- Rust and Node toolchains are both required to build. CI installs both.
- Linux builds need `webkit2gtk-4.1`, `libappindicator3`, `librsvg2` and
  `patchelf` on the build host. CI installs them; developers on Linux must
  do the same.
- Domain types exist twice, in Rust (`sandbox-core::model`) and TypeScript
  (`src/types.ts`). They are kept in sync by hand until a generator is
  justified. The Rust side is the source of truth.
