# Automated checks

What CI verifies beyond tests, how to run each check locally, and how to
maintain it. Everything here is deterministic except the wall-clock budgets,
which are measured on one runner type with wide margins.

| Check                               | Where it runs                                   | Local command                                                         |
| ----------------------------------- | ----------------------------------------------- | --------------------------------------------------------------------- |
| Rust/TypeScript contracts           | `CI` core (Rust side), `CI` web (TS side), hook | `cargo test -p sandbox-core --test contracts && pnpm check:contracts` |
| Cargo advisories, licences, sources | `Dependency audit` on PRs, main, weekly         | `cargo deny check`                                                    |
| pnpm advisories                     | `Dependency audit` on PRs, main, weekly         | `pnpm audit --audit-level moderate`                                   |
| Action pins                         | `CI` web (offline), `Dependency audit` (online) | `pnpm check:actions` / `pnpm check:actions -- --verify`               |
| Performance budgets                 | `CI` perf (release, Linux)                      | see [PERFORMANCE.md](PERFORMANCE.md)                                  |
| Sandbox upstream files              | `Sandbox upstream` weekly                       | `pnpm check:upstream`                                                 |

## Rust/TypeScript contracts

`src/types.ts` is written by hand; the Rust types are the source of truth.
Two tests keep them honest without generating code:

1. `crates/sandbox-core/tests/contracts.rs` serialises every type a Tauri
   command returns, accepts or emits, with every enum variant and every
   `Option` both empty and set, into `fixtures/contracts/<Type>.json`.
   Each sample is also deserialised again, so the input direction
   (`save_settings`) is covered. `fixtures/contracts/enums.json` lists every
   enum's wire values. Each enum list is guarded by an exhaustive `match`,
   so a new variant fails to compile until it has a sample. The test fails
   when a fixture differs from the current serialisation.
   `src-tauri/src/contracts.rs` does the same for the three types that live
   in the shell (`UpdateInfo`, `UpdateProgress`, `AllowPreview`); it runs in
   CI's app job, because the shell needs the system web view to compile.
2. `scripts/check-contracts.mjs` (`pnpm check:contracts`) turns each sample
   into `const x: T.<Type> = <json>` in a temporary TypeScript project and
   runs `tsc`. Object literals get excess-property checks, so a field Rust
   sends that TypeScript lacks fails, as do missing fields, wrong
   nullability, a wrong tag spelling and a wrong scalar type. For enums it
   also checks the other direction: a union member Rust never produces
   fails through `Exclude<Union, KnownValues>`.

Covered by construction: kebab-case enum names, `tag`/`content` layouts,
`#[serde(flatten)]` (`ContainerOverview`, `HostStatus`, `RuntimeEvent`,
`ConnectedRuntime`), `#[serde(rename)]` (`Mount.type`), `#[serde(default)]`
inputs, and `Option` as `| null`.

When you change a serialised Rust type:

```sh
SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-core --test contracts
# for UpdateInfo, UpdateProgress or AllowPreview, on a machine that builds the shell:
SANDBOX_WATCHER_UPDATE_CONTRACTS=1 cargo test -p sandbox-watcher contracts
pnpm check:contracts        # now fails until src/types.ts matches
```

Commit the fixtures with the Rust and TypeScript change; the diff of the
JSON is the review artefact. A new type that crosses the bridge needs a
`contract("TypeName", samples)` call with samples for every variant and
both states of every optional field, and the enum needs an entry in the
`enums` test. `pnpm check:contracts -- --keep` leaves the generated project
in `target/contracts/` for inspection.

## Dependency audits

`.github/workflows/audit.yml` runs on pull requests that touch a manifest
or lock file, on every push to `main`, every Monday (new advisories against
an unchanged tree) and on demand.

