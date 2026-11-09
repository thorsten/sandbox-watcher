//! The work of one background refresh and when the next one may start.
//!
//! The shell's loop calls [`Cadence`] to pace cycles and the two collectors
//! here for the exec-heavy steps after the overview: transcript insights for
//! the history and the network logs. Keeping them in core means a test can
//! run a whole cycle against the mock and count the execs it costs
//! (`tests/exec_budget.rs`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::activity::{AgentInsight, collect_insights};
use crate::adapter::RuntimeAdapter;
use crate::network::{NetworkReport, collect};
use crate::overview::{ContainerOverview, HostedAdapter, Overview, within_budget};
use crate::transcripts::TranscriptCache;
use crate::usage::PriceTable;

/// Shortest gap between the starts of two cycles, whatever triggers them.
/// Runtime events arrive in bursts (a `docker exec` makes several); without
/// a floor each burst would start a cycle, and a cycle's own execs would
/// start the next.
pub const MIN_CYCLE_GAP: Duration = Duration::from_secs(2);

/// Decides when the next cycle may start and what it reads.
#[derive(Debug, Clone)]
pub struct Cadence {
    min_gap: Duration,
    network_every: Duration,
    network_at: Option<Instant>,
}

impl Cadence {
    pub fn new(min_gap: Duration, network_every: Duration) -> Self {
        Self {
            min_gap,
            network_every,
            network_at: None,
        }
    }

    /// Change how often the network logs are read. Takes effect at once.
    pub fn set_network_interval(&mut self, every: Duration) {
        self.network_every = every;
    }

    /// How long a cycle asked for by an event has to wait, given when the
    /// current cycle started: the rest of [`Self::min_gap`], if any.
    pub fn delay_for_trigger(&self, cycle_started: Instant, now: Instant) -> Duration {
        (cycle_started + self.min_gap).saturating_duration_since(now)
    }

    /// Whether the cycle starting at `now` reads the network logs: when the
    /// user asked (`force`), the first time, or once `network_every` has
    /// passed since the last read. Records the read when it says yes.
    pub fn network_due(&mut self, now: Instant, force: bool) -> bool {
        let due = force
            || self
                .network_at
                .is_none_or(|at| now.saturating_duration_since(at) >= self.network_every);
        if due {
            self.network_at = Some(now);
        }
        due
    }
}

/// Finds the adapter and raw container id for a container key.
pub type Resolve<'a> = dyn Fn(&str) -> Option<(Arc<dyn RuntimeAdapter>, String)> + Sync + 'a;

/// Containers worth reading from: running, reachable, with a session.
fn live_with_sessions(overview: &Overview) -> impl Iterator<Item = &ContainerOverview> {
    overview
        .containers
        .iter()
        .filter(|c| c.container.state.is_running() && c.last_seen_at.is_none())
        .filter(|c| !c.sessions.is_empty())
}

/// Transcript insights for every live container with sessions, from the
/// discovery the probe already delivered. Containers run at the same time;
/// on a remote host each gets `REMOTE_TIMEOUT`.
pub async fn collect_all_insights(
    overview: &Overview,
    resolve: &Resolve<'_>,
    cache: &TranscriptCache,
    prices: &PriceTable,
) -> Vec<(String, Vec<AgentInsight>)> {
    let jobs = live_with_sessions(overview).filter_map(|c| {
        let (adapter, id) = resolve(&c.key)?;
        Some(async move {
            let insights = within_budget(
                &c.host,
                collect_insights(&adapter, &id, &c.key, &c.discovery, cache, prices),
            )
            .await
            .unwrap_or_default();
            (c.key.clone(), insights)
        })
    });
    futures::future::join_all(jobs).await
}

/// Network logs of every live container the overview lists, one exec each,
/// keyed by container key. Remote hosts are bounded by `REMOTE_TIMEOUT`.
pub async fn collect_all_network(
    adapters: &[HostedAdapter],
    overview: &Overview,
    now: DateTime<Utc>,
) -> Vec<NetworkReport> {
    let collections = adapters.iter().map(|hosted| async move {
        let kind = hosted.adapter.kind();
        let containers = overview
            .containers
            .iter()
            .filter(|c| c.host == hosted.host && c.runtime == kind)
            .filter(|c| c.container.state.is_running() && c.last_seen_at.is_none());
        let collect_all = futures::future::join_all(containers.map(|c| async move {
            let mut report = collect(&hosted.adapter, &c.container, now).await;
            report.container_id = c.key.clone();
            report
        }));
        within_budget(&hosted.host, collect_all)
            .await
            .unwrap_or_default()
    });
    futures::future::join_all(collections)
        .await
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggered_cycles_keep_the_minimum_gap() {
        let cadence = Cadence::new(Duration::from_secs(2), Duration::from_secs(10));
        let started = Instant::now();
        // A fast cycle: the trigger waits for the rest of the gap.
        assert_eq!(
            cadence.delay_for_trigger(started, started + Duration::from_millis(500)),
            Duration::from_millis(1500)
        );
        // A slow cycle already used the gap up: no extra wait.
        assert_eq!(
            cadence.delay_for_trigger(started, started + Duration::from_secs(3)),
            Duration::ZERO
        );
    }

    #[test]
    fn network_logs_are_read_on_their_own_interval_or_on_request() {
        let mut cadence = Cadence::new(Duration::from_secs(2), Duration::from_secs(10));
        let t0 = Instant::now();
        assert!(cadence.network_due(t0, false), "first cycle reads them");
        assert!(!cadence.network_due(t0 + Duration::from_secs(2), false));
        assert!(!cadence.network_due(t0 + Duration::from_secs(9), false));
        assert!(cadence.network_due(t0 + Duration::from_secs(10), false));
        assert!(!cadence.network_due(t0 + Duration::from_secs(12), false));
        assert!(
            cadence.network_due(t0 + Duration::from_secs(13), true),
            "the user's refresh reads them at once"
        );
        assert!(
            !cadence.network_due(t0 + Duration::from_secs(20), false),
            "and restarts the interval"
        );
        cadence.set_network_interval(Duration::from_secs(2));
        assert!(cadence.network_due(t0 + Duration::from_secs(20), false));
    }
}
