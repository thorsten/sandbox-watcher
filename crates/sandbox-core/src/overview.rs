//! The top-level state the UI renders: which runtimes, which containers, who
//! is inside them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::adapter::{Capabilities, ContainerFilter, RuntimeAdapter};
use crate::detect::DetectedRuntime;
use crate::host::{Host, container_key};
use crate::model::{Readiness, RuntimeInfo, RuntimeKind, SandboxContainer, Session};
use crate::remote::HostStatus;
use crate::sandbox::TESTED_SANDBOX_VERSION;
use crate::sessions::probe_container;

/// How long one remote host may take to connect or to answer a refresh.
pub const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the data on screen comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataSource {
    Mock,
    Live,
}

/// A runtime that was detected but could not be used this round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFailure {
    #[serde(default)]
    pub host: Host,
    pub kind: RuntimeKind,
    pub endpoint: String,
    pub reason: String,
}

/// A runtime that answered, with what it can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectedRuntime {
    #[serde(default)]
    pub host: Host,
    #[serde(flatten)]
    pub info: RuntimeInfo,
    pub capabilities: Capabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
pub enum RuntimeStatus {
    /// No Docker, Podman or Apple `container` found.
    NotDetected,
    /// Runtime binaries or sockets exist, but none could be connected.
    DetectedOnly { candidates: Vec<DetectedRuntime> },
    /// At least one runtime answers. `failures` lists detected runtimes that
    /// did not, so the UI can say why a second runtime is missing.
    Connected {
        runtimes: Vec<ConnectedRuntime>,
        failures: Vec<RuntimeFailure>,
    },
    /// Adapters exist but every runtime refused the connection.
    Unavailable {
        candidates: Vec<DetectedRuntime>,
        reason: String,
    },
}

/// A container plus what the probe found inside it. Serialises flat, so the
/// UI sees one object with the container fields, `runtime`, `readiness` and
/// `sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerOverview {
    #[serde(flatten)]
    pub container: SandboxContainer,
    pub host: Host,
    /// Unique across hosts: the raw id locally, `alias/id` on a remote host.
    pub key: String,
    pub runtime: RuntimeKind,
    pub readiness: Readiness,
    pub sessions: Vec<Session>,
    /// Set only for containers of an unreachable host, shown from cache.
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overview {
    pub source: DataSource,
    pub runtime: RuntimeStatus,
    pub containers: Vec<ContainerOverview>,
    pub tested_sandbox_version: String,
    /// Configured remote hosts and their connection state.
    #[serde(default)]
    pub hosts: Vec<HostStatus>,
}

impl Overview {
    fn with(
        source: DataSource,
        runtime: RuntimeStatus,
        containers: Vec<ContainerOverview>,
    ) -> Self {
        Self {
            source,
            runtime,
            containers,
            tested_sandbox_version: TESTED_SANDBOX_VERSION.into(),
            hosts: vec![],
        }
    }

    pub fn not_detected(source: DataSource) -> Self {
        Self::with(source, RuntimeStatus::NotDetected, vec![])
    }

    pub fn detected_only(candidates: Vec<DetectedRuntime>) -> Self {
        Self::with(
            DataSource::Live,
            RuntimeStatus::DetectedOnly { candidates },
            vec![],
        )
    }

    /// Runtime kinds that answered this round.
    pub fn connected_kinds(&self) -> Vec<RuntimeKind> {
        match &self.runtime {
            RuntimeStatus::Connected { runtimes, .. } => {
                runtimes.iter().map(|r| r.info.kind).collect()
            }
            _ => vec![],
        }
    }
}

/// An adapter plus the machine it talks to.
#[derive(Clone)]
pub struct HostedAdapter {
    pub host: Host,
    pub adapter: Arc<dyn RuntimeAdapter>,
}

impl HostedAdapter {
    pub fn local(adapter: Arc<dyn RuntimeAdapter>) -> Self {
        Self {
            host: Host::Local,
            adapter,
        }
    }
}

struct RuntimeLoad {
    runtime: ConnectedRuntime,
    containers: Vec<ContainerOverview>,
}

/// Start times of running containers, by container key. Docker and Podman
/// report `StartedAt` only from `inspect`, so each running container is
/// inspected once and remembered until it stops, dies or is removed (the
/// live runtime forgets it on those events) or disappears from a listing.
#[derive(Default)]
pub struct StartTimeCache {
    inner: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl StartTimeCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, DateTime<Utc>>> {
        self.inner.lock().expect("start time cache poisoned")
    }

