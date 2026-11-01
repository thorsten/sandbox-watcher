//! Performance budgets. Three layers, from stable to optional:
//!
//! 1. **Data-volume budgets** run with every `cargo test`: they count chunk
//!    reads, bytes and records, which do not depend on the machine.
//! 2. **Wall-clock budgets** run when `SANDBOX_WATCHER_BUDGETS=1` is set,
//!    in CI's `perf` job with `--release` on one runner type. The limits
//!    are 6 to 20 times the baseline in `docs/PERFORMANCE.md`, so a budget
//!    failure means a regression of the algorithm, not a noisy runner.
//! 3. **Real runtime** runs when `SANDBOX_WATCHER_DOCKER_BENCH=1` is set
//!    and containers were prepared by `scripts/bench-runtime.sh`. It
//!    prints the measured overview and probe times and only checks that the
//!    refresh finishes inside the UI's poll interval.
//!
//! The workloads are shared with `benches/perf.rs`, which measures without
//! asserting; run that first when adjusting a budget and update the table
//! in `docs/PERFORMANCE.md` in the same change.

#[path = "../benches/scenarios.rs"]
mod scenarios;

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::transcripts::{CHUNK_BYTES, TranscriptCache};

// ---------------------------------------------------------------------------
// Budgets. Keep in sync with docs/PERFORMANCE.md.

/// Chunk reads one refresh may spend on one transcript (`MAX_CHUNKS_PER_SYNC`).
const CHUNKS_PER_REFRESH: usize = 4;
/// Bytes of history file per session record, including usage and files.
const HISTORY_BYTES_PER_RECORD: usize = 1024;
/// Transcript size at which the budgets are measured: about 25 MB, more
/// than one refresh reads, so catch-up spans several refreshes.
const LARGE_TRANSCRIPT_LINES: usize = 80_000;
/// Containers the overview budget is measured with.
const MANY_CONTAINERS: usize = 200;
/// Session records the history budgets are measured with: about a year of
/// heavy use at 30 sessions a day.
const LARGE_HISTORY: usize = 10_000;

/// Wall-clock budgets (release profile), median of [`RUNS`] runs.
const FIRST_SYNC_LARGE: Duration = Duration::from_millis(1500);
const INCREMENTAL_LARGE: Duration = Duration::from_millis(100);
const ACTIVITY_PANEL_LARGE: Duration = Duration::from_millis(1000);
const OVERVIEW_MANY: Duration = Duration::from_millis(50);
const HISTORY_OBSERVE_LARGE: Duration = Duration::from_millis(100);
const HISTORY_QUERY_LARGE: Duration = Duration::from_millis(100);
const HISTORY_SERIALISE_LARGE: Duration = Duration::from_millis(100);
const RUNS: usize = 5;

fn enabled(var: &str) -> bool {
    std::env::var(var).ok().as_deref() == Some("1")
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// Median wall-clock of `body` over [`RUNS`] runs after one warm-up.
fn timed<R>(mut body: impl FnMut() -> R) -> Duration {
    body();
    let samples = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            body();
            start.elapsed()
        })
        .collect();
    median(samples)
}

fn within(name: &str, measured: Duration, budget: Duration) {
    eprintln!("{name}: {measured:?} (budget {budget:?})");
    assert!(
        measured <= budget,
        "{name} took {measured:?}, budget is {budget:?}; see docs/PERFORMANCE.md"
    );
}

// ---------------------------------------------------------------------------
// 1. Data volume, always on.

