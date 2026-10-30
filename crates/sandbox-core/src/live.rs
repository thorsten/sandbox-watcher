//! Resolve and cache the adapter for the runtime found on this machine, and
//! forward its lifecycle events to a listener.

use std::sync::{Arc, Mutex};

use futures::StreamExt;

use crate::adapter::RuntimeAdapter;
use crate::detect::{DetectedRuntime, Environment, detect};
use crate::docker::DockerAdapter;
use crate::model::{RuntimeEvent, RuntimeEventKind, RuntimeKind};
use crate::overview::{DataSource, Overview, RuntimeStatus, load};
use crate::sandbox::PROBE_SENTINEL;

/// Receives relevant runtime events, see [`is_relevant`].
pub type Listener = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

/// Holds the connected adapter between refreshes. Reconnects after failures.
#[derive(Default)]
pub struct LiveRuntime {
    adapter: Mutex<Option<Arc<dyn RuntimeAdapter>>>,
    listener: Mutex<Option<Listener>>,
    events_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Events worth a UI refresh. Our own probe execs and the noise around them
/// (`exec_start`, `exec_die`, `resize`, ...) are dropped so a refresh never
/// triggers another refresh.
pub fn is_relevant(event: &RuntimeEvent) -> bool {
    match &event.kind {
        RuntimeEventKind::Created
        | RuntimeEventKind::Started
        | RuntimeEventKind::Stopped
        | RuntimeEventKind::Died
        | RuntimeEventKind::Removed => true,
        RuntimeEventKind::ExecCreated(command) => !command.contains(PROBE_SENTINEL),
        RuntimeEventKind::Other(_) => false,
    }
}

impl LiveRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the event sink. Takes effect on the next (re)connect.
    pub fn set_listener(&self, listener: Listener) {
        *self.listener.lock().expect("listener poisoned") = Some(listener);
    }

    /// Pick the first candidate we have an adapter for.
    pub fn connect(candidates: &[DetectedRuntime]) -> Option<Arc<dyn RuntimeAdapter>> {
        candidates.iter().find_map(|c| match c.kind {
            RuntimeKind::Docker | RuntimeKind::Podman => {
                DockerAdapter::connect(c.kind, &c.endpoint)
                    .ok()
                    .map(|a| Arc::new(a) as Arc<dyn RuntimeAdapter>)
            }
            // Apple `container` adapter arrives in M5.
            RuntimeKind::AppleContainer => None,
        })
    }

    fn cached(&self) -> Option<Arc<dyn RuntimeAdapter>> {
        self.adapter.lock().expect("live adapter poisoned").clone()
    }

    fn set(self: &Arc<Self>, adapter: Option<Arc<dyn RuntimeAdapter>>) {
        let changed = {
            let mut current = self.adapter.lock().expect("live adapter poisoned");
            let same = match (&*current, &adapter) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            *current = adapter.clone();
            !same
        };
        if changed {
            self.restart_events(adapter);
        }
    }

    fn restart_events(self: &Arc<Self>, adapter: Option<Arc<dyn RuntimeAdapter>>) {
        if let Some(task) = self
            .events_task
            .lock()
            .expect("events task poisoned")
            .take()
        {
            task.abort();
        }
        let Some(adapter) = adapter else { return };
        let Some(listener) = self.listener.lock().expect("listener poisoned").clone() else {
            return;
        };
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            if let Ok(mut stream) = adapter.events().await {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(event) if is_relevant(&event) => listener(event),
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            // The stream ended: the daemon went away or restarted. Drop the
            // cached adapter so the next refresh reconnects and resubscribes.
            if let Some(live) = weak.upgrade() {
                *live.adapter.lock().expect("live adapter poisoned") = None;
            }
        });
        *self.events_task.lock().expect("events task poisoned") = Some(task);
    }

    /// Detect, connect if needed, and load the overview.
    pub async fn overview(self: &Arc<Self>) -> Overview {
        let candidates = detect(&Environment::current());
        if candidates.is_empty() {
            self.set(None);
            return Overview::not_detected(DataSource::Live);
        }
        let adapter = match self.cached().or_else(|| Self::connect(&candidates)) {
            Some(adapter) => adapter,
            None => return Overview::detected_only(candidates),
        };
        let overview = load(adapter.clone(), DataSource::Live, candidates).await;
        match overview.runtime {
            RuntimeStatus::Connected { .. } => self.set(Some(adapter)),
            _ => self.set(None),
        }
        overview
    }

    /// Use a specific adapter instead of detecting one. For tests and demos.
    pub fn with_adapter(self: &Arc<Self>, adapter: Arc<dyn RuntimeAdapter>) {
        self.set(Some(adapter));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::Utc;
    use tokio::sync::mpsc;

    use super::*;
    use crate::mock::MockAdapter;

    fn event(kind: RuntimeEventKind) -> RuntimeEvent {
        RuntimeEvent {
            at: Utc::now(),
            container_id: "c".into(),
            container_name: None,
            kind,
        }
    }

    #[test]
    fn filters_probe_and_noise_events() {
        assert!(is_relevant(&event(RuntimeEventKind::Started)));
        assert!(is_relevant(&event(RuntimeEventKind::ExecCreated(
            "sh -c zsh".into()
        ))));
        assert!(!is_relevant(&event(RuntimeEventKind::ExecCreated(
            format!("sh -c if [ -f ... ] {PROBE_SENTINEL}")
        ))));
        assert!(!is_relevant(&event(RuntimeEventKind::Other(
            "exec_start".into()
        ))));
    }

    #[tokio::test]
    async fn forwards_adapter_events_to_listener() {
        let live = Arc::new(LiveRuntime::new());
        let (tx, mut rx) = mpsc::unbounded_channel();
        live.set_listener(Arc::new(move |event| {
            let _ = tx.send(event);
        }));
        let mock = MockAdapter::demo();
        live.with_adapter(Arc::new(mock.clone()));
        // The forwarding task subscribes asynchronously; keep emitting until it listens.
        let received = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                mock.emit(event(RuntimeEventKind::Other("resize".into())));
                mock.emit(event(RuntimeEventKind::Started));
                tokio::select! {
                    received = rx.recv() => return received.unwrap(),
                    () = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
            }
        })
        .await
        .expect("listener received an event");
        assert_eq!(
            received.kind,
            RuntimeEventKind::Started,
            "noise event was filtered"
        );
    }
}
