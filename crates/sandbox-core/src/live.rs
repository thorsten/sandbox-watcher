//! Detect runtimes, keep one adapter per usable runtime between refreshes,
//! and forward lifecycle events to a listener.
//!
//! Selection follows sandbox: a `runtime` set in sandbox's user config (or
//! `SANDBOX_WATCHER_RUNTIME`) pins one runtime; otherwise every detected
//! runtime is used and containers from all of them are shown together.
//!
//! Remote hosts from settings are connected in the background (see
//! [`crate::remote_hosts`]); their adapters sit next to the local ones.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;

use crate::adapter::RuntimeAdapter;
use crate::apple::AppleContainerAdapter;
use crate::detect::{DetectedRuntime, Environment, detect};
use crate::diagnostics::{Probe, ProbeOutcome};
use crate::docker::DockerAdapter;
use crate::endpoints;
use crate::host::{Host, RuntimeId, container_key, parse_container_key};
use crate::model::{RuntimeEvent, RuntimeEventKind, RuntimeKind};
use crate::network::DIAGNOSTIC_COMMAND_PREFIX;
use crate::overview::{
    DataSource, HostedAdapter, Overview, RuntimeFailure, RuntimeStatus, StartTimeCache,
    load_many_with_failures,
};
use crate::remote_hosts::{RemoteHosts, Wake};
use crate::runner::CommandOutput;
use crate::sandbox::PROBE_SENTINEL;
use crate::settings::RemoteHost;

pub use crate::remote_hosts::{REMOTE_TIMEOUT, stale_snapshot, test_remote_host};

/// Receives relevant runtime events, see [`is_relevant`].
pub type Listener = Arc<dyn Fn(RuntimeEvent) + Send + Sync>;

#[derive(Default)]
struct Connections {
    adapters: HashMap<RuntimeId, Arc<dyn RuntimeAdapter>>,
    /// Container key to its adapter and raw container id.
    by_container: HashMap<String, (Arc<dyn RuntimeAdapter>, String)>,
    event_tasks: HashMap<RuntimeId, tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
pub struct LiveRuntime {
    state: Mutex<Connections>,
    listener: Mutex<Option<Listener>>,
    /// Runtime pinned by the user's settings; wins over env and sandbox config.
    preferred_override: Mutex<Option<RuntimeKind>>,
    remotes: RemoteHosts,
    /// Start times of running containers, filled from `inspect` once each.
    start_times: StartTimeCache,
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
            !command.contains(PROBE_SENTINEL)
                && !command.contains(DIAGNOSTIC_COMMAND_PREFIX)
                && !command.contains("sandbox-watcher-")
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
    connect_traced(candidate).await.0
}

/// `connect` with the resolve and connect steps recorded.
pub async fn connect_traced(
    candidate: &DetectedRuntime,
) -> (Option<Arc<dyn RuntimeAdapter>>, Vec<Probe>) {
    let mut trace = Vec::new();
    let endpoint = match endpoints::resolve_traced(candidate).await {
        Ok((endpoint, how)) => {
            // Say how the endpoint was found, or why the fallback applies.
            if how != candidate.source {
                trace.push(Probe {
                    kind: candidate.kind,
                    candidate: how,
                    outcome: ProbeOutcome::Found {
                        endpoint: endpoint.clone(),
                    },
                });
            }
            endpoint
        }
        Err(reason) => {
            trace.push(Probe {
                kind: candidate.kind,
                candidate: format!("resolve {}", candidate.endpoint),
                outcome: ProbeOutcome::Failed { reason },
            });
            return (None, trace);
        }
    };
    let adapter: Arc<dyn RuntimeAdapter> = match candidate.kind {
        RuntimeKind::Docker | RuntimeKind::Podman => {
            match DockerAdapter::connect(candidate.kind, &endpoint) {
                Ok(a) => Arc::new(a),
                Err(e) => {
                    trace.push(Probe {
                        kind: candidate.kind,
                        candidate: format!("connect {endpoint}"),
                        outcome: ProbeOutcome::Failed {
                            reason: e.to_string(),
                        },
                    });
                    return (None, trace);
                }
            }
        }
        RuntimeKind::AppleContainer => Arc::new(AppleContainerAdapter::new(endpoint)),
    };
    (Some(adapter), trace)
}

impl LiveRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the event sink. Takes effect on the next (re)connect.
    pub fn set_listener(&self, listener: Listener) {
        *self.listener.lock().expect("listener poisoned") = Some(listener);
    }