#[tokio::test]
async fn a_large_transcript_is_caught_up_in_bounded_chunks() {
    let adapter = scenarios::transcript_adapter(LARGE_TRANSCRIPT_LINES);
    let dynamic: Arc<dyn RuntimeAdapter> = Arc::new(adapter.clone());
    let cache = TranscriptCache::default();
    let prices = scenarios::prices();

    let insights = scenarios::refresh_insights(&dynamic, &cache, &prices).await;
    let coverage = insights[0].coverage;
    assert_eq!(
        adapter.chunk_reads(),
        CHUNKS_PER_REFRESH,
        "first refresh reads exactly the chunk budget"
    );
    assert!(
        !coverage.complete && coverage.pending_bytes > 0,
        "the rest waits for the next refresh: {coverage:?}"
    );
    assert!(
        coverage.pending_bytes < 3 * CHUNK_BYTES as u64,
        "80k lines are about 25 MB and 16 MiB were read: {coverage:?}"
    );

    let mut refreshes = 1;
    let usage = loop {
        let insights = scenarios::refresh_insights(&dynamic, &cache, &prices).await;
        refreshes += 1;
        if insights[0].coverage.complete {
            break insights[0].usage.clone().expect("usage once complete");
        }
        assert!(refreshes <= 3, "catch-up did not converge");
    };
    assert!(adapter.chunk_reads() <= 2 * CHUNKS_PER_REFRESH);
    // 2 demo assistant lines plus one per appended call line.
    assert_eq!(usage.turns as usize, 2 + LARGE_TRANSCRIPT_LINES / 2);
}

#[tokio::test]
async fn an_appended_line_costs_one_read_and_an_idle_refresh_none() {
    let adapter = scenarios::transcript_adapter(LARGE_TRANSCRIPT_LINES);
    let dynamic: Arc<dyn RuntimeAdapter> = Arc::new(adapter.clone());
    let cache = TranscriptCache::default();
    let prices = scenarios::prices();
    for _ in 0..4 {
        scenarios::refresh_insights(&dynamic, &cache, &prices).await;
    }
    let before = scenarios::refresh_insights(&dynamic, &cache, &prices).await;
    assert!(before[0].coverage.complete);
    let reads = adapter.chunk_reads();

    scenarios::grow_transcript(&adapter, LARGE_TRANSCRIPT_LINES, 20);
    let after = scenarios::refresh_insights(&dynamic, &cache, &prices).await;
    assert_eq!(
        adapter.chunk_reads(),
        reads + 1,
        "one chunk for 20 new lines"
    );
    assert_eq!(
        after[0].usage.as_ref().unwrap().turns,
        before[0].usage.as_ref().unwrap().turns + 10,
        "only the new lines are added"
    );

    scenarios::refresh_insights(&dynamic, &cache, &prices).await;
    assert_eq!(
        adapter.chunk_reads(),
        reads + 1,
        "nothing new, nothing read"
    );
}

#[test]
fn history_stays_under_a_kilobyte_per_record() {
    let now = Utc::now();
    let history = scenarios::history_with(LARGE_HISTORY, now);
    let bytes = serde_json::to_string(&history).unwrap().len();
    let per_record = bytes / LARGE_HISTORY;
    eprintln!("history: {bytes} bytes for {LARGE_HISTORY} records, {per_record} per record");
    assert!(
        per_record <= HISTORY_BYTES_PER_RECORD,
        "{per_record} bytes per record, budget {HISTORY_BYTES_PER_RECORD}"
    );
}

#[tokio::test]
async fn many_containers_all_appear_once() {
    let adapter = scenarios::containers_adapter(MANY_CONTAINERS);
    let overview = scenarios::overview(&adapter).await;
    assert_eq!(overview.containers.len(), MANY_CONTAINERS);
    let mut keys: Vec<&str> = overview.containers.iter().map(|c| c.key.as_str()).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), MANY_CONTAINERS);
    assert!(overview.containers.iter().all(|c| !c.sessions.is_empty()));
}

// ---------------------------------------------------------------------------
// 2. Wall clock, opt-in.

