//! Background refresh loop. Owns the latest overview and network reports,
//! feeds the tray and notifications, and tells the webview when to re-render.
//! Runs regardless of whether the window is visible.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::adapter::ContainerFilter;
use sandbox_core::live::LiveRuntime;
use sandbox_core::mock::MockAdapter;
use sandbox_core::network::{NetworkReport, collect_all};
use sandbox_core::notify::{Notification, blocked_bursts, diff, recent_blocked};
use sandbox_core::overview::{DataSource, Overview, load};
use sandbox_core::settings::Settings;
use sandbox_core::stats_hub::StatsHub;
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
    /// `SANDBOX_WATCHER_MOCK=1`: demo data, no runtime access.
    Mock(Arc<dyn RuntimeAdapter>),
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
    app: AppHandle,
}

impl Watcher {
    pub fn new(app: AppHandle, settings: Settings) -> Arc<Self> {
        let mock = std::env::var("SANDBOX_WATCHER_MOCK").is_ok_and(|v| v == "1" || v == "true");
        let mode = if mock {
            Mode::Mock(Arc::new(MockAdapter::demo()))
        } else {
            let live = Arc::new(LiveRuntime::new());
            live.set_preferred(settings.runtime_override);
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
            app,
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().expect("settings poisoned").clone()
    }

    /// Store new settings and apply what the watcher owns.
    pub fn apply_settings(&self, settings: Settings) {
        if let Mode::Live(live) = &self.mode {
            live.set_preferred(settings.runtime_override);
        }
        *self.settings.write().expect("settings poisoned") = settings.clone();
        let _ = self.app.emit(SETTINGS_UPDATED, &settings);
        self.poke();
    }

    pub fn overview(&self) -> Option<Overview> {
        self.overview.read().expect("overview poisoned").clone()
    }

    pub fn network(&self) -> Vec<NetworkReport> {
        self.network.read().expect("network poisoned").clone()
    }

    pub fn adapters(&self) -> Vec<Arc<dyn RuntimeAdapter>> {
        match &self.mode {
            Mode::Mock(adapter) => vec![adapter.clone()],
            Mode::Live(live) => live.adapters(),
        }
    }

    pub fn adapter_for(&self, container_id: &str) -> Result<Arc<dyn RuntimeAdapter>, String> {
        match &self.mode {
            Mode::Mock(adapter) => Some(adapter.clone()),
            Mode::Live(live) => live.adapter_for(container_id),
        }
        .ok_or_else(|| format!("no runtime known for container {container_id}"))
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
            Mode::Mock(adapter) => load(adapter.clone(), DataSource::Mock, vec![]).await,
            Mode::Live(live) => live.overview().await,
        };

        // Stats tracking per adapter.
        let sets: Vec<(Arc<dyn RuntimeAdapter>, Vec<String>)> = self
            .adapters()
            .into_iter()
            .map(|adapter| {
                let kind = adapter.kind();
                let ids = overview
                    .containers
                    .iter()
                    .filter(|c| c.runtime == kind && c.container.state.is_running())
                    .map(|c| c.container.id.clone())
                    .collect();
                (adapter, ids)
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

        // Network logs: one more round trip, published separately.
        let mut reports = Vec::new();
        for adapter in self.adapters() {
            if let Ok(containers) = adapter.list_containers(&ContainerFilter::running()).await {
                reports.extend(collect_all(&adapter, &containers, Utc::now()).await);
            }
        }
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
