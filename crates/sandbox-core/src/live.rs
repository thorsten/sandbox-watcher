//! Detect runtimes, keep one adapter per usable runtime between refreshes,
//! and forward lifecycle events to a listener.
//!
//! Selection follows sandbox: a `runtime` set in sandbox's user config (or
//! `SANDBOX_WATCHER_RUNTIME`) pins one runtime; otherwise every detected
//! runtime is used and containers from all of them are shown together.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;

use crate::adapter::RuntimeAdapter;
use crate::apple::AppleContainerAdapter;
use crate::detect::{DetectedRuntime, Environment, detect};
use crate::docker::DockerAdapter;
use crate::endpoints;
use crate::model::{RuntimeEvent, RuntimeEventKind, RuntimeKind};
use crate::network::DIAGNOSTIC_COMMAND_PREFIX;
use crate::overview::{DataSource, Overview, RuntimeStatus, load_many};
use crate::sandbox::PROBE_SENTINEL;

/// Receives relevant runtime events, see [`is_relevant`].
pub type Listener = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

#[derive(Default)]
struct Connections {
    adapters: HashMap<RuntimeKind, Arc<dyn RuntimeAdapter>>,
    by_container: HashMap<String, Arc<dyn RuntimeAdapter>>,
    event_tasks: HashMap<RuntimeKind, tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
pub struct LiveRuntime {
    state: Mutex<Connections>,
    listener: Mutex<Option<Listener>>,
}

/// Events worth a UI refresh. Our own probe and diagnostic execs and the
/// noise around them (`exec_start`, `exec_die`, `resize`, ...) are dropped so
/// a refresh never triggers another refresh.
pub fn is_relevant(event: &RuntimeEvent) -> bool {
    match &event.kind {
        RuntimeEventKind::Created
        | RuntimeEventKind::Started
        | RuntimeEventKind::Stopped
        | RuntimeEventKind::Died
        | RuntimeEventKind::Removed => true,
        RuntimeEventKind::ExecCreated(command) => {
            !command.contains(PROBE_SENTINEL) && !command.contains(DIAGNOSTIC_COMMAND_PREFIX)
        }
        RuntimeEventKind::Other(_) => false,
    }
}

/// Runtime pinned by `SANDBOX_WATCHER_RUNTIME` or sandbox's `runtime` setting.
pub fn preferred_runtime() -> Option<RuntimeKind> {
    if let Ok(value) = std::env::var("SANDBOX_WATCHER_RUNTIME") {
        return match value.trim() {
            "docker" => Some(RuntimeKind::Docker),
            "podman" => Some(RuntimeKind::Podman),
            "apple-container" | "apple" => Some(RuntimeKind::AppleContainer),
            _ => None,
        };
    }
    crate::config::load().runtime
}

/// Candidates to use: the preferred one if it was detected, else all.
pub fn select_candidates(
    candidates: Vec<DetectedRuntime>,
    preferred: Option<RuntimeKind>,
) -> Vec<DetectedRuntime> {
    match preferred {
        Some(kind) if candidates.iter().any(|c| c.kind == kind) => {
            candidates.into_iter().filter(|c| c.kind == kind).collect()
        }
        _ => candidates,
    }
}

/// Build an adapter for a candidate, resolving binary-only endpoints.
pub async fn connect(candidate: &DetectedRuntime) -> Option<Arc<dyn RuntimeAdapter>> {
    let endpoint = endpoints::resolve(candidate).await?;
    match candidate.kind {
        RuntimeKind::Docker | RuntimeKind::Podman => {
            DockerAdapter::connect(candidate.kind, &endpoint)
                .ok()
                .map(|a| Arc::new(a) as Arc<dyn RuntimeAdapter>)
        }
        RuntimeKind::AppleContainer => Some(Arc::new(AppleContainerAdapter::new(endpoint))),
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

    /// All adapters that answered in the last refresh.
    pub fn adapters(&self) -> Vec<Arc<dyn RuntimeAdapter>> {
        let state = self.state.lock().expect("live state poisoned");
        let mut list: Vec<_> = state.adapters.iter().collect();
        list.sort_by_key(|(k, _)| k.as_str());
        list.into_iter().map(|(_, a)| a.clone()).collect()
    }

    /// The adapter that listed this container in the last refresh.
    pub fn adapter_for(&self, container_id: &str) -> Option<Arc<dyn RuntimeAdapter>> {
        let state = self.state.lock().expect("live state poisoned");
        state.by_container.get(container_id).cloned().or_else(|| {
            state
                .by_container
                .iter()
                .find(|(id, _)| id.starts_with(container_id))
                .map(|(_, a)| a.clone())
        })
    }

    /// Install a specific adapter instead of detecting. For tests and demos.
    pub fn with_adapter(self: &Arc<Self>, adapter: Arc<dyn RuntimeAdapter>) {
        let kind = adapter.kind();
        let mut state = self.state.lock().expect("live state poisoned");
        state.adapters.insert(kind, adapter.clone());
        drop(state);
        self.start_events(kind, adapter);
    }

    fn drop_adapter(&self, kind: RuntimeKind) {
        let mut state = self.state.lock().expect("live state poisoned");
        state.adapters.remove(&kind);
        if let Some(task) = state.event_tasks.remove(&kind) {
            task.abort();
        }
    }

    fn start_events(self: &Arc<Self>, kind: RuntimeKind, adapter: Arc<dyn RuntimeAdapter>) {
        if !adapter.capabilities().events {
            return;
        }
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
            // The stream ended: the daemon went away or restarted. Forget this
            // adapter so the next refresh reconnects and resubscribes.
            if let Some(live) = weak.upgrade() {
                let mut state = live.state.lock().expect("live state poisoned");
                state.adapters.remove(&kind);
                state.event_tasks.remove(&kind);
            }
        });
        let mut state = self.state.lock().expect("live state poisoned");
        if let Some(old) = state.event_tasks.insert(kind, task) {
            old.abort();
        }
    }

