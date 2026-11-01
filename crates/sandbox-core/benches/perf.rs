//! Reproducible measurements of the watcher's hot paths on synthetic data:
//! growing transcripts, many containers, large histories. No runtime needed.
//!
//! ```sh
//! cargo bench -p sandbox-core --bench perf              # table on stdout
//! cargo bench -p sandbox-core --bench perf -- --json target/perf.json
//! cargo bench -p sandbox-core --bench perf -- --quick   # fewer iterations
//! ```
//!
//! Each case reports the median and the 90th percentile of its iterations
//! plus the counters that matter for the budget (chunk reads, bytes). The
//! budgets themselves live in `tests/budgets.rs`; `docs/PERFORMANCE.md`
//! records the measured baseline these budgets were derived from.

#[path = "scenarios.rs"]
mod scenarios;

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::transcripts::TranscriptCache;

struct Case {
    name: &'static str,
    /// Size parameter, for the table.
    scale: String,
    samples: Vec<Duration>,
    counters: Vec<(&'static str, u64)>,
}

impl Case {
    fn percentile(&self, p: f64) -> Duration {
        let mut sorted = self.samples.clone();
        sorted.sort();
        let index = ((sorted.len() - 1) as f64 * p).round() as usize;
        sorted[index]
    }
}

struct Options {
    iterations: usize,
    json: Option<String>,
}

fn options() -> Options {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = Options {
        iterations: 20,
        json: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--quick" => options.iterations = 5,
            "--json" => {
                options.json = args.get(i + 1).cloned();
                i += 1;
            }
            "--bench" | "--nocapture" => {}
            other if other.starts_with("--") => {}
            _ => {}
        }
        i += 1;
    }
    options
}

/// Time `body` `iterations` times after one warm-up run. `setup` builds a
/// fresh state for every run so the measurement never sees a warm cache
/// unless the case wants one.
fn measure<S, F, R>(
    name: &'static str,
    scale: String,
    iterations: usize,
    mut setup: impl FnMut() -> S,
    mut body: F,
) -> Case
where
    F: FnMut(&mut S) -> R,
{
    let mut state = setup();
    body(&mut state);
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let mut state = setup();
        let start = Instant::now();
        body(&mut state);
        samples.push(start.elapsed());
    }
    Case {
        name,
        scale,
        samples,
        counters: vec![],
    }
}

