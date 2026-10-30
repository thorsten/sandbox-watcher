# ADR 0002: Runtime adapter contract

Status: accepted, 2026-10-30

## Context

Sandbox supports three container runtimes with different APIs. Sandbox has
no API of its own, so the watcher reads sandbox's conventions (labels,
container names, in-container marker files) through the runtime. All of
that must stay testable without a real runtime.

## Decision

A single async trait `RuntimeAdapter` in `sandbox-core::adapter` with these
operations: `ping`, `list_containers`, `inspect`, `exec`, `stats`, `logs`,
`events`, `stop`. Every feature in M1 to M6 is implemented against this trait
only. Adapters:

- `MockAdapter` (M0): in-memory, used by unit tests and by the app when
  `SANDBOX_WATCHER_MOCK=1` is set.
- `DockerAdapter` (M1), `PodmanAdapter` and `AppleContainerAdapter` (M5).

Sandbox conventions live in `sandbox-core::sandbox` as constants and pure
functions (label names, container name parsing, session marker paths) with
the sandbox version they were taken from noted in a comment.

Fixtures of runtime responses live in `fixtures/<runtime>/` and are loaded by
tests. They are currently authored by hand from the Docker Engine API
schema. `scripts/record-docker-fixtures.sh` replaces them with real
recordings from a machine that has Docker and a running sandbox.

## Consequences

- Features never call a runtime API directly, so adding a runtime means one
  new adapter and the conformance tests.
- Capabilities differ per runtime (Apple `container` has no stats API). The
  trait reports `capabilities()` so the UI can hide what is unavailable
  instead of failing.
