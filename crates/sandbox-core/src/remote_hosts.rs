//! Remote hosts in the live runtime: one SSH connection per configured
//! host, opened in the background, checked every refresh, reconnected with
//! backoff, and shown from the last snapshot while unreachable.
//!
//! [`LiveRuntime`](crate::live::LiveRuntime) owns one [`RemoteHosts`] and
//! installs the adapters of connected hosts next to the local ones.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use tokio::task::JoinHandle;

use crate::host::{Host, validate_alias};
use crate::model::RuntimeKind;
use crate::overview::{ContainerOverview, HostedAdapter, Overview, RuntimeFailure};
use crate::remote::{Backoff, HostState, HostStatus, RemoteDetection};
use crate::settings::RemoteHost;

/// How long one remote host may take to connect or to answer a refresh.
pub const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

/// The reason every configured host shows where SSH hosts cannot work.
pub const NOT_SUPPORTED: &str = "Remote hosts are not supported on Windows";

/// A refresh gap this many poll intervals long is taken as a wake from sleep.
const WAKE_FACTOR: u32 = 3;

/// The parts of an SSH master connection the lifecycle needs. A seam so the
/// state machine can be tested without `ssh`.
pub trait RemoteSession: Send + Sync {
    /// Whether the connection is still up.
    fn check(&self) -> BoxFuture<'_, bool>;
    fn close(&self) -> BoxFuture<'_, ()>;
    /// The last diagnostic line the connection reported, if any.
    fn last_error(&self) -> Option<String>;
}

#[cfg(unix)]
impl RemoteSession for crate::ssh::SshSession {
    fn check(&self) -> BoxFuture<'_, bool> {
        Box::pin(crate::ssh::SshSession::check(self))
    }
    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(crate::ssh::SshSession::close(self))
    }
    fn last_error(&self) -> Option<String> {
        crate::ssh::SshSession::last_error(self)
    }
}

/// A connected session with its adapters, or the reason and probes of a failure.
pub type ConnectResult = std::result::Result<
    (Arc<dyn RemoteSession>, Vec<HostedAdapter>, RemoteDetection),
    (String, Vec<String>),
>;

/// Opens a connection to an alias.
pub type Connector = Arc<dyn Fn(String) -> BoxFuture<'static, ConnectResult> + Send + Sync>;

/// Called when a background connection attempt finished, to ask for a refresh.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[cfg(unix)]
fn ssh_connector() -> Connector {
    Arc::new(|alias: String| {
        Box::pin(async move {
            let (session, adapters, detection) =
                crate::remote::connect_host(&alias, REMOTE_TIMEOUT).await?;
            Ok((session as Arc<dyn RemoteSession>, adapters, detection))
        })
    })
}