    pub fn get(&self, key: &str) -> Option<DateTime<Utc>> {
        self.lock().get(key).copied()
    }

    pub fn put(&self, key: &str, started_at: DateTime<Utc>) {
        self.lock().insert(key.to_string(), started_at);
    }

    /// Drop one container, e.g. after a start, stop or die event: the next
    /// listing inspects it again.
    pub fn forget(&self, key: &str) {
        self.lock().remove(key);
    }

    /// Drop every container of `host` that is not in `running`.
    fn retain_running(&self, host: &Host, running: &[String]) {
        let prefix = container_key(host, "");
        self.lock().retain(|key, _| {
            !(key.starts_with(&prefix) && !running.iter().any(|k| k == key))
                || key.len() == prefix.len()
        });
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

/// Run one round trip to `host`. On a remote host it gets
/// [`REMOTE_TIMEOUT`], so a stalled connection cannot hold up the caller;
/// `None` means it ran out of time. Local round trips run to completion.
pub async fn within_budget<F: std::future::Future>(host: &Host, fut: F) -> Option<F::Output> {
    if host.is_local() {
        Some(fut.await)
    } else {
        tokio::time::timeout(REMOTE_TIMEOUT, fut).await.ok()
    }
}

/// Load one runtime, giving up after `timeout` if one is set.
async fn load_one(
    hosted: HostedAdapter,
    timeout: Option<Duration>,
    start_times: &StartTimeCache,
) -> std::result::Result<RuntimeLoad, RuntimeFailure> {
    let Some(limit) = timeout else {
        return load_runtime(hosted, start_times).await;
    };
    let host = hosted.host.clone();
    let kind = hosted.adapter.kind();
    tokio::time::timeout(limit, load_runtime(hosted, start_times))
        .await
        .unwrap_or_else(|_| {
            Err(RuntimeFailure {
                host,
                kind,
                endpoint: String::new(),
                reason: format!("no answer within {}s", limit.as_secs()),
            })
        })
}

/// When `container` started, from the cache or one `inspect`. `None` when
/// the runtime does not say or the inspect fails; the UI shows "unknown".
async fn started_at(
    adapter: &Arc<dyn RuntimeAdapter>,
    key: &str,
    container: &SandboxContainer,
    start_times: &StartTimeCache,
) -> Option<DateTime<Utc>> {
    if !container.state.is_running() {
        return None;
    }
    if let Some(known) = container.started_at {
        return Some(known);
    }
    if let Some(cached) = start_times.get(key) {
        return Some(cached);
    }
    let started = adapter
        .inspect(&container.id)
        .await
        .ok()
        .flatten()
        .and_then(|d| d.container.started_at)?;
    start_times.put(key, started);
    Some(started)
}

async fn load_runtime(
    hosted: HostedAdapter,
    start_times: &StartTimeCache,
) -> std::result::Result<RuntimeLoad, RuntimeFailure> {
    let HostedAdapter { host, adapter } = hosted;
    let failure = |endpoint: String, reason: String| RuntimeFailure {
        host: host.clone(),
        kind: adapter.kind(),
        endpoint,
        reason,
    };
    let info = adapter
        .ping()
        .await
        .map_err(|e| failure(String::new(), e.to_string()))?;
    let containers = adapter
        .list_containers(&ContainerFilter::all())
        .await
        .map_err(|e| failure(info.endpoint.clone(), e.to_string()))?;

    let running_keys: Vec<String> = containers
        .iter()
        .filter(|c| c.state.is_running())
        .map(|c| container_key(&host, &c.id))
        .collect();
    start_times.retain_running(&host, &running_keys);
    let probes = containers.iter().map(|c| {
        let adapter = adapter.clone();
        let running = c.state.is_running();
        let key = container_key(&host, &c.id);
        async move {
            if !running {
                return (Default::default(), None);
            }
            // A failed probe must not hide the container; it just shows as unknown.
            futures::join!(
                async { probe_container(&adapter, &c.id).await.unwrap_or_default() },
                started_at(&adapter, &key, c, start_times),
            )
        }
    });
    let probes = futures::future::join_all(probes).await;
    let kind = adapter.kind();
    let containers = containers
        .into_iter()
        .zip(probes)
        .map(|(mut container, (probe, started))| {
            if started.is_some() {
                container.started_at = started;
            }
            ContainerOverview {
                key: container_key(&host, &container.id),
                host: host.clone(),
                container,
                runtime: kind,
                readiness: probe.readiness,
                sessions: probe.sessions,
                last_seen_at: None,
            }
        })
        .collect();
    Ok(RuntimeLoad {
        runtime: ConnectedRuntime {
            host: host.clone(),
            info,
            capabilities: adapter.capabilities(),
        },
        containers,
    })
}

/// Load the overview across several runtimes at once. Running containers
/// sort first, then by host (local first), runtime order, then name. Every
/// running container is probed for readiness and sessions.
pub async fn load_many(
    adapters: &[HostedAdapter],
    source: DataSource,
    candidates: Vec<DetectedRuntime>,
) -> Overview {
    load_many_with_failures(adapters, source, candidates, &StartTimeCache::default())
        .await
        .0
}

/// [`load_many`], also returning every runtime that failed, even when none
/// answered and the overview only carries the joined reasons. Remote
/// runtimes get [`REMOTE_TIMEOUT`] each, so a slow host cannot hold up the
/// others.
pub async fn load_many_with_failures(
    adapters: &[HostedAdapter],
    source: DataSource,
    candidates: Vec<DetectedRuntime>,
    start_times: &StartTimeCache,
) -> (Overview, Vec<RuntimeFailure>) {
    let loads = adapters.iter().cloned().map(|hosted| {
        let timeout = (!hosted.host.is_local()).then_some(REMOTE_TIMEOUT);
        load_one(hosted, timeout, start_times)
    });
    let results = futures::future::join_all(loads).await;
    let mut runtimes = Vec::new();
    let mut failures = Vec::new();
    let mut containers = Vec::new();
    for result in results {
        match result {
            Ok(load) => {
                runtimes.push(load.runtime);
                containers.extend(load.containers);
            }
            Err(failure) => failures.push(failure),
        }
    }
    if runtimes.is_empty() {
        let reason = failures
            .iter()
            .map(|f| {
                let label = if f.host.is_local() {
                    f.kind.to_string()
                } else {
                    format!("{} on {}", f.kind, f.host)
                };
                format!("{label}: {}", f.reason)
            })
            .collect::<Vec<_>>()
            .join("; ");
        return (
            Overview::with(
                source,
                RuntimeStatus::Unavailable { candidates, reason },
                vec![],
            ),
            failures,
        );
    }
    sort_containers(&mut containers);
    (
        Overview::with(
            source,
            RuntimeStatus::Connected {
                runtimes,
                failures: failures.clone(),
            },
            containers,
        ),
        failures,
    )
}

/// Running first, then by host (local first), runtime order, then name.
pub(crate) fn sort_containers(containers: &mut [ContainerOverview]) {
    containers.sort_by(|a, b| {
        b.container
            .state
            .is_running()
            .cmp(&a.container.state.is_running())
            .then_with(|| a.host.cmp(&b.host))
            .then_with(|| a.runtime.as_str().cmp(b.runtime.as_str()))
            .then_with(|| a.container.name.cmp(&b.container.name))
    });
}

/// Single-runtime convenience over [`load_many`].
pub async fn load(
    adapter: Arc<dyn RuntimeAdapter>,
    source: DataSource,
    candidates: Vec<DetectedRuntime>,
) -> Overview {
    load_many(&[HostedAdapter::local(adapter)], source, candidates).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockAdapter;
    use crate::model::AgentKind;

    #[tokio::test]
    async fn running_containers_sort_first_and_are_probed() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        assert!(matches!(overview.runtime, RuntimeStatus::Connected { .. }));
        let names: Vec<_> = overview
            .containers
            .iter()
            .map(|c| c.container.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "sandbox-billing-api-live",
                "sandbox-web-shop",
                "sandbox-web-shop-2",
                "sandbox-billing-api"
            ]
        );
        let shop = &overview.containers[1];
        assert_eq!(shop.readiness, Readiness::Ready);
        let agents: Vec<_> = shop.sessions.iter().map(|s| s.agent).collect();
        assert_eq!(agents, vec![AgentKind::ClaudeCode, AgentKind::Shell]);
        let stopped = overview.containers.last().unwrap();
        assert_eq!(stopped.readiness, Readiness::Unknown);
        assert!(
            stopped.sessions.is_empty(),
            "stopped containers are not probed"
        );
    }

