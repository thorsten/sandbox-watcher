//! Counters for what the watcher costs the containers it watches.
//!
//! Every `exec` starts a process inside a container, plus `runc init` and
//! Docker's own work on the host, so the exec rate is the number to watch
//! when an idle sandbox shows CPU load. The adapters count here; the shell
//! reads the rate after each refresh and prints it when
//! `SANDBOX_WATCHER_DEBUG_EXECS` is set.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static EXECS: AtomicU64 = AtomicU64::new(0);

/// Environment variable that turns on the per-refresh exec log on stderr.
pub const DEBUG_EXECS_ENV: &str = "SANDBOX_WATCHER_DEBUG_EXECS";

/// Count one exec. Called by every adapter.
pub fn record_exec() {
    EXECS.fetch_add(1, Ordering::Relaxed);
}

/// Execs since the process started.
pub fn execs_total() -> u64 {
    EXECS.load(Ordering::Relaxed)
}

/// Execs per second between two readings.
#[derive(Debug, Clone)]
pub struct ExecRate {
    last_total: u64,
    last_at: Instant,
}

/// One reading of [`ExecRate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecSample {
    /// Execs since the previous reading.
    pub execs: u64,
    /// Time since the previous reading.
    pub elapsed: Duration,
}

impl ExecSample {
    pub fn per_second(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            0.0
        } else {
            self.execs as f64 / secs
        }
    }

    /// `per_second` over `containers` running containers, the number the
    /// budget in `tests/exec_budget.rs` is about.
    pub fn per_container_second(&self, containers: usize) -> f64 {
        if containers == 0 {
            0.0
        } else {
            self.per_second() / containers as f64
        }
    }
}

impl Default for ExecRate {
    fn default() -> Self {
        Self {
            last_total: execs_total(),
            last_at: Instant::now(),
        }
    }
}

impl ExecRate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Execs and time since the previous call; starts the next window.
    pub fn sample(&mut self) -> ExecSample {
        self.sample_at(execs_total(), Instant::now())
    }

    fn sample_at(&mut self, total: u64, now: Instant) -> ExecSample {
        let sample = ExecSample {
            execs: total.saturating_sub(self.last_total),
            elapsed: now.saturating_duration_since(self.last_at),
        };
        self.last_total = total;
        self.last_at = now;
        sample
    }
}

/// Whether the exec log is turned on.
pub fn debug_execs() -> bool {
    std::env::var(DEBUG_EXECS_ENV).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The stderr line printed after a refresh when the exec log is on.
pub fn describe(cycle: u64, sample: ExecSample, cycle_time: Duration, containers: usize) -> String {
    format!(
        "sandbox-watcher: refresh {cycle}: {} execs in {} ms; {:.2} exec/s, {:.2} per running container ({containers}) since the previous refresh",
        sample.execs,
        cycle_time.as_millis(),
        sample.per_second(),
        sample.per_container_second(containers),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_counts_execs_between_readings() {
        let start = Instant::now();
        let mut rate = ExecRate {
            last_total: 10,
            last_at: start,
        };
        let sample = rate.sample_at(30, start + Duration::from_secs(10));
        assert_eq!(sample.execs, 20);
        assert!((sample.per_second() - 2.0).abs() < 1e-9);
        assert!((sample.per_container_second(4) - 0.5).abs() < 1e-9);
        assert_eq!(sample.per_container_second(0), 0.0);
        // The window restarts.
        let next = rate.sample_at(30, start + Duration::from_secs(12));
        assert_eq!(next.execs, 0);
        assert_eq!(next.per_second(), 0.0);
    }

    #[test]
    fn counter_increments() {
        let before = execs_total();
        record_exec();
        record_exec();
        assert!(execs_total() >= before + 2);
    }

    #[test]
    fn describes_a_refresh() {
        let line = describe(
            7,
            ExecSample {
                execs: 3,
                elapsed: Duration::from_secs(10),
            },
            Duration::from_millis(180),
            3,
        );
        assert_eq!(
            line,
            "sandbox-watcher: refresh 7: 3 execs in 180 ms; 0.30 exec/s, 0.10 per running container (3) since the previous refresh"
        );
    }
}