#[test]
fn wall_clock_budgets() {
    if !enabled("SANDBOX_WATCHER_BUDGETS") {
        eprintln!("skipped: set SANDBOX_WATCHER_BUDGETS=1 (and build with --release)");
        return;
    }
    if cfg!(debug_assertions) {
        panic!("wall-clock budgets are defined for the release profile; run with --release");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let prices = scenarios::prices();

    let adapter = scenarios::transcript_adapter(LARGE_TRANSCRIPT_LINES);
    let dynamic: Arc<dyn RuntimeAdapter> = Arc::new(adapter.clone());
    within(
        "transcript/first-sync 80k lines",
        timed(|| {
            let cache = TranscriptCache::default();
            runtime.block_on(scenarios::refresh_insights(&dynamic, &cache, &prices))
        }),
        FIRST_SYNC_LARGE,
    );

    let cache = TranscriptCache::default();
    for _ in 0..8 {
        runtime.block_on(scenarios::refresh_insights(&dynamic, &cache, &prices));
    }
    let mut next = LARGE_TRANSCRIPT_LINES;
    within(
        "transcript/incremental 80k lines +20",
        timed(|| {
            scenarios::grow_transcript(&adapter, next, 20);
            next += 20;
            runtime.block_on(scenarios::refresh_insights(&dynamic, &cache, &prices))
        }),
        INCREMENTAL_LARGE,
    );
    within(
        "transcript/activity-panel 80k lines",
        timed(|| runtime.block_on(scenarios::activity_panel(&dynamic, &cache, &prices))),
        ACTIVITY_PANEL_LARGE,
    );

    let adapter = scenarios::containers_adapter(MANY_CONTAINERS);
    within(
        "containers/overview 200 running",
        timed(|| runtime.block_on(scenarios::overview(&adapter))),
        OVERVIEW_MANY,
    );

    let now = Utc::now();
    let overview = scenarios::synthetic_overview(50, now);
    let mut history = scenarios::history_with(LARGE_HISTORY, now);
    history.observe(&overview, now);
    within(
        "history/observe 10k records, 50 containers",
        timed(|| history.clone().observe(&overview, now)),
        HISTORY_OBSERVE_LARGE,
    );
    within(
        "history/query-all 10k records",
        timed(|| history.query(&scenarios::everything(now), now)),
        HISTORY_QUERY_LARGE,
    );
    within(
        "history/serialise 10k records",
        timed(|| serde_json::to_string(&history).unwrap().len()),
        HISTORY_SERIALISE_LARGE,
    );
}

// ---------------------------------------------------------------------------
// 3. Real runtime, opt-in. Containers come from scripts/bench-runtime.sh.

#[tokio::test]
async fn real_runtime_refresh() {
    if !enabled("SANDBOX_WATCHER_DOCKER_BENCH") {
        eprintln!(
            "skipped: run scripts/bench-runtime.sh, which sets SANDBOX_WATCHER_DOCKER_BENCH=1"
        );
        return;
    }
    use sandbox_core::docker::DockerAdapter;
    use sandbox_core::model::RuntimeKind;
    use sandbox_core::overview::{DataSource, HostedAdapter, load_many};

    let kind = match std::env::var("SANDBOX_WATCHER_RUNTIME").as_deref() {
        Ok("podman") => RuntimeKind::Podman,
        _ => RuntimeKind::Docker,
    };
    let endpoint = std::env::var("DOCKER_HOST").unwrap_or_else(|_| "/var/run/docker.sock".into());
    let adapter: Arc<dyn RuntimeAdapter> =
        Arc::new(DockerAdapter::connect(kind, &endpoint).expect("connect"));
    let hosted = [HostedAdapter::local(adapter.clone())];
    let expected: usize = std::env::var("SANDBOX_WATCHER_BENCH_CONTAINERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // Warm-up, then five refreshes like the watcher's poll loop.
    let overview = load_many(&hosted, DataSource::Live, vec![]).await;
    let running = overview
        .containers
        .iter()
        .filter(|c| c.container.state.is_running())
        .count();
    if expected > 0 {
        assert!(
            running >= expected,
            "expected at least {expected} running bench containers, found {running}"
        );
    }
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let overview = load_many(&hosted, DataSource::Live, vec![]).await;
        samples.push(start.elapsed());
        assert!(!overview.containers.is_empty());
    }
    let refresh = median(samples);
    eprintln!(
        "real runtime ({kind}): {running} running containers, overview refresh median {refresh:?}"
    );
    // The default poll interval is the only hard limit: a refresh that takes
    // longer than that would queue up behind itself.
    assert!(
        refresh < Duration::from_secs(5),
        "overview refresh took {refresh:?} for {running} containers"
    );
}