fn default_connector() -> Option<Connector> {
    #[cfg(unix)]
    {
        Some(ssh_connector())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Containers of an unreachable host as last seen: greyed out, no live sessions.
pub fn stale_snapshot(
    containers: &[ContainerOverview],
    since: DateTime<Utc>,
) -> Vec<ContainerOverview> {
    containers
        .iter()
        .cloned()
        .map(|mut c| {
            c.last_seen_at = Some(c.last_seen_at.unwrap_or(since));
            c.sessions.clear();
            c
        })
        .collect()
}

/// Open a throwaway connection, detect runtimes and close it again. Backs
/// the "Test connection" button.
pub async fn test_remote_host(alias: &str) -> HostStatus {
    let mut status = HostStatus {
        alias: alias.to_string(),
        display_name: alias.to_string(),
        state: HostState::Connecting,
        runtimes: vec![],
        probes: vec![],
    };
    if let Err(reason) = validate_alias(alias) {
        status.state = HostState::Unreachable {
            reason,
            since: Utc::now(),
        };
        return status;
    }
    let Some(connector) = default_connector() else {
        status.state = HostState::Unreachable {
            reason: NOT_SUPPORTED.into(),
            since: Utc::now(),
        };
        return status;
    };
    match connect_with_timeout(&connector, alias.to_string()).await {
        Ok((session, adapters, detection)) => {
            session.close().await;
            status.state = HostState::Connected;
            status.runtimes = kinds(&adapters);
            status.probes = detection.probes;
        }
        Err((reason, probes)) => {
            status.state = HostState::Unreachable {
                reason,
                since: Utc::now(),
            };
            status.probes = probes;
        }
    }
    status
}

async fn connect_with_timeout(connector: &Connector, alias: String) -> ConnectResult {
    match tokio::time::timeout(REMOTE_TIMEOUT, connector(alias)).await {
        Ok(result) => result,
        Err(_) => Err((
            format!("no answer within {}s", REMOTE_TIMEOUT.as_secs()),
            vec![],
        )),
    }
}

fn kinds(adapters: &[HostedAdapter]) -> Vec<RuntimeKind> {
    adapters.iter().map(|a| a.adapter.kind()).collect()
}

fn close_in_background(session: Arc<dyn RemoteSession>) {
    tokio::spawn(async move { session.close().await });
}

struct RemoteConnection {
    config: RemoteHost,
    session: Option<Arc<dyn RemoteSession>>,
    adapters: Vec<HostedAdapter>,
    status: HostStatus,
    backoff: Backoff,
    retry_at: Option<Instant>,
    /// A connection attempt running in the background.
    connecting: Option<JoinHandle<ConnectResult>>,
    last_containers: Vec<ContainerOverview>,
}

impl RemoteConnection {
    fn new(config: RemoteHost) -> Self {
        let state = if config.enabled {
            HostState::Connecting
        } else {
            HostState::Disabled
        };
        let status = HostStatus {
            alias: config.alias.clone(),
            display_name: display_name(&config),
            state,
            runtimes: vec![],
            probes: vec![],
        };
        Self {
            config,
            session: None,
            adapters: vec![],
            status,
            backoff: Backoff::new(),
            retry_at: None,
            connecting: None,
            last_containers: vec![],
        }
    }

    fn host(&self) -> Host {
        Host::Remote(self.config.alias.clone())
    }

    fn is_connected(&self) -> bool {
        self.status.state == HostState::Connected
    }
}

fn display_name(config: &RemoteHost) -> String {
    config
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&config.alias)
        .to_string()
}

/// Every configured remote host and its connection.
pub struct RemoteHosts {
    /// Held for a whole refresh round, which runs one at a time.
    connections: tokio::sync::Mutex<Vec<RemoteConnection>>,
    /// Hosts from settings, applied at the start of the next round.
    pending: Mutex<Option<Vec<RemoteHost>>>,
    reset_requested: AtomicBool,
    /// Open sessions by alias, so shutdown never waits for a running round.
    open: Mutex<HashMap<String, Arc<dyn RemoteSession>>>,
    shut_down: Arc<AtomicBool>,
    connector: Option<Connector>,
    poll_interval: Mutex<Duration>,
    /// Wall clock, so time spent asleep counts (monotonic clocks may pause).
    last_refresh: Mutex<Option<DateTime<Utc>>>,
}

impl Default for RemoteHosts {
    fn default() -> Self {
        Self::with_connector(default_connector())
    }
}

impl RemoteHosts {
    /// `None` means remote hosts cannot be used on this platform.
    pub fn with_connector(connector: Option<Connector>) -> Self {
        Self {
            connections: tokio::sync::Mutex::new(Vec::new()),
            pending: Mutex::new(None),
            reset_requested: AtomicBool::new(false),
            open: Mutex::new(HashMap::new()),
            shut_down: Arc::new(AtomicBool::new(false)),
            connector,
            poll_interval: Mutex::new(Duration::from_secs(10)),
            last_refresh: Mutex::new(None),
        }
    }

    /// Hosts from settings. Applied at the start of the next refresh.
    pub fn set_hosts(&self, hosts: Vec<RemoteHost>) {
        *self.pending.lock().expect("remote hosts poisoned") = Some(hosts);
    }

    /// Retry every unreachable host on the next refresh.
    pub fn reset_backoff(&self) {
        self.reset_requested.store(true, Ordering::SeqCst);
    }

    /// The watcher's poll interval, used to recognise a wake from sleep.
    pub fn set_poll_interval(&self, interval: Duration) {
        *self.poll_interval.lock().expect("poll interval poisoned") = interval;
    }

    /// Close every open session; no new connections are made afterwards.
    pub async fn shutdown(&self) {
        self.shut_down.store(true, Ordering::SeqCst);
        let sessions: Vec<_> = self
            .open
            .lock()
            .expect("remote sessions poisoned")
            .drain()
            .map(|(_, session)| session)
            .collect();
        futures::future::join_all(sessions.iter().map(|s| s.close())).await;
    }

    /// One refresh round: apply settings, check or (re)connect hosts, call
    /// `load` with the adapters of connected hosts, then mark hosts whose
    /// runtimes all failed as unreachable, add greyed snapshots and the host
    /// list to the overview. Returns the overview and the adapters of the
    /// hosts still connected afterwards.
    pub async fn refresh<F, Fut>(
        &self,
        wake: Option<Wake>,
        load: F,
    ) -> (Overview, Vec<HostedAdapter>)
    where
        F: FnOnce(Vec<HostedAdapter>) -> Fut,
        Fut: Future<Output = (Overview, Vec<RuntimeFailure>)>,
    {
        let mut list = self.connections.lock().await;
        if self.woke_from_sleep(Utc::now()) {
            self.reset_backoff();
        }
        let pending = self.pending.lock().expect("remote hosts poisoned").take();
        if let Some(hosts) = pending {
            self.apply_hosts(&mut list, hosts);
        }
        if self.reset_requested.swap(false, Ordering::SeqCst) {
            for c in list.iter_mut() {
                c.backoff.reset();
                c.retry_at = None;
            }
        }

        futures::future::join_all(
            list.iter_mut()
                .filter(|c| c.config.enabled)
                .map(|c| self.advance(c, wake.clone())),
        )
        .await;

        let (mut overview, failures) = load(connected_adapters(&list)).await;

        for c in list.iter_mut().filter(|c| c.is_connected()) {
            let host = c.host();
            let failed: Vec<&RuntimeFailure> = failures.iter().filter(|f| f.host == host).collect();
            let all_failed = !c.adapters.is_empty()
                && c.adapters
                    .iter()
                    .all(|a| failed.iter().any(|f| f.kind == a.adapter.kind()));
            if all_failed {
                let reason = failed
                    .iter()
                    .map(|f| format!("{}: {}", f.kind, f.reason))
                    .collect::<Vec<_>>()
                    .join("; ");
                self.mark_unreachable(c, reason);
            }
        }

        let mut stale = Vec::new();
        for c in list.iter_mut() {
            let host = c.host();
            match &c.status.state {
                HostState::Connected => {
                    c.last_containers = overview
                        .containers
                        .iter()
                        .filter(|x| x.host == host)
                        .cloned()
                        .collect();
                }
                HostState::Unreachable { since, .. } => {
                    stale.extend(stale_snapshot(&c.last_containers, *since));
                }
                HostState::Disabled | HostState::Connecting => {}
            }
        }
        if !stale.is_empty() {
            overview.containers.extend(stale);
            crate::overview::sort_containers(&mut overview.containers);
        }
        overview.hosts = list.iter().map(|c| c.status.clone()).collect();
        *self.last_refresh.lock().expect("last refresh poisoned") = Some(Utc::now());
        (overview, connected_adapters(&list))
    }

    fn woke_from_sleep(&self, now: DateTime<Utc>) -> bool {
        let interval = *self.poll_interval.lock().expect("poll interval poisoned");
        let limit =
            chrono::Duration::from_std(interval * WAKE_FACTOR).unwrap_or(chrono::Duration::MAX);
        self.last_refresh
            .lock()
            .expect("last refresh poisoned")
            .is_some_and(|last| now - last > limit)
    }

    /// Keep connections whose host is still configured and enabled, start
    /// new ones, close the rest.
    fn apply_hosts(&self, list: &mut Vec<RemoteConnection>, hosts: Vec<RemoteHost>) {
        let mut old = std::mem::take(list);
        for config in hosts {
            if validate_alias(&config.alias).is_err()
                || list.iter().any(|c| c.config.alias == config.alias)
            {
                continue;
            }
            let kept = old
                .iter()
                .position(|c| c.config.alias == config.alias && c.config.enabled && config.enabled);
            match kept {
                Some(i) => {
                    let mut c = old.remove(i);
                    c.status.display_name = display_name(&config);
                    c.config = config;
                    list.push(c);
                }
                None => list.push(RemoteConnection::new(config)),
            }
        }
        for c in old {
            self.discard(c);
        }
    }

    fn discard(&self, c: RemoteConnection) {
        if let Some(handle) = c.connecting {
            handle.abort();
        }
        if let Some(session) = c.session {
            self.forget_session(&c.config.alias);
            close_in_background(session);
        }
    }

    fn forget_session(&self, alias: &str) {
        self.open
            .lock()
            .expect("remote sessions poisoned")
            .remove(alias);
    }

    /// Move one enabled host forward: collect a finished connection attempt,
    /// check a live session, or start a new attempt when the backoff allows.
    async fn advance(&self, c: &mut RemoteConnection, wake: Option<Wake>) {
        if let Some(handle) = c.connecting.take() {
            if !handle.is_finished() {
                c.connecting = Some(handle);
                return;
            }
            match handle.await {
                Ok(Ok(connected)) => self.mark_connected(c, connected),
                Ok(Err((reason, probes))) => {
                    self.mark_unreachable(c, reason);
                    c.status.probes = probes;
                }
                Err(e) => self.mark_unreachable(c, format!("connection attempt failed: {e}")),
            }
            return;
        }
        if let Some(session) = c.session.clone() {
            if !session.check().await {
                let reason = match session.last_error() {
                    Some(line) => format!("connection lost: {line}"),
                    None => "connection lost".to_string(),
                };
                self.mark_unreachable(c, reason);
            }
            return;
        }
        if self.shut_down.load(Ordering::SeqCst) || c.retry_at.is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        let Some(connector) = self.connector.clone() else {
            if !matches!(c.status.state, HostState::Unreachable { .. }) {
                c.status.state = HostState::Unreachable {
                    reason: NOT_SUPPORTED.into(),
                    since: Utc::now(),
                };
            }
            return;
        };
        // A retry keeps showing the last failure and snapshot until it succeeds.
        if !matches!(c.status.state, HostState::Unreachable { .. }) {
            c.status.state = HostState::Connecting;
        }
        let alias = c.config.alias.clone();
        let shut_down = self.shut_down.clone();
        c.connecting = Some(tokio::spawn(async move {
            let result = match connect_with_timeout(&connector, alias).await {
                Ok((session, _, _)) if shut_down.load(Ordering::SeqCst) => {
                    session.close().await;
                    Err(("shutting down".to_string(), vec![]))
                }
                other => other,
            };
            if let Some(wake) = wake {
                wake();
            }
            result
        }));
    }

    fn mark_connected(
        &self,
        c: &mut RemoteConnection,
        (session, adapters, detection): (
            Arc<dyn RemoteSession>,
            Vec<HostedAdapter>,
            RemoteDetection,
        ),
    ) {
        if self.shut_down.load(Ordering::SeqCst) {
            close_in_background(session);
            return;
        }
        self.open
            .lock()
            .expect("remote sessions poisoned")
            .insert(c.config.alias.clone(), session.clone());
        c.session = Some(session);
        c.status.runtimes = kinds(&adapters);
        c.adapters = adapters;
        c.status.state = HostState::Connected;
        c.status.probes = detection.probes;
        c.backoff.reset();
        c.retry_at = None;
    }

    /// Close the session, drop the adapters and schedule the next attempt.
    fn mark_unreachable(&self, c: &mut RemoteConnection, reason: String) {
        if let Some(session) = c.session.take() {
            self.forget_session(&c.config.alias);
            close_in_background(session);
        }
        c.adapters.clear();
        c.status.runtimes.clear();
        let since = match &c.status.state {
            HostState::Unreachable { since, .. } => *since,
            _ => Utc::now(),
        };
        c.status.state = HostState::Unreachable { reason, since };
        c.retry_at = Some(Instant::now() + c.backoff.next_delay());
    }

    /// Wait until no connection attempt is running.
    #[cfg(test)]
    async fn settle(&self) {
        loop {
            {
                let list = self.connections.lock().await;
                if list
                    .iter()
                    .all(|c| c.connecting.as_ref().is_none_or(|h| h.is_finished()))
                {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    #[cfg(test)]
    fn backdate_last_refresh(&self, by: chrono::Duration) {
        let mut last = self.last_refresh.lock().unwrap();
        *last = last.map(|t| t - by);
    }
}

fn connected_adapters(list: &[RemoteConnection]) -> Vec<HostedAdapter> {
    list.iter()
        .filter(|c| c.is_connected())
        .flat_map(|c| c.adapters.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::adapter::RuntimeAdapter;
    use crate::mock::MockAdapter;
    use crate::overview::{DataSource, RuntimeFailure, load_many_with_failures};

    #[derive(Default)]
    struct FakeSession {
        dead: AtomicBool,
        closed: AtomicBool,
        last: StdMutex<Option<String>>,
    }

    impl RemoteSession for FakeSession {
        fn check(&self) -> BoxFuture<'_, bool> {
            let alive = !self.dead.load(Ordering::SeqCst);
            Box::pin(async move { alive })
        }
        fn close(&self) -> BoxFuture<'_, ()> {
            self.closed.store(true, Ordering::SeqCst);
            Box::pin(async {})
        }
        fn last_error(&self) -> Option<String> {
            self.last.lock().unwrap().clone()
        }
    }

    #[derive(Default)]
    struct Script {
        fail: StdMutex<Option<String>>,
        calls: AtomicUsize,
        sessions: StdMutex<Vec<Arc<FakeSession>>>,
    }

    impl Script {
        fn connector(self: &Arc<Self>) -> Connector {
            let script = self.clone();
            Arc::new(move |alias: String| {
                let script = script.clone();
                Box::pin(async move {
                    script.calls.fetch_add(1, Ordering::SeqCst);
                    if let Some(reason) = script.fail.lock().unwrap().clone() {
                        return Err((reason, vec!["docker: not found on PATH".into()]));
                    }
                    let session = Arc::new(FakeSession::default());
                    script.sessions.lock().unwrap().push(session.clone());
                    let adapters = vec![HostedAdapter {
                        host: Host::Remote(alias),
                        adapter: Arc::new(MockAdapter::demo()) as Arc<dyn RuntimeAdapter>,
                    }];
                    let detection = RemoteDetection {
                        probes: vec!["docker: unix:///var/run/docker.sock".into()],
                        ..Default::default()
                    };
                    Ok((session as Arc<dyn RemoteSession>, adapters, detection))
                })
            })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
        fn last_session(&self) -> Arc<FakeSession> {
            self.sessions.lock().unwrap().last().unwrap().clone()
        }
    }

    fn host(alias: &str) -> RemoteHost {
        RemoteHost {
            alias: alias.into(),
            display_name: None,
            enabled: true,
        }
    }

    async fn round(remotes: &RemoteHosts) -> (Overview, Vec<HostedAdapter>) {
        remotes
            .refresh(None, |adapters| async move {
                load_many_with_failures(&adapters, DataSource::Mock, vec![]).await
            })
            .await
    }

    async fn round_failing(remotes: &RemoteHosts, reason: &str) -> Overview {
        let reason = reason.to_string();
        remotes
            .refresh(None, |adapters| async move {
                let failures = adapters
                    .iter()
                    .map(|a| RuntimeFailure {
                        host: a.host.clone(),
                        kind: a.adapter.kind(),
                        endpoint: String::new(),
                        reason: reason.clone(),
                    })
                    .collect();
                let (overview, _) = load_many_with_failures(&[], DataSource::Mock, vec![]).await;
                (overview, failures)
            })
            .await
            .0
    }

    fn state(overview: &Overview, alias: &str) -> HostState {
        overview
            .hosts
            .iter()
            .find(|h| h.alias == alias)
            .expect("host listed")
            .state
            .clone()
    }

    fn setup() -> (Arc<Script>, RemoteHosts) {
        let script = Arc::new(Script::default());
        let remotes = RemoteHosts::with_connector(Some(script.connector()));
        (script, remotes)
    }

    #[tokio::test]
    async fn connects_in_the_background_then_lists_remote_containers() {
        let (script, remotes) = setup();
        remotes.set_hosts(vec![host("devbox")]);
        let (first, adapters) = round(&remotes).await;
        assert_eq!(state(&first, "devbox"), HostState::Connecting);
        assert!(adapters.is_empty());

        remotes.settle().await;
        let (next, adapters) = round(&remotes).await;
        assert_eq!(state(&next, "devbox"), HostState::Connected);
        assert_eq!(adapters.len(), 1);
        let status = &next.hosts[0];
        assert_eq!(status.display_name, "devbox");
        assert_eq!(status.runtimes, vec![RuntimeKind::Docker]);
        assert_eq!(status.probes, vec!["docker: unix:///var/run/docker.sock"]);
        assert!(next.containers.iter().all(|c| !c.host.is_local()));
        assert!(!next.containers.is_empty());
        assert_eq!(script.calls(), 1);
    }

    #[tokio::test]
    async fn failed_connect_is_unreachable_and_retried_after_backoff_or_reset() {
        let (script, remotes) = setup();
        *script.fail.lock().unwrap() = Some("Permission denied (publickey)".into());
        remotes.set_hosts(vec![host("devbox")]);
        round(&remotes).await;
        remotes.settle().await;
        let (overview, _) = round(&remotes).await;
        match state(&overview, "devbox") {
            HostState::Unreachable { reason, .. } => {
                assert_eq!(reason, "Permission denied (publickey)")
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(overview.hosts[0].probes, vec!["docker: not found on PATH"]);

        // Within the backoff delay: no new attempt.
        round(&remotes).await;
        remotes.settle().await;
        assert_eq!(script.calls(), 1);

        // "Test connection" resets the backoff: retried at once.
        *script.fail.lock().unwrap() = None;
        remotes.reset_backoff();
        round(&remotes).await;
        remotes.settle().await;
        let (overview, _) = round(&remotes).await;
        assert_eq!(script.calls(), 2);
        assert_eq!(state(&overview, "devbox"), HostState::Connected);
    }

    #[tokio::test]
    async fn lost_connection_greys_out_the_last_snapshot() {
        let (script, remotes) = setup();
        remotes.set_hosts(vec![host("devbox")]);
        round(&remotes).await;
        remotes.settle().await;
        let (connected, _) = round(&remotes).await;
        let seen = connected.containers.len();
        assert!(seen > 0);

        let session = script.last_session();
        *session.last.lock().unwrap() = Some("Connection reset by peer".into());
        session.dead.store(true, Ordering::SeqCst);
        let (lost, adapters) = round(&remotes).await;
        assert!(adapters.is_empty(), "adapters of a lost host are dropped");
        match state(&lost, "devbox") {
            HostState::Unreachable { reason, .. } => {
                assert_eq!(reason, "connection lost: Connection reset by peer")
            }
            other => panic!("{other:?}"),
        }
        assert!(
            session.closed.load(Ordering::SeqCst) || {
                remotes.settle().await;
                tokio::task::yield_now().await;
                session.closed.load(Ordering::SeqCst)
            }
        );
        assert_eq!(lost.containers.len(), seen);
        assert!(lost.containers.iter().all(|c| c.last_seen_at.is_some()));
        assert!(lost.containers.iter().all(|c| c.sessions.is_empty()));
    }

    #[tokio::test]
    async fn host_whose_runtimes_all_fail_becomes_unreachable() {
        let (script, remotes) = setup();
        remotes.set_hosts(vec![host("devbox")]);
        round(&remotes).await;
        remotes.settle().await;
        round(&remotes).await;
        let overview = round_failing(&remotes, "no answer within 10s").await;
        match state(&overview, "devbox") {
            HostState::Unreachable { reason, .. } => {
                assert_eq!(reason, "docker: no answer within 10s")
            }
            other => panic!("{other:?}"),
        }
        remotes.settle().await;
        tokio::task::yield_now().await;
        assert!(script.last_session().closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn disabled_hosts_are_listed_but_never_connected() {
        let (script, remotes) = setup();
        remotes.set_hosts(vec![RemoteHost {
            display_name: Some("Build box".into()),
            enabled: false,
            ..host("devbox")
        }]);
        let (overview, _) = round(&remotes).await;
        remotes.settle().await;
        assert_eq!(state(&overview, "devbox"), HostState::Disabled);
        assert_eq!(overview.hosts[0].display_name, "Build box");
        assert_eq!(script.calls(), 0);
    }

    #[tokio::test]
    async fn removed_hosts_are_closed_and_dropped() {
        let (script, remotes) = setup();
        remotes.set_hosts(vec![host("devbox")]);
        round(&remotes).await;
        remotes.settle().await;
        round(&remotes).await;
        remotes.set_hosts(vec![]);
        let (overview, adapters) = round(&remotes).await;
        assert!(overview.hosts.is_empty());
        assert!(adapters.is_empty());
        assert!(overview.containers.is_empty());
        tokio::task::yield_now().await;
        assert!(script.last_session().closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn shutdown_closes_every_session() {
        let (script, remotes) = setup();
        remotes.set_hosts(vec![host("devbox")]);
        round(&remotes).await;
        remotes.settle().await;
        round(&remotes).await;
        remotes.shutdown().await;
        assert!(script.last_session().closed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn long_gap_between_refreshes_resets_the_backoff() {
        let (script, remotes) = setup();
        *script.fail.lock().unwrap() = Some("timeout".into());
        remotes.set_poll_interval(Duration::from_secs(10));
        remotes.set_hosts(vec![host("devbox")]);
        round(&remotes).await;
        remotes.settle().await;
        round(&remotes).await;
        assert_eq!(script.calls(), 1);

        // Pretend the machine slept for a minute.
        remotes.backdate_last_refresh(chrono::Duration::seconds(60));
        round(&remotes).await;
        remotes.settle().await;
        assert_eq!(script.calls(), 2, "wake from sleep retries at once");
    }

    #[tokio::test]
    async fn without_a_connector_hosts_are_unsupported() {
        let remotes = RemoteHosts::with_connector(None);
        remotes.set_hosts(vec![host("devbox")]);
        let (overview, _) = round(&remotes).await;
        match state(&overview, "devbox") {
            HostState::Unreachable { reason, .. } => assert_eq!(reason, NOT_SUPPORTED),
            other => panic!("{other:?}"),
        }
        assert_eq!(NOT_SUPPORTED, "Remote hosts are not supported on Windows");
    }
}
