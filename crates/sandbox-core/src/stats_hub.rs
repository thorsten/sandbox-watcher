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

type History = Arc<Mutex<VecDeque<ResourceSample>>>;

struct Tracked {
    task: tokio::task::JoinHandle<()>,
    history: History,
}

#[derive(Default)]
struct Inner {
    /// Identity of the adapter the tasks were started with.
    adapter_ptr: Option<usize>,
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
    /// restart tasks whose stream ended. A different adapter drops everything.
    pub fn sync(&self, adapter: &Arc<dyn RuntimeAdapter>, running_ids: &[String]) {
        let mut inner = self.inner.lock().expect("stats hub poisoned");
        let ptr = ptr_of(adapter);
        if inner.adapter_ptr != Some(ptr) {
            for (_, t) in inner.tracked.drain() {
                t.task.abort();
            }
            inner.adapter_ptr = Some(ptr);
        }
        inner.tracked.retain(|id, t| {
            let keep = running_ids.iter().any(|r| r == id);
            if !keep {
                t.task.abort();
            }
            keep
        });
        for id in running_ids {
            let restart = inner.tracked.get(id).is_some_and(|t| t.task.is_finished());
            if restart {
                inner.tracked.remove(id);
            }
            if inner.tracked.contains_key(id) {
                continue;
            }
            let history: History = Arc::default();
            let task = tokio::spawn(collect(adapter.clone(), id.clone(), history.clone()));
            inner.tracked.insert(id.clone(), Tracked { task, history });
        }
    }

    /// Stop all tracking, e.g. when the runtime went away.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().expect("stats hub poisoned");
        for (_, t) in inner.tracked.drain() {
            t.task.abort();
        }
        inner.adapter_ptr = None;
    }

    /// Current history per container id, oldest first.
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
        hub.sync(
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

        hub.sync(&adapter, &["sandbox-web-shop".into()]);
        assert_eq!(hub.snapshot().len(), 1);

        let other: Arc<dyn RuntimeAdapter> = Arc::new(MockAdapter::empty());
        hub.sync(&other, &[]);
        assert!(hub.snapshot().is_empty(), "new adapter drops old tasks");
    }
}
