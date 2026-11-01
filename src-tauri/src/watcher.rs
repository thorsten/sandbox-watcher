//! Background refresh loop. Owns the latest overview and network reports,
//! feeds the tray and notifications, and tells the webview when to re-render.
//! Runs regardless of whether the window is visible.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::activity::collect_insights;
use sandbox_core::adapter::ContainerFilter;
use sandbox_core::history::{History, HistoryQuery, HistoryView};
use sandbox_core::host::{Host, container_key, parse_container_key};
use sandbox_core::live::LiveRuntime;
use sandbox_core::mock::MockAdapter;
use sandbox_core::model::RuntimeKind;
use sandbox_core::network::{NetworkReport, collect_all};
use sandbox_core::notify::{Notification, blocked_bursts, diff, recent_blocked};
use sandbox_core::overview::{DataSource, HostedAdapter, Overview, load_many, within_budget};
use sandbox_core::remote::{HostState, HostStatus};
use sandbox_core::runner::CommandOutput;
use sandbox_core::settings::Settings;
use sandbox_core::stats_hub::{StatsHub, StatsSet};
use sandbox_core::transcripts::TranscriptCache;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{Notify, mpsc};

use crate::tray;

/// Webview events.
pub const OVERVIEW_UPDATED: &str = "overview-updated";
pub const NETWORK_UPDATED: &str = "network-updated";
pub const SETTINGS_UPDATED: &str = "settings-updated";

const TRIGGER_DEBOUNCE: Duration = Duration::from_millis(300);

pub enum Mode {
    /// `SANDBOX_WATCHER_MOCK=1`: demo data for the local machine and the
    /// remote host `devbox`, no runtime access.
    Mock(Vec<HostedAdapter>),
    /// Detect runtimes and keep one adapter per runtime between refreshes.
    Live(Arc<LiveRuntime>),
}

pub struct Watcher {
    pub mode: Mode,
    pub stats: StatsHub,
    settings: RwLock<Settings>,
    overview: RwLock<Option<Overview>>,
    network: RwLock<Vec<NetworkReport>>,
    blocked_counts: Mutex<HashMap<String, u64>>,
    trigger: mpsc::Sender<()>,
    trigger_rx: Mutex<Option<mpsc::Receiver<()>>>,
    /// Signalled after every completed refresh.
    refreshed: Notify,
    history: Mutex<History>,
    history_path: Option<PathBuf>,
    history_saved_at: Mutex<Option<Instant>>,
    /// Read positions and running totals of agent transcripts, shared by the
    /// history refresh and the activity panel so each byte is read once.
    transcripts: TranscriptCache,
    app: AppHandle,
}

impl Watcher {
    pub fn new(app: AppHandle, settings: Settings, history_path: Option<PathBuf>) -> Arc<Self> {
        let mut history = history_path
            .as_deref()
            .map(History::load)
            .unwrap_or_default();
        history.close_stale();
        history.prune(settings.history_retention_days, Utc::now());
        let mock = std::env::var("SANDBOX_WATCHER_MOCK").is_ok_and(|v| v == "1" || v == "true");
        let mode = if mock {
            Mode::Mock(vec![
                HostedAdapter::local(Arc::new(MockAdapter::demo())),
                HostedAdapter {
                    host: Host::Remote("devbox".into()),
                    adapter: Arc::new(MockAdapter::remote_demo()),
                },
            ])
        } else {
            let live = Arc::new(LiveRuntime::new());
            live.set_preferred(settings.runtime_override);
            live.set_remote_hosts(settings.remote_hosts.clone());
            live.set_poll_interval(Duration::from_secs(settings.poll_interval_secs));
            Mode::Live(live)
        };
        let (trigger, rx) = mpsc::channel(16);
        Arc::new(Self {
            mode,
            stats: StatsHub::new(),
            settings: RwLock::new(settings),
            overview: RwLock::new(None),
            network: RwLock::new(Vec::new()),
            blocked_counts: Mutex::new(HashMap::new()),
            trigger,
            trigger_rx: Mutex::new(Some(rx)),
            refreshed: Notify::new(),
            history: Mutex::new(history),
            history_path,
            history_saved_at: Mutex::new(None),
            transcripts: TranscriptCache::default(),
            app,
        })
    }

    pub fn transcripts(&self) -> &TranscriptCache {
        &self.transcripts
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().expect("settings poisoned").clone()
    }

