# Performance: workloads, baseline, budgets

The watcher's cost grows along three axes: transcripts that keep growing
while an agent works, the number of running containers, and the size of the
session history. `crates/sandbox-core/benches/scenarios.rs` builds
deterministic workloads for each; `benches/perf.rs` measures them and
`tests/budgets.rs` asserts budgets. All of it runs against `MockAdapter`,
so the numbers describe the watcher's own work. The optional real-runtime
run is at the end.

## Run

```sh
cargo bench -p sandbox-core --bench perf                     # table
cargo bench -p sandbox-core --bench perf -- --json target/perf.json
cargo bench -p sandbox-core --bench perf -- --quick          # 5 iterations instead of 20

cargo test -p sandbox-core --test budgets                    # data-volume budgets (always on)
SANDBOX_WATCHER_BUDGETS=1 cargo test --release -p sandbox-core --test budgets -- --nocapture
```

The `perf` job in `.github/workflows/ci.yml` runs the budgets in release
mode on `ubuntu-latest` and uploads `perf.json` (90 days) for every commit,
so a trend can be read from the artifacts without a dashboard.

## Workloads

| Case                        | What it does                                                                                       |
| --------------------------- | -------------------------------------------------------------------------------------------------- |
| `transcript/first-sync`     | A Claude Code transcript of N lines exists before the watcher starts; one `collect_insights`.      |
| `transcript/incremental`    | The transcript is caught up; the agent appends 20 lines; one `collect_insights`.                   |
| `transcript/activity-panel` | `activity::collect` for the Activity panel (processes, events, usage, files, git) on a warm cache. |
| `containers/overview`       | `load_many` over N running containers, each probed for readiness and sessions.                     |
| `history/observe`           | One refresh folds a 50-container overview into a history of N records.                             |
| `history/query-all`         | `History::query` over a year, no filter.                                                           |
| `history/query-filtered`    | `History::query` for one day, one project, one agent.                                              |
| `history/serialise`         | `serde_json::to_string(&history)`, what every save writes.                                         |

Transcript lines alternate an assistant tool call with usage (about 430
bytes) and its result (about 170 bytes); 80 000 lines are about 25 MB.
History records are spread over 90 days and 40 projects; every fourth one
carries usage and three files.

## Baseline (2026-11-01)

Release profile, median of 20 iterations and 90th percentile. Linux
aarch64 development container, 10 cores, rustc 1.99.1. CI runners differ;
the budgets below leave room for that.

| Case                        | Scale                        | median   | p90      | Counters                         |
| --------------------------- | ---------------------------- | -------- | -------- | -------------------------------- |
| `transcript/first-sync`     | 1 000 lines                  | 2.0 ms   | 2.0 ms   | 1 chunk read                     |
| `transcript/first-sync`     | 10 000 lines                 | 34.5 ms  | 53.4 ms  | 1 chunk read                     |
| `transcript/first-sync`     | 80 000 lines                 | 113.7 ms | 190.3 ms | 4 chunk reads (16 MiB)           |
| `transcript/incremental`    | 1 000 lines, +20             | 167 µs   | 183 µs   | 1 chunk read                     |
| `transcript/incremental`    | 80 000 lines, +20            | 10.9 ms  | 14.1 ms  | 1 chunk read                     |
| `transcript/activity-panel` | 1 000 lines                  | 1.3 ms   | 1.4 ms   |                                  |
| `transcript/activity-panel` | 80 000 lines                 | 114.1 ms | 173.0 ms |                                  |
| `containers/overview`       | 5 running                    | 4 µs     | 5 µs     |                                  |
| `containers/overview`       | 50 running                   | 55 µs    | 58 µs    |                                  |
| `containers/overview`       | 200 running                  | 245 µs   | 263 µs   |                                  |
| `history/observe`           | 1 000 records, 50 containers | 382 µs   | 453 µs   |                                  |
| `history/observe`           | 10 000 records               | 3.5 ms   | 3.6 ms   |                                  |
| `history/observe`           | 50 000 records               | 107.5 ms | 186.3 ms |                                  |
| `history/query-all`         | 1 000 records                | 409 µs   | 451 µs   |                                  |
| `history/query-all`         | 10 000 records               | 4.2 ms   | 5.9 ms   |                                  |
| `history/query-all`         | 50 000 records               | 42.3 ms  | 60.1 ms  |                                  |
| `history/query-filtered`    | 10 000 records               | 368 µs   | 405 µs   |                                  |
| `history/serialise`         | 1 000 records                | 1.1 ms   | 1.1 ms   | 795 689 bytes, 795 per record    |
| `history/serialise`         | 10 000 records               | 9.6 ms   | 10.9 ms  | 7 410 939 bytes, 741 per record  |
| `history/serialise`         | 50 000 records               | 62.1 ms  | 85.2 ms  | 36 820 840 bytes, 736 per record |

