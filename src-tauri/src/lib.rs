//! Tauri shell around `sandbox-core`. Commands are thin: resolve the adapter,
//! call core, return serialisable state. Runtime events are forwarded to the
//! webview as the `runtime-event` Tauri event.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::adapter::{ContainerFilter, LogOptions};
use sandbox_core::detect::{DetectedRuntime, Environment, detect};
use sandbox_core::live::LiveRuntime;
use sandbox_core::mock::MockAdapter;
use sandbox_core::model::{ContainerDetails, ResourceSample};
use sandbox_core::network::NetworkReport;
use sandbox_core::overview::{DataSource, Overview};
use sandbox_core::stats_hub::StatsHub;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

/// Name of the Tauri event the UI listens to for refresh triggers.
pub const RUNTIME_EVENT: &str = "runtime-event";

enum Mode {
    /// `SANDBOX_WATCHER_MOCK=1`: demo data, no runtime access.
    Mock(Arc<dyn RuntimeAdapter>),
    /// Detect the runtime and keep the adapter between refreshes.
    Live(Arc<LiveRuntime>),
}

/// Shared application state.
pub struct AppState {
    mode: Mode,
    stats: StatsHub,
}

impl AppState {
    pub fn from_env() -> Self {
        let mock = std::env::var("SANDBOX_WATCHER_MOCK").is_ok_and(|v| v == "1" || v == "true");
        let mode = if mock {
            Mode::Mock(Arc::new(MockAdapter::demo()))
        } else {
            Mode::Live(Arc::new(LiveRuntime::new()))
        };
        Self {
            mode,
            stats: StatsHub::new(),
        }
    }

    fn adapter(&self) -> Option<Arc<dyn RuntimeAdapter>> {
        match &self.mode {
            Mode::Mock(adapter) => Some(adapter.clone()),
            Mode::Live(live) => live.adapter(),
        }
    }

    fn adapter_or_err(&self) -> Result<Arc<dyn RuntimeAdapter>, String> {
        self.adapter()
            .ok_or_else(|| "no container runtime connected".to_string())
    }
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    let overview = match &state.mode {
        Mode::Mock(adapter) => {
            sandbox_core::overview::load(adapter.clone(), DataSource::Mock, vec![]).await
        }
        Mode::Live(live) => live.overview().await,
    };
    match state.adapter() {
        Some(adapter) => {
            let running: Vec<String> = overview
                .containers
                .iter()
                .filter(|c| c.container.state.is_running())
                .map(|c| c.container.id.clone())
                .collect();
            state.stats.sync(&adapter, &running);
        }
        None => state.stats.clear(),
    }
    Ok(overview)
}

/// Network activity of every running sandbox container. Separate from the
/// overview so a slow log read never delays the container list.
#[tauri::command]
async fn get_network_overview(state: State<'_, AppState>) -> Result<Vec<NetworkReport>, String> {
    let Some(adapter) = state.adapter() else {
        return Ok(vec![]);
    };
    let containers = adapter
        .list_containers(&ContainerFilter::running())
        .await
        .map_err(|e| e.to_string())?;
    Ok(sandbox_core::network::collect_all(&adapter, &containers, Utc::now()).await)
}

/// Resource history per running container, oldest sample first.
#[tauri::command]
fn get_stats(state: State<'_, AppState>) -> HashMap<String, Vec<ResourceSample>> {
    state.stats.snapshot()
}

/// Mounts, ports, env names and user of one container, mounts classified.
#[tauri::command]
async fn get_container_details(
    state: State<'_, AppState>,
    container_id: String,
) -> Result<Option<ContainerDetails>, String> {
    let adapter = state.adapter_or_err()?;
    let mut details = adapter
        .inspect(&container_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(details) = details.as_mut() {
        sandbox_core::mounts::classify(details);
    }
    Ok(details)
}

/// Last `tail` lines of the container's own log.
#[tauri::command]
async fn get_logs(
    state: State<'_, AppState>,
    container_id: String,
    tail: Option<usize>,
) -> Result<String, String> {
    let adapter = state.adapter_or_err()?;
    adapter
        .logs(
            &container_id,
            LogOptions {
                tail: Some(tail.unwrap_or(200).clamp(1, 5000)),
                timestamps: false,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

/// Show a mount's host path in the file manager. Only paths that are bind
/// mount sources of the given container are accepted, so the webview cannot
/// open arbitrary locations.
#[tauri::command]
async fn reveal_mount(
    app: AppHandle,
    state: State<'_, AppState>,
    container_id: String,
    path: String,
) -> Result<(), String> {
    let adapter = state.adapter_or_err()?;
    let details = adapter
        .inspect(&container_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("container {container_id} not found"))?;
    let allowed = details
        .mounts
        .iter()
        .any(|m| m.kind == "bind" && m.source == path);
    if !allowed {
        return Err("path is not a mount of this container".into());
    }
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn detect_runtimes() -> Vec<DetectedRuntime> {
    detect(&Environment::current())
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::from_env())
        .setup(|app| {
            if let Mode::Live(live) = &app.state::<AppState>().inner().mode {
                let handle = app.handle().clone();
                live.set_listener(Arc::new(move |event| {
                    let _ = handle.emit(RUNTIME_EVENT, &event);
                }));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_network_overview,
            get_stats,
            get_container_details,
            get_logs,
            reveal_mount,
            detect_runtimes,
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sandbox Watcher");
}