    /// Store new settings and apply what the watcher owns.
    pub fn apply_settings(&self, settings: Settings) {
        if let Mode::Live(live) = &self.mode {
            live.set_preferred(settings.runtime_override);
            live.set_remote_hosts(settings.remote_hosts.clone());
            live.set_poll_interval(Duration::from_secs(settings.poll_interval_secs));
        }
        *self.settings.write().expect("settings poisoned") = settings.clone();
        let _ = self.app.emit(SETTINGS_UPDATED, &settings);
        self.poke();
    }

    /// Retry unreachable remote hosts now. Does nothing in mock mode.
    pub fn reset_backoff(&self) {
        if let Mode::Live(live) = &self.mode {
            live.reset_backoff();
            self.poke();
        }
    }

    /// Close remote sessions. Call once on exit.
    pub async fn shutdown(&self) {
        if let Mode::Live(live) = &self.mode {
            live.shutdown().await;
        }
    }

    pub fn overview(&self) -> Option<Overview> {
        self.overview.read().expect("overview poisoned").clone()
    }

    pub fn network(&self) -> Vec<NetworkReport> {
        self.network.read().expect("network poisoned").clone()
    }

    pub fn adapters(&self) -> Vec<HostedAdapter> {
        match &self.mode {
            Mode::Mock(adapters) => adapters.clone(),
            Mode::Live(live) => live.adapters(),
        }
    }

    /// Run a shell command line on a connected remote host.
    pub async fn run_on_host(
        &self,
        alias: &str,
        command: &str,
        timeout: Duration,
    ) -> Result<CommandOutput, String> {
        match &self.mode {
            Mode::Mock(_) => Err("no remote shell in mock mode".into()),
            Mode::Live(live) => live.run_on_host(alias, command, timeout).await,
        }
    }

    /// The adapter for a container key and the container's raw id.
    pub fn adapter_for(&self, key: &str) -> Result<(Arc<dyn RuntimeAdapter>, String), String> {
        match &self.mode {
            Mode::Mock(adapters) => {
                let (host, id) = parse_container_key(key);
                adapters
                    .iter()
                    .find(|a| a.host == host)
                    .map(|a| (a.adapter.clone(), id))
            }
            Mode::Live(live) => live.adapter_for(key),
        }
        .ok_or_else(|| format!("no runtime known for container {key}"))
    }

    pub fn history_view(&self, query: &HistoryQuery) -> HistoryView {
        self.history
            .lock()
            .expect("history poisoned")
            .query(query, Utc::now())
    }

    pub fn clear_history(&self) {
        let mut history = self.history.lock().expect("history poisoned");
        *history = History::default();
        drop(history);
        self.save_history(true);
    }