**Cargo** uses [cargo-deny](https://embarkstudios.github.io/cargo-deny/)
with `deny.toml`: advisories from the RustSec database, a licence allow
list, banned crates and crate sources, for the three shipped targets.
Install once with `cargo install cargo-deny --locked`; `cargo deny check`
is the full run, `cargo deny check advisories` the quick one. Several
versions of one crate are allowed (binary size, not safety; `cargo tree -d`
lists them). Both crates are `publish = false` so that the path dependency
of the shell on the core passes the wildcard check.

**pnpm** uses `pnpm audit --audit-level moderate` for all dependency
groups: the development tools build the bundle that ships.

### Exceptions

No exception is active. A finding is handled in this order:

1. Upgrade the dependency, or the dependency that pulls it in
   (`cargo tree -i <crate>`, `pnpm why <package>`).
2. Replace it.
3. Record an exception with a reason and a re-check date, in the same pull
   request as the analysis:
   - Cargo: `[advisories].ignore` in `deny.toml`, for example
     `{ id = "RUSTSEC-2026-0001", reason = "<why the vulnerable code path is not reachable>; re-check by 2026-12-31; issue #123" }`.
     Licence exceptions go to `[licenses].exceptions` with the crate name.
   - pnpm: `pnpm.auditConfig.ignoreGhsas` in `package.json`, plus a row in
     the table below, because `package.json` cannot hold comments.

| Advisory | Package | Reason | Re-check by | Issue |
| -------- | ------- | ------ | ----------- | ----- |
| none     |         |        |             |       |

Never raise the audit level, disable a check or ignore a whole crate to make
the run green. Remove an exception when the fix is available; the re-check
date is when to look again, not when the exception expires on its own.

## Action pins

Every third-party action in `.github/workflows/` is pinned to a full commit
SHA with the release in a trailing comment:

```yaml
- uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
```

The SHAs were resolved from the GitHub API on 2026-11-01 for the release
each floating tag pointed to at that time (`dtolnay/rust-toolchain` has no
release tags; its `stable` branch tip is pinned and the toolchain is passed
explicitly). `scripts/check-action-pins.mjs` enforces the format offline in
CI's web job and, with `--verify` in the audit workflow, asks GitHub whether
the commented tag or branch still resolves to the pinned SHA.

Updates come from Dependabot (`.github/dependabot.yml`, weekly, one grouped
pull request for all actions), which bumps the SHA and the comment together.
Major version jumps arrive the same way; read the action's release notes
before merging them. To pin a new action by hand:

```sh
curl -s https://api.github.com/repos/<owner>/<repo>/tags?per_page=100 | jq -r '.[] | "\(.name) \(.commit.sha)"'
```

and copy the SHA of the release you want with its name in the comment, then
run `pnpm check:actions -- --verify`.

## Workflow permissions

Every workflow declares `permissions:` at the top with `contents: read`.
Jobs that need more declare it themselves: the release `build` job
(tauri-action creates the release and uploads installers) and the release
`sbom` job (`gh release upload`) have `contents: write`; Pages has
`pages: write` and `id-token: write`. Signing secrets are referenced only in
the bundle job. Keep it that way: a new job starts read-only, and a new
secret is wired into the single step that needs it.

## Performance budgets

See [PERFORMANCE.md](PERFORMANCE.md): workloads, the measured baseline, the
budgets derived from it, and how to run the optional benchmark against a
real runtime.

## Maintenance checklist

- Contract fixtures: regenerate on every serialised-type change (CI fails
  otherwise). Review the JSON diff like code.
- `deny.toml`: when a new licence appears, add it to the allow list only if
  it is permissive; cargo-deny names the crate. Remove allow-list entries
  it reports as unused.
- Exceptions: re-check at the recorded date; remove when fixed upstream.
- Action pins: merge the weekly Dependabot pull request after CI passes;
  `--verify` in the audit workflow catches moved tags.
- Budgets: when a budget fails, run `cargo bench -p sandbox-core --bench perf`
  on the same code and on `main`, decide whether the algorithm regressed,
  and only then adjust the number together with the baseline table.