    #[tokio::test]
    async fn merges_several_runtimes() {
        let docker = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let podman =
            HostedAdapter::local(Arc::new(MockAdapter::demo().with_kind(RuntimeKind::Podman)));
        let overview = load_many(&[docker, podman], DataSource::Mock, vec![]).await;
        assert_eq!(
            overview.connected_kinds(),
            vec![RuntimeKind::Docker, RuntimeKind::Podman]
        );
        assert_eq!(overview.containers.len(), 8);
        let running: Vec<_> = overview
            .containers
            .iter()
            .take(6)
            .map(|c| (c.runtime, c.container.name.as_str()))
            .collect();
        assert_eq!(
            running[0],
            (RuntimeKind::Docker, "sandbox-billing-api-live")
        );
        assert_eq!(
            running[3],
            (RuntimeKind::Podman, "sandbox-billing-api-live")
        );
    }

    #[tokio::test]
    async fn serialises_flat_for_the_ui() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        let json = serde_json::to_value(&overview).unwrap();
        assert_eq!(json["runtime"]["status"], "connected");
        assert_eq!(json["runtime"]["runtimes"][0]["kind"], "docker");
        let first = &json["containers"][0];
        assert!(first["name"].is_string(), "container fields are flattened");
        assert_eq!(first["runtime"], "docker");
        assert_eq!(first["readiness"], "ready");
        assert_eq!(first["sessions"][0]["agent"], "claude-code");
    }

    #[tokio::test]
    async fn same_container_id_on_two_hosts_stays_apart() {
        let local = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let remote = HostedAdapter {
            host: Host::Remote("devbox".into()),
            adapter: Arc::new(MockAdapter::demo()),
        };
        let overview = load_many(&[local, remote], DataSource::Mock, vec![]).await;
        assert_eq!(overview.containers.len(), 8);
        let keys: std::collections::HashSet<_> =
            overview.containers.iter().map(|c| c.key.clone()).collect();
        assert_eq!(keys.len(), 8, "keys are unique across hosts");
        let first_remote = overview
            .containers
            .iter()
            .find(|c| !c.host.is_local())
            .unwrap();
        assert_eq!(
            first_remote.key,
            format!("devbox/{}", first_remote.container.id)
        );
        // Local first within running containers.
        assert!(overview.containers[0].host.is_local());
    }

    #[tokio::test]
    async fn local_sorts_before_remote_whatever_the_input_order() {
        let remote = HostedAdapter {
            host: Host::Remote("devbox".into()),
            adapter: Arc::new(MockAdapter::demo()),
        };
        let local = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let overview = load_many(&[remote, local], DataSource::Mock, vec![]).await;
        let hosts: Vec<_> = overview
            .containers
            .iter()
            .take(6)
            .map(|c| c.host.is_local())
            .collect();
        assert_eq!(hosts, vec![true, true, true, false, false, false]);
    }

    #[tokio::test]
    async fn serialises_host_and_key() {
        let overview = load(Arc::new(MockAdapter::demo()), DataSource::Mock, vec![]).await;
        let json = serde_json::to_value(&overview).unwrap();
        let first = &json["containers"][0];
        assert_eq!(first["host"], "local");
        assert_eq!(first["key"], first["id"]);
        assert!(first["last_seen_at"].is_null());
        assert_eq!(json["runtime"]["runtimes"][0]["host"], "local");
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_remote_runtime_fails_after_the_remote_timeout() {
        let local = HostedAdapter::local(Arc::new(MockAdapter::demo()));
        let remote = HostedAdapter {
            host: Host::Remote("devbox".into()),
            adapter: Arc::new(MockAdapter::demo().stalled()),
        };
        let started = tokio::time::Instant::now();
        let (overview, failures) = tokio::time::timeout(
            Duration::from_secs(600),
            load_many_with_failures(
                &[local, remote],
                DataSource::Mock,
                vec![],
                &StartTimeCache::default(),
            ),
        )
        .await
        .expect("a stalled remote runtime must not hold up the overview");
        assert!(
            started.elapsed() <= REMOTE_TIMEOUT + Duration::from_millis(50),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].host, Host::Remote("devbox".into()));
        assert_eq!(failures[0].reason, "no answer within 10s");
        assert_eq!(overview.containers.len(), 4, "local containers still load");
        assert!(overview.containers.iter().all(|c| c.host.is_local()));
    }

    #[tokio::test(start_paused = true)]
    async fn within_budget_bounds_remote_hosts_only() {
        let remote = Host::Remote("devbox".into());
        let slow = tokio::time::sleep(REMOTE_TIMEOUT * 2);
        assert_eq!(within_budget(&remote, slow).await, None);
        let started = tokio::time::Instant::now();
        let local = within_budget(&Host::Local, async {
            tokio::time::sleep(REMOTE_TIMEOUT * 2).await;
            1
        })
        .await;
        assert_eq!(local, Some(1), "local round trips keep their own limits");
        assert!(started.elapsed() >= REMOTE_TIMEOUT * 2);
    }

    /// A listing without start times (Docker, Podman) gets them from one
    /// inspect per running container, remembered for the next round.
    #[tokio::test]
    async fn start_times_come_from_inspect_once_and_are_cached() {
        use crate::adapter::ExecRequest;
        use crate::mock::MockAdapter;

        /// Lists containers without `started_at`, like Docker's summary, and
        /// counts inspects.
        struct Summary {
            inner: MockAdapter,
            inspects: std::sync::atomic::AtomicUsize,
        }
        #[async_trait::async_trait]
        impl RuntimeAdapter for Summary {
            fn kind(&self) -> RuntimeKind {
                self.inner.kind()
            }
            fn capabilities(&self) -> Capabilities {
                self.inner.capabilities()
            }
            async fn ping(&self) -> crate::Result<RuntimeInfo> {
                self.inner.ping().await
            }
            async fn list_containers(
                &self,
                filter: &ContainerFilter,
            ) -> crate::Result<Vec<SandboxContainer>> {
                let mut list = self.inner.list_containers(filter).await?;
                for c in &mut list {
                    c.started_at = None;
                }
                Ok(list)
            }
            async fn inspect(
                &self,
                id: &str,
            ) -> crate::Result<Option<crate::model::ContainerDetails>> {
                self.inspects
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.inner.inspect(id).await
            }
            async fn exec(
                &self,
                id: &str,
                request: ExecRequest,
            ) -> crate::Result<crate::adapter::ExecOutput> {
                self.inner.exec(id, request).await
            }
            async fn stats(&self, id: &str) -> crate::Result<crate::model::ResourceSample> {
                self.inner.stats(id).await
            }
            async fn logs(
                &self,
                id: &str,
                options: crate::adapter::LogOptions,
            ) -> crate::Result<String> {
                self.inner.logs(id, options).await
            }
            async fn events(
                &self,
            ) -> crate::Result<
                futures::stream::BoxStream<'static, crate::Result<crate::model::RuntimeEvent>>,
            > {
                self.inner.events().await
            }
            async fn stop(&self, id: &str, timeout: Option<Duration>) -> crate::Result<()> {
                self.inner.stop(id, timeout).await
            }
        }

        let summary = Arc::new(Summary {
            inner: MockAdapter::demo(),
            inspects: Default::default(),
        });
        let adapters = vec![HostedAdapter::local(summary.clone())];
        let cache = StartTimeCache::default();
        let (overview, _) =
            load_many_with_failures(&adapters, DataSource::Mock, vec![], &cache).await;
        let running: Vec<_> = overview
            .containers
            .iter()
            .filter(|c| c.container.state.is_running())
            .collect();
        assert_eq!(running.len(), 3);
        assert!(
            running.iter().all(|c| c.container.started_at.is_some()),
            "{running:?}"
        );
        assert!(
            overview
                .containers
                .iter()
                .filter(|c| !c.container.state.is_running())
                .all(|c| c.container.started_at.is_none())
        );
        let inspects = summary.inspects.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(inspects, 3, "one inspect per running container");
        assert_eq!(cache.len(), 3);

        load_many_with_failures(&adapters, DataSource::Mock, vec![], &cache).await;
        assert_eq!(
            summary.inspects.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "cached: no inspect on the next round"
        );

        // Forgetting one (as a start event does) inspects it once more; a
        // stopped container drops out of the cache.
        let first = running[0].key.clone();
        cache.forget(&first);
        summary
            .inner
            .stop(&running[1].container.id, None)
            .await
            .unwrap();
        load_many_with_failures(&adapters, DataSource::Mock, vec![], &cache).await;
        assert_eq!(
            summary.inspects.load(std::sync::atomic::Ordering::SeqCst),
            4
        );
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&running[1].key).is_none());
    }

    #[tokio::test]
    async fn empty_adapter_gives_no_containers() {
        let overview = load(Arc::new(MockAdapter::empty()), DataSource::Mock, vec![]).await;
        assert!(overview.containers.is_empty());
    }
}