fn main() {
    let options = options();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let prices = scenarios::prices();
    let mut cases: Vec<Case> = Vec::new();

    // Transcript that has grown to N lines before the watcher first sees it:
    // the catch-up cost (at most 4 chunks of 4 MiB per refresh).
    for lines in [1_000usize, 10_000, 80_000] {
        let adapter = scenarios::transcript_adapter(lines);
        let dynamic: Arc<dyn RuntimeAdapter> = Arc::new(adapter.clone());
        let reads_before = adapter.chunk_reads();
        let mut case = measure(
            "transcript/first-sync",
            format!("{lines} lines"),
            options.iterations,
            TranscriptCache::default,
            |cache| runtime.block_on(scenarios::refresh_insights(&dynamic, cache, &prices)),
        );
        let reads = (adapter.chunk_reads() - reads_before) as u64 / (options.iterations as u64 + 1);
        case.counters.push(("chunk_reads", reads));
        cases.push(case);
    }

    // Steady state: the agent wrote 20 more lines since the last refresh.
    for lines in [1_000usize, 80_000] {
        let adapter = scenarios::transcript_adapter(lines);
        let dynamic: Arc<dyn RuntimeAdapter> = Arc::new(adapter.clone());
        let cache = TranscriptCache::default();
        // Catch up completely first.
        for _ in 0..8 {
            runtime.block_on(scenarios::refresh_insights(&dynamic, &cache, &prices));
        }
        let mut next = lines;
        let reads_before = adapter.chunk_reads();
        let mut case = measure(
            "transcript/incremental",
            format!("{lines} lines +20"),
            options.iterations,
            || {
                scenarios::grow_transcript(&adapter, next, 20);
                next += 20;
            },
            |_| runtime.block_on(scenarios::refresh_insights(&dynamic, &cache, &prices)),
        );
        let reads = (adapter.chunk_reads() - reads_before) as u64 / (options.iterations as u64 + 1);
        case.counters.push(("chunk_reads", reads));
        cases.push(case);
    }

    // The Activity panel: tail of the transcript parsed into events.
    for lines in [1_000usize, 80_000] {
        let adapter = scenarios::transcript_adapter(lines);
        let dynamic: Arc<dyn RuntimeAdapter> = Arc::new(adapter);
        let cache = TranscriptCache::default();
        runtime.block_on(scenarios::refresh_insights(&dynamic, &cache, &prices));
        cases.push(measure(
            "transcript/activity-panel",
            format!("{lines} lines"),
            options.iterations,
            || (),
            |_| runtime.block_on(scenarios::activity_panel(&dynamic, &cache, &prices)),
        ));
    }

    // Overview refresh with many running containers, each probed for
    // readiness and sessions.
    for count in [5usize, 50, 200] {
        let adapter = scenarios::containers_adapter(count);
        cases.push(measure(
            "containers/overview",
            format!("{count} running"),
            options.iterations,
            || (),
            |_| {
                let overview = runtime.block_on(scenarios::overview(&adapter));
                assert_eq!(overview.containers.len(), count);
            },
        ));
    }

    // History: a refresh folds the overview in (observe) and a query builds
    // the History view. Saving serialises the whole file.
    let now = Utc::now();
    for sessions in [1_000usize, 10_000, 50_000] {
        let overview = scenarios::synthetic_overview(50, now);
        let base = {
            let mut history = scenarios::history_with(sessions, now);
            history.observe(&overview, now);
            history
        };
        cases.push(measure(
            "history/observe",
            format!("{sessions} records, 50 containers"),
            options.iterations,
            || base.clone(),
            |history| history.observe(&overview, now),
        ));
        cases.push(measure(
            "history/query-all",
            format!("{sessions} records"),
            options.iterations,
            || (),
            |_| base.query(&scenarios::everything(now), now),
        ));
        cases.push(measure(
            "history/query-filtered",
            format!("{sessions} records"),
            options.iterations,
            || (),
            |_| base.query(&scenarios::last_day_one_project(now), now),
        ));
        let mut case = measure(
            "history/serialise",
            format!("{sessions} records"),
            options.iterations,
            || (),
            |_| serde_json::to_string(&base).unwrap().len(),
        );
        let bytes = serde_json::to_string(&base).unwrap().len() as u64;
        case.counters.push(("bytes", bytes));
        case.counters
            .push(("bytes_per_record", bytes / sessions as u64));
        cases.push(case);
    }

    report(&cases, options.json.as_deref());
}

fn report(cases: &[Case], json: Option<&str>) {
    println!(
        "{:<28} {:<30} {:>10} {:>10}  counters",
        "case", "scale", "median", "p90"
    );
    for case in cases {
        let counters: Vec<String> = case
            .counters
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        println!(
            "{:<28} {:<30} {:>10} {:>10}  {}",
            case.name,
            case.scale,
            fmt(case.percentile(0.5)),
            fmt(case.percentile(0.9)),
            counters.join(" ")
        );
    }
    if let Some(path) = json {
        let entries: Vec<serde_json::Value> = cases
            .iter()
            .map(|case| {
                serde_json::json!({
                    "case": case.name,
                    "scale": case.scale,
                    "median_us": case.percentile(0.5).as_micros(),
                    "p90_us": case.percentile(0.9).as_micros(),
                    "iterations": case.samples.len(),
                    "counters": case.counters.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
                })
            })
            .collect();
        let document = serde_json::json!({
            "generated_at": Utc::now().to_rfc3339(),
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "cases": entries,
        });
        // `cargo bench` runs with the crate directory as cwd; resolve
        // relative paths against the workspace root for convenience.
        let path = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path)
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create output directory");
        }
        std::fs::write(&path, serde_json::to_string_pretty(&document).unwrap())
            .expect("write json");
        let path = path.canonicalize().unwrap_or(path);
        println!("\nwritten to {}", path.display());
    }
}

fn fmt(d: Duration) -> String {
    let us = d.as_micros();
    if us >= 1_000_000 {
        format!("{:.2} s", d.as_secs_f64())
    } else if us >= 1_000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{us} µs")
    }
}