    /// Detect, connect what is missing, and load the overview across runtimes.
    pub async fn overview(self: &Arc<Self>) -> Overview {
        let candidates = detect(&Environment::current());
        if candidates.is_empty() {
            self.clear();
            return Overview::not_detected(DataSource::Live);
        }
        let selected = select_candidates(candidates.clone(), preferred_runtime());

        // Drop adapters for runtimes that are no longer selected.
        let stale: Vec<RuntimeKind> = {
            let state = self.state.lock().expect("live state poisoned");
            state
                .adapters
                .keys()
                .filter(|k| !selected.iter().any(|c| c.kind == **k))
                .copied()
                .collect()
        };
        for kind in stale {
            self.drop_adapter(kind);
        }

        // Connect the missing ones.
        for candidate in &selected {
            let present = self
                .state
                .lock()
                .expect("live state poisoned")
                .adapters
                .contains_key(&candidate.kind);
            if present {
                continue;
            }
            if let Some(adapter) = connect(candidate).await {
                self.with_adapter(adapter);
            }
        }

        let adapters = self.adapters();
        if adapters.is_empty() {
            return Overview::detected_only(candidates);
        }
        let overview = load_many(&adapters, DataSource::Live, candidates).await;

        // Forget adapters that failed so they are reconnected next time, and
        // remember which adapter owns which container.
        match &overview.runtime {
            RuntimeStatus::Connected { failures, .. } => {
                for failure in failures {
                    self.drop_adapter(failure.kind);
                }
                let mut state = self.state.lock().expect("live state poisoned");
                state.by_container.clear();
                for c in &overview.containers {
                    if let Some(adapter) = state.adapters.get(&c.runtime).cloned() {
                        state.by_container.insert(c.container.id.clone(), adapter);
                    }
                }
            }
            _ => self.clear(),
        }
        overview
    }

    fn clear(&self) {
        let mut state = self.state.lock().expect("live state poisoned");
        state.adapters.clear();
        state.by_container.clear();
        for (_, task) in state.event_tasks.drain() {
            task.abort();
        }
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

    fn candidate(kind: RuntimeKind) -> DetectedRuntime {
        DetectedRuntime {
            kind,
            endpoint: kind.as_str().into(),
            source: "test".into(),
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
        assert!(!is_relevant(&event(RuntimeEventKind::ExecCreated(
            "/usr/local/bin/sandbox-container-tools network diagnostic dns".into()
        ))));
    }

    #[test]
    fn preferred_runtime_narrows_candidates_only_when_present() {
        let all = vec![
            candidate(RuntimeKind::Docker),
            candidate(RuntimeKind::Podman),
        ];
        let picked = select_candidates(all.clone(), Some(RuntimeKind::Podman));
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].kind, RuntimeKind::Podman);
        assert_eq!(
            select_candidates(all.clone(), Some(RuntimeKind::AppleContainer)).len(),
            2
        );
        assert_eq!(select_candidates(all, None).len(), 2);
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

    #[tokio::test]
    async fn adapters_are_listed_in_stable_order() {
        let live = Arc::new(LiveRuntime::new());
        live.with_adapter(Arc::new(MockAdapter::demo().with_kind(RuntimeKind::Podman)));
        live.with_adapter(Arc::new(MockAdapter::demo()));
        let kinds: Vec<_> = live.adapters().iter().map(|a| a.kind()).collect();
        assert_eq!(kinds, vec![RuntimeKind::Docker, RuntimeKind::Podman]);
    }
}
