//! Keeps a short history of resource samples for every running container.
//!
//! One background task per container consumes the adapter's native stats
//! stream, or polls `stats()` when there is none. `sync` reconciles tasks
//! with the current set of running containers after every overview load.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;

use crate::adapter::RuntimeAdapter;
use crate::model::ResourceSample;

/// Samples kept per container, roughly 90 seconds at one per second.
pub const HISTORY_LEN: usize = 90;
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// One adapter with its running containers as `(key, raw id)` pairs.
pub type StatsSet = (Arc<dyn RuntimeAdapter>, Vec<(String, String)>);

type History = Arc<Mutex<VecDeque<ResourceSample>>>;

struct Tracked {
    task: tokio::task::JoinHandle<()>,
    history: History,
    /// Identity of the adapter the task was started with.
    adapter_ptr: usize,
}

#[derive(Default)]
struct Inner {
    tracked: HashMap<String, Tracked>,
}

#[derive(Default)]
pub struct StatsHub {
    inner: Mutex<Inner>,
}

fn ptr_of(adapter: &Arc<dyn RuntimeAdapter>) -> usize {
    Arc::as_ptr(adapter).cast::<()>() as usize
}

async fn collect(adapter: Arc<dyn RuntimeAdapter>, id: String, history: History) {
    let push = |sample: ResourceSample| {
        let mut h = history.lock().expect("stats history poisoned");
        if h.len() >= HISTORY_LEN {
            h.pop_front();
        }
        h.push_back(sample);
    };
    match adapter.stats_stream(&id).await {
        Ok(Some(mut stream)) => {
            while let Some(Ok(sample)) = stream.next().await {
                push(sample);
            }
        }
        Ok(None) => {
            while let Ok(sample) = adapter.stats(&id).await {
                push(sample);
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
        Err(_) => {}
    }
}

impl StatsHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking new running containers, stop tracking gone ones, and
    /// restart tasks whose stream ended. Each container is tied to the adapter
    /// that listed it; a container that moved to another adapter is restarted.
    /// Adapters without stats support are skipped.
    pub fn sync(&self, sets: &[StatsSet]) {
        let mut inner = self.inner.lock().expect("stats hub poisoned");
        let wanted: Vec<(usize, &Arc<dyn RuntimeAdapter>, &String, &String)> = sets
            .iter()
            .filter(|(adapter, _)| adapter.capabilities().stats)
            .flat_map(|(adapter, pairs)| {
                pairs
                    .iter()
                    .map(move |(key, id)| (ptr_of(adapter), adapter, key, id))
            })
            .collect();
        inner.tracked.retain(|key, t| {
            let keep = wanted
                .iter()
                .any(|(ptr, _, wanted_key, _)| *wanted_key == key && *ptr == t.adapter_ptr)
                && !t.task.is_finished();
            if !keep {
                t.task.abort();
            }
            keep
        });
        for (ptr, adapter, key, id) in wanted {
            if inner.tracked.contains_key(key) {
                continue;
            }
            let history: History = Arc::default();
            let task = tokio::spawn(collect(adapter.clone(), id.clone(), history.clone()));
            inner.tracked.insert(
                key.clone(),
                Tracked {
                    task,
                    history,
                    adapter_ptr: ptr,
                },
            );
        }
    }

    /// Convenience for a single adapter whose keys equal the raw ids.
    pub fn sync_one(&self, adapter: &Arc<dyn RuntimeAdapter>, running_ids: &[String]) {
        let pairs = running_ids
            .iter()
            .map(|id| (id.clone(), id.clone()))
            .collect();
        self.sync(&[(adapter.clone(), pairs)]);
    }

    /// Stop all tracking, e.g. when the runtime went away.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().expect("stats hub poisoned");
        for (_, t) in inner.tracked.drain() {
            t.task.abort();
        }
    }

    /// Current history per container key, oldest first.
    pub fn snapshot(&self) -> HashMap<String, Vec<ResourceSample>> {
        let inner = self.inner.lock().expect("stats hub poisoned");
        inner
            .tracked
            .iter()
            .map(|(id, t)| {
                let h = t.history.lock().expect("stats history poisoned");
                (id.clone(), h.iter().cloned().collect())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockAdapter;

    #[tokio::test]
    async fn tracks_running_containers_and_drops_gone_ones() {
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let hub = StatsHub::new();
        hub.sync_one(
            &adapter,
            &["sandbox-web-shop".into(), "sandbox-web-shop-2".into()],
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        let snap = hub.snapshot();
        assert_eq!(snap.len(), 2);
        assert!(
            !snap["sandbox-web-shop"].is_empty(),
            "first sample arrives immediately"
        );

        hub.sync_one(&adapter, &["sandbox-web-shop".into()]);
        assert_eq!(hub.snapshot().len(), 1);

        let other: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::empty());
        hub.sync_one(&other, &["sandbox-web-shop".into()]);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap = hub.snapshot();
        assert_eq!(snap.len(), 1);
        assert!(
            snap["sandbox-web-shop"].is_empty(),
            "container moved to another adapter: fresh history"
        );
        hub.sync(&[]);
        assert!(hub.snapshot().is_empty());
    }

    #[tokio::test]
    async fn snapshot_is_keyed_by_container_key() {
        let hub = StatsHub::new();
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::demo());
        let id = "sandbox-web-shop".to_string();
        hub.sync(&[(adapter, vec![(format!("devbox/{id}"), id.clone())])]);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap = hub.snapshot();
        assert!(!snap[&format!("devbox/{id}")].is_empty());
    }
}