    /// Persist history when something changed, or at most once a minute for
    /// sighting timestamps.
    fn save_history(&self, force: bool) {
        let Some(path) = &self.history_path else {
            return;
        };
        let mut saved_at = self
            .history_saved_at
            .lock()
            .expect("history saved_at poisoned");
        let due = force || saved_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(60));
        if !due {
            return;
        }
        let history = self.history.lock().expect("history poisoned");
        if history.save(path).is_ok() {
            *saved_at = Some(Instant::now());
        }
    }

    /// Ask for a refresh soon. Cheap; bursts collapse into one.
    pub fn poke(&self) {
        let _ = self.trigger.try_send(());
    }

    /// Refresh now and wait for it.
    pub async fn refresh_and_wait(&self) {
        let notified = self.refreshed.notified();
        self.poke();
        notified.await;
    }

    /// Start the loop. Call once from setup.
    pub fn start(self: &Arc<Self>) {
        let watcher = self.clone();
        let mut rx = self
            .trigger_rx
            .lock()
            .expect("trigger poisoned")
            .take()
            .expect("start called twice");
        tauri::async_runtime::spawn(async move {
            loop {
                watcher.refresh().await;
                watcher.refreshed.notify_waiters();
                let poll = Duration::from_secs(watcher.settings().poll_interval_secs);
                tokio::select! {
                    _ = rx.recv() => {
                        // Collapse bursts of triggers (events arrive in groups).
                        tokio::time::sleep(TRIGGER_DEBOUNCE).await;
                        while rx.try_recv().is_ok() {}
                    }
                    () = tokio::time::sleep(poll) => {}
                }
            }
        });
    }

    async fn refresh(&self) {
        let overview = match &self.mode {
            Mode::Mock(adapters) => {
                let mut overview = load_many(adapters, DataSource::Mock, vec![]).await;
                overview.hosts = vec![HostStatus {
                    alias: "devbox".into(),
                    display_name: "devbox".into(),
                    state: HostState::Connected,
                    runtimes: vec![RuntimeKind::Docker],
                    probes: vec![],
                }];
                overview
            }
            Mode::Live(live) => live.overview().await,
        };

        // Stats tracking per adapter.
        let sets: Vec<StatsSet> = self
            .adapters()
            .into_iter()
            .map(|hosted| {
                let kind = hosted.adapter.kind();
                let ids = overview
                    .containers
                    .iter()
                    .filter(|c| {
                        c.host == hosted.host && c.runtime == kind && c.container.state.is_running()
                    })
                    .map(|c| (c.key.clone(), c.container.id.clone()))
                    .collect();
                (hosted.adapter, ids)
            })
            .collect();
        self.stats.sync(&sets);

        // Notifications from the snapshot diff.
        let settings = self.settings();
        let previous = self.overview();
        let mut notifications: Vec<Notification> = Vec::new();
        if let Some(previous) = &previous {
            notifications.extend(diff(previous, &overview, &settings.notifications));
        }
        *self.overview.write().expect("overview poisoned") = Some(overview.clone());
        let _ = self.app.emit(OVERVIEW_UPDATED, &overview);
        tray::update(&self.app, &overview);

        // Transcript state only for containers that are running and reachable.
        let live_keys: Vec<&str> = overview
            .containers
            .iter()
            .filter(|c| c.container.state.is_running() && c.last_seen_at.is_none())
            .map(|c| c.key.as_str())
            .collect();
        self.transcripts.retain_scopes(&live_keys);

        // History: fold the snapshot in, then attach usage for agent sessions.
        if settings.history_enabled {
            let now = Utc::now();
            let mut changed = {
                let mut history = self.history.lock().expect("history poisoned");
                let changed = history.observe(&overview, now);
                history.prune(settings.history_retention_days, now) || changed
            };
            // Containers run at the same time; on a remote host each gets
            // REMOTE_TIMEOUT, so a host that stalls after its overview loaded
            // cannot delay notifications or the next round. Transcripts are
            // read from where the last refresh stopped, so a long one is
            // caught up over a few rounds rather than re-read.
            let insights = overview
                .containers
                .iter()
                .filter(|c| {
                    c.container.state.is_running()
                        && c.last_seen_at.is_none()
                        && !c.sessions.is_empty()
                })
                .filter_map(|c| {
                    let (adapter, id) = self.adapter_for(&c.key).ok()?;
                    let prices = &settings.prices;
                    let cache = &self.transcripts;
                    Some(async move {
                        let insights = within_budget(
                            &c.host,
                            collect_insights(&adapter, &id, &c.key, &c.sessions, cache, prices),
                        )
                        .await
                        .unwrap_or_default();
                        (c, insights)
                    })
                });
            for (c, insights) in futures::future::join_all(insights).await {
                let mut history = self.history.lock().expect("history poisoned");
                changed = history.record_insights(&c.key, &insights) || changed;
            }
            self.save_history(changed);
        }

        // Network logs: one more round trip, published separately.
        // Adapters run at the same time, remote ones within REMOTE_TIMEOUT.
        let collections = self.adapters().into_iter().map(|hosted| async move {
            let collect = async {
                let adapter = &hosted.adapter;
                match adapter.list_containers(&ContainerFilter::running()).await {
                    Ok(containers) => collect_all(adapter, &containers, Utc::now()).await,
                    Err(_) => Vec::new(),
                }
            };
            let mut reports = within_budget(&hosted.host, collect)
                .await
                .unwrap_or_default();
            for report in &mut reports {
                report.container_id = container_key(&hosted.host, &report.container_id);
            }
            reports
        });
        let reports: Vec<NetworkReport> = futures::future::join_all(collections)
            .await
            .into_iter()
            .flatten()
            .collect();
        let counts = recent_blocked(&reports, Utc::now(), settings.blocked_burst_window_secs);
        {
            let mut previous_counts = self.blocked_counts.lock().expect("blocked counts poisoned");
            if previous.is_some() {
                notifications.extend(blocked_bursts(
                    &previous_counts,
                    &counts,
                    &overview,
                    settings.blocked_burst_threshold,
                    &settings.notifications,
                ));
            }
            *previous_counts = counts;
        }
        *self.network.write().expect("network poisoned") = reports.clone();
        let _ = self.app.emit(NETWORK_UPDATED, &reports);

        for n in notifications {
            let _ = self
                .app
                .notification()
                .builder()
                .title(&n.title)
                .body(&n.body)
                .show();
        }
    }
}