    /// Pin a runtime from settings. `None` falls back to env and sandbox config.
    pub fn set_preferred(&self, kind: Option<RuntimeKind>) {
        *self.preferred_override.lock().expect("preferred poisoned") = kind;
    }

    /// Remote hosts from settings. Takes effect on the next refresh.
    pub fn set_remote_hosts(&self, hosts: Vec<RemoteHost>) {
        self.remotes.set_hosts(hosts);
    }

    /// Run a shell command line on a connected remote host.
    pub async fn run_on_host(
        &self,
        alias: &str,
        command: &str,
        timeout: Duration,
    ) -> std::result::Result<CommandOutput, String> {
        self.remotes.run_on(alias, command, timeout).await
    }

    /// Retry unreachable hosts on the next refresh ("Test connection").
    pub fn reset_backoff(&self) {
        self.remotes.reset_backoff();
    }

    /// The watcher's poll interval. A much longer gap between refreshes is
    /// taken as a wake from sleep and resets the reconnect backoff.
    pub fn set_poll_interval(&self, interval: Duration) {
        self.remotes.set_poll_interval(interval);
    }

    /// Close every remote session. Call on quit.
    pub async fn shutdown(&self) {
        self.remotes.shutdown().await;
    }

    fn preferred(&self) -> Option<RuntimeKind> {
        self.preferred_override
            .lock()
            .expect("preferred poisoned")
            .or_else(preferred_runtime)
    }

    /// All adapters that answered in the last refresh, local first.
    pub fn adapters(&self) -> Vec<HostedAdapter> {
        let state = self.state.lock().expect("live state poisoned");
        let mut list: Vec<_> = state.adapters.iter().collect();
        list.sort_by_key(|(id, _)| (*id).clone());
        list.into_iter()
            .map(|(id, adapter)| HostedAdapter {
                host: id.host.clone(),
                adapter: adapter.clone(),
            })
            .collect()
    }

    /// The adapter that listed this container in the last refresh, with the
    /// raw container id. A unique-prefix of the id works within the same host.
    pub fn adapter_for(&self, key: &str) -> Option<(Arc<dyn RuntimeAdapter>, String)> {
        let state = self.state.lock().expect("live state poisoned");
        if let Some(found) = state.by_container.get(key) {
            return Some(found.clone());
        }
        let (host, id) = parse_container_key(key);
        state
            .by_container
            .iter()
            .find(|(known, (_, raw))| parse_container_key(known).0 == host && raw.starts_with(&id))
            .map(|(_, found)| found.clone())
    }

    /// Install a specific adapter instead of detecting. For tests and demos.
    pub fn with_adapter(self: &Arc<Self>, host: Host, adapter: Arc<dyn RuntimeAdapter>) {
        let id = RuntimeId {
            host,
            kind: adapter.kind(),
        };
        let mut state = self.state.lock().expect("live state poisoned");
        state.adapters.insert(id.clone(), adapter.clone());
        drop(state);
        self.start_events(id, adapter);
    }

    fn drop_adapter(&self, id: &RuntimeId) {
        let mut state = self.state.lock().expect("live state poisoned");
        state.adapters.remove(id);
        if let Some(task) = state.event_tasks.remove(id) {
            task.abort();
        }
    }