What the baseline says:

- **Transcripts are bounded by design.** A refresh reads at most four
  4 MiB chunks of one transcript (`MAX_CHUNKS_PER_SYNC`); a 25 MB backlog
  is caught up in two refreshes, after which every refresh reads only the
  appended bytes. The incremental refresh on a long transcript still costs
  about 11 ms because usage and files are re-aggregated from the cached
  per-line events, not because the file is re-read.
- **Containers are cheap.** The mock answers instantly, so the overview
  numbers measure the watcher's bookkeeping only: 200 containers cost well
  under a millisecond. Against a real runtime the probes dominate; see the
  real-runtime run below.
- **History is linear in the record count, per container per refresh.**
  `History::observe` looks up each running container and session by scanning
  the record vectors, so a refresh costs `containers × sessions × records`.
  At 10 000 records (about a year at 30 sessions a day) that is 3.5 ms; at
  50 000 it is over 100 ms per refresh, and the file written on every save is
  37 MB. Retention (`history_retention_days`, default pruning) keeps real
  histories far below that. If histories ever grow past 10 000 records, index
  the open records by key before raising the budget.

## Budgets

Two kinds. Data-volume budgets count things and run in every
`cargo test -p sandbox-core`; wall-clock budgets run in release mode when
`SANDBOX_WATCHER_BUDGETS=1` is set, as the median of 5 runs, and are set at
6 to 20 times the baseline median so that only an algorithmic regression
trips them, not a slow runner. The constants live at the top of
`tests/budgets.rs`; change them together with this table.

| Budget                                   | Limit                          | Baseline | Headroom | Why this limit                                                       |
| ---------------------------------------- | ------------------------------ | -------- | -------- | -------------------------------------------------------------------- |
| Chunk reads per transcript and refresh   | exactly 4 on a 25 MB backlog   | 4        |          | The contract of `TranscriptCache`; more would stall a refresh.       |
| Catch-up of a 25 MB backlog              | complete within 3 refreshes    | 2        |          | 16 MiB per refresh.                                                  |
| Appended lines                           | 1 chunk read; idle refresh 0   | 1 / 0    |          | Incremental reading is the point of the cache.                       |
| History file size                        | ≤ 1 024 bytes per record       | 741      | 1.4×     | Keeps a year of history under 10 MB; fails if a record grows a blob. |
| 200 running containers                   | all listed once, with sessions |          |          | Correctness at scale, not speed.                                     |
| `transcript/first-sync` 80 000 lines     | 1 500 ms                       | 140 ms   | 10×      | One refresh must stay well inside the 5 s default poll interval.     |
| `transcript/incremental` 80 000 lines    | 100 ms                         | 14 ms    | 7×       | Per container per refresh; 10 containers must fit in a second.       |
| `transcript/activity-panel` 80 000 lines | 1 000 ms                       | 130 ms   | 8×       | Interactive: the panel polls while open.                             |
| `containers/overview` 200 running        | 50 ms                          | 0.3 ms   | >100×    | Bookkeeping only; a regression here is a quadratic sort or clone.    |
| `history/observe` 10 000 records         | 100 ms                         | 5.4 ms   | 18×      | Runs on every refresh while history is enabled.                      |
| `history/query-all` 10 000 records       | 100 ms                         | 4.9 ms   | 20×      | Interactive: the History panel.                                      |
| `history/serialise` 10 000 records       | 100 ms                         | 10 ms    | 10×      | Runs on every save, on the refresh task.                             |

Baseline values in this table are the release-mode medians the budget test
printed on the machine above; they differ slightly from the 20-iteration
bench run.

## Real runtime (optional, not in CI)

`scripts/bench-runtime.sh [count]` starts `count` (default 20) Alpine
containers labelled like sandbox's, with the ready marker, runs the
`real_runtime_refresh` test in `tests/budgets.rs` against the local Docker
(or Podman with `SANDBOX_WATCHER_RUNTIME=podman` and `DOCKER_HOST`), prints
the median overview refresh over five rounds, and removes the containers.
The only assertion is that a refresh finishes within the 5 s default poll
interval; the measured time depends on the machine and the runtime and
belongs in the pull request description, not in this file. Not run yet on a
real machine as of 2026-11-01.