    fn start_events(self: &Arc<Self>, id: RuntimeId, adapter: Arc<dyn RuntimeAdapter>) {
        if !adapter.capabilities().events {
            return;
        }
        let Some(listener) = self.listener.lock().expect("listener poisoned").clone() else {
            return;
        };
        let weak = Arc::downgrade(self);
        let task_id = id.clone();
        let task = tokio::spawn(async move {
            // An error from the stream means this runtime cannot deliver
            // events (e.g. rootless Podman without an events backend). Keep
            // the adapter; the UI's poll covers it. Only a clean end of the
            // stream means the daemon went away, in which case the adapter is
            // forgotten so the next refresh reconnects and resubscribes.
            let mut reconnect = false;
            if let Ok(mut stream) = adapter.events().await {
                reconnect = true;
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(event) if is_relevant(&event) => {
                            // A (re)start changes the start time; a stop or
                            // removal ends it. The next listing inspects again.
                            if !matches!(event.kind, RuntimeEventKind::ExecCreated(_))
                                && let Some(live) = weak.upgrade()
                            {
                                live.start_times
                                    .forget(&container_key(&task_id.host, &event.container_id));
                            }
                            listener(event)
                        }
                        Ok(_) => {}
                        Err(_) => {
                            reconnect = false;
                            break;
                        }
                    }
                }
            }
            if let Some(live) = weak.upgrade() {
                let mut state = live.state.lock().expect("live state poisoned");
                if reconnect {
                    state.adapters.remove(&task_id);
                }
                state.event_tasks.remove(&task_id);
            }
        });
        let mut state = self.state.lock().expect("live state poisoned");
        if let Some(old) = state.event_tasks.insert(id, task) {
            old.abort();
        }
    }

    /// Detect, connect what is missing, and load the overview across runtimes
    /// and remote hosts.
    pub async fn overview(self: &Arc<Self>) -> Overview {
        let candidates = detect(&Environment::current());
        let selected = select_candidates(candidates.clone(), self.preferred());

        // Drop adapters for runtimes that are no longer selected.
        let stale: Vec<RuntimeId> = {
            let state = self.state.lock().expect("live state poisoned");
            state
                .adapters
                .keys()
                .filter(|id| id.host.is_local() && !selected.iter().any(|c| c.kind == id.kind))
                .cloned()
                .collect()
        };
        for id in stale {
            self.drop_adapter(&id);
        }

        // Connect the missing ones.
        for candidate in &selected {
            let present = self
                .state
                .lock()
                .expect("live state poisoned")
                .adapters
                .contains_key(&RuntimeId {
                    host: Host::Local,
                    kind: candidate.kind,
                });
            if present {
                continue;
            }
            if let Some(adapter) = connect(candidate).await {
                self.with_adapter(Host::Local, adapter);
            }
        }

        let (overview, connected) = self
            .remotes
            .refresh(self.wake(), |remote| async move {
                self.sync_remote_adapters(&remote);
                self.load_reporting(candidates).await
            })
            .await;
        self.sync_remote_adapters(&connected);
        overview
    }

    /// Asks the listener for a refresh once a background connection finished.
    fn wake(&self) -> Option<Wake> {
        let listener = self.listener.lock().expect("listener poisoned").clone()?;
        Some(Arc::new(move || {
            listener(RuntimeEvent {
                at: chrono::Utc::now(),
                container_id: String::new(),
                container_name: None,
                kind: RuntimeEventKind::Created,
            })
        }))
    }

    /// Make the installed remote adapters exactly `wanted`: drop the others,
    /// install new ones (which also subscribes to their events).
    fn sync_remote_adapters(self: &Arc<Self>, wanted: &[HostedAdapter]) {
        let is_wanted = |id: &RuntimeId, adapter: &Arc<dyn RuntimeAdapter>| {
            wanted.iter().any(|w| {
                w.host == id.host && w.adapter.kind() == id.kind && Arc::ptr_eq(&w.adapter, adapter)
            })
        };
        let (stale, missing): (Vec<RuntimeId>, Vec<HostedAdapter>) = {
            let state = self.state.lock().expect("live state poisoned");
            let stale = state
                .adapters
                .iter()
                .filter(|(id, adapter)| !id.host.is_local() && !is_wanted(id, adapter))
                .map(|(id, _)| id.clone())
                .collect();
            let missing = wanted
                .iter()
                .filter(|w| {
                    let id = RuntimeId {
                        host: w.host.clone(),
                        kind: w.adapter.kind(),
                    };
                    !state
                        .adapters
                        .get(&id)
                        .is_some_and(|a| Arc::ptr_eq(a, &w.adapter))
                })
                .cloned()
                .collect();
            (stale, missing)
        };
        for id in stale {
            self.drop_adapter(&id);
        }
        for hosted in missing {
            self.with_adapter(hosted.host, hosted.adapter);
        }
    }

    /// Load the overview from the connected adapters, forget the ones that
    /// failed, and remember which adapter owns which container.
    pub async fn load_connected(self: &Arc<Self>, candidates: Vec<DetectedRuntime>) -> Overview {
        self.load_reporting(candidates).await.0
    }

    async fn load_reporting(
        self: &Arc<Self>,
        candidates: Vec<DetectedRuntime>,
    ) -> (Overview, Vec<RuntimeFailure>) {
        let adapters = self.adapters();
        if adapters.is_empty() {
            self.clear();
            let overview = if candidates.is_empty() {
                Overview::not_detected(DataSource::Live)
            } else {
                Overview::detected_only(candidates)
            };
            return (overview, vec![]);
        }
        let (overview, failures) =
            load_many_with_failures(&adapters, DataSource::Live, candidates, &self.start_times)
                .await;

        // Forget adapters that failed so they are reconnected next time, and
        // remember which adapter owns which container.
        match &overview.runtime {
            RuntimeStatus::Connected { failures, .. } => {
                for failure in failures {
                    self.drop_adapter(&RuntimeId {
                        host: failure.host.clone(),
                        kind: failure.kind,
                    });
                }
                let mut state = self.state.lock().expect("live state poisoned");
                state.by_container.clear();
                for c in &overview.containers {
                    let id = RuntimeId {
                        host: c.host.clone(),
                        kind: c.runtime,
                    };
                    if let Some(adapter) = state.adapters.get(&id).cloned() {
                        state
                            .by_container
                            .insert(c.key.clone(), (adapter, c.container.id.clone()));
                    }
                }
            }
            _ => self.clear(),
        }
        (overview, failures)
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
    use crate::overview::ContainerOverview;

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
        live.with_adapter(Host::Local, Arc::new(mock.clone()));
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
        live.with_adapter(
            Host::Local,
            Arc::new(MockAdapter::demo().with_kind(RuntimeKind::Podman)),
        );
        live.with_adapter(Host::Local, Arc::new(MockAdapter::demo()));
        let kinds: Vec<_> = live.adapters().iter().map(|a| a.adapter.kind()).collect();
        assert_eq!(kinds, vec![RuntimeKind::Docker, RuntimeKind::Podman]);
    }

    #[tokio::test]
    async fn adapter_for_resolves_container_keys() {
        let live = Arc::new(LiveRuntime::new());
        live.with_adapter(Host::Local, Arc::new(MockAdapter::demo()));
        let overview = live.load_connected(vec![]).await;
        let first = &overview.containers[0];
        let (_, id) = live.adapter_for(&first.key).expect("known key");
        assert_eq!(id, first.container.id);
        assert!(live.adapter_for("devbox/unknown").is_none());

        // A unique id prefix resolves, but only within the same host.
        let prefix: String = first.container.id.chars().take(4).collect();
        let (_, id) = live.adapter_for(&prefix).expect("prefix on same host");
        assert!(id.starts_with(&prefix));
        assert!(live.adapter_for(&format!("devbox/{prefix}")).is_none());
    }

    fn test_container(key: &str) -> ContainerOverview {
        let (host, id) = parse_container_key(key);
        ContainerOverview {
            container: crate::model::SandboxContainer {
                id: id.clone(),
                name: format!("sandbox-{id}"),
                project_slug: id,
                image: "sandbox:latest".into(),
                state: crate::model::ContainerState::Running,
                created_at: None,
                started_at: None,
                config_hash: None,
                labels: Default::default(),
            },
            host,
            key: key.into(),
            runtime: RuntimeKind::Docker,
            readiness: crate::model::Readiness::Ready,
            sessions: vec![crate::model::Session {
                pid: 7,
                command: "claude".into(),
                agent: crate::model::AgentKind::ClaudeCode,
            }],
            last_seen_at: None,
        }
    }

    #[test]
    fn unreachable_host_keeps_last_snapshot_greyed() {
        let mut c = test_container("devbox/c1");
        c.last_seen_at = None;
        let since = chrono::Utc::now();
        let kept = stale_snapshot(&[c], since);
        assert_eq!(kept[0].last_seen_at, Some(since));
        assert!(
            kept[0].sessions.is_empty(),
            "no live sessions claimed while unreachable"
        );
    }

    #[test]
    fn remote_timeout_is_ten_seconds() {
        assert_eq!(REMOTE_TIMEOUT, Duration::from_secs(10));
    }
}
