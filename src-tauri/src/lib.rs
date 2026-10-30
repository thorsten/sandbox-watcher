//! Tauri shell around `sandbox-core`. Commands are thin: resolve the adapter,
//! call core, return serialisable state. Runtime events are forwarded to the
//! webview as the `runtime-event` Tauri event.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use sandbox_core::RuntimeAdapter;
use sandbox_core::actions::{STOP_TIMEOUT, StopResult, terminal_launches};
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
    /// Detect runtimes and keep one adapter per runtime between refreshes.
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

    fn adapters(&self) -> Vec<Arc<dyn RuntimeAdapter>> {
        match &self.mode {
            Mode::Mock(adapter) => vec![adapter.clone()],
            Mode::Live(live) => live.adapters(),
        }
    }

    fn adapter_for(&self, container_id: &str) -> Result<Arc<dyn RuntimeAdapter>, String> {
        match &self.mode {
            Mode::Mock(adapter) => Some(adapter.clone()),
            Mode::Live(live) => live.adapter_for(container_id),
        }
        .ok_or_else(|| format!("no runtime known for container {container_id}"))
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
    let sets: Vec<(Arc<dyn RuntimeAdapter>, Vec<String>)> = state
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
    state.stats.sync(&sets);
    Ok(overview)
}

/// Network activity of every running sandbox container across runtimes.
/// Separate from the overview so a slow log read never delays the list.
#[tauri::command]
async fn get_network_overview(state: State<'_, AppState>) -> Result<Vec<NetworkReport>, String> {
    let mut reports = Vec::new();
    for adapter in state.adapters() {
        let Ok(containers) = adapter.list_containers(&ContainerFilter::running()).await else {
            continue;
        };
        reports.extend(sandbox_core::network::collect_all(&adapter, &containers, Utc::now()).await);
    }
    Ok(reports)
}

/// Resource history per running container, oldest sample first.
#[tauri::command]
fn get_stats(state: State<'_, AppState>) -> HashMap<String, Vec<ResourceSample>> {
    state.stats.snapshot()
}

/// Mounts, ports, env names and user of one container, mounts classified
/// and host paths spelled the way this OS shows them.
#[tauri::command]
async fn get_container_details(
    state: State<'_, AppState>,
    container_id: String,
) -> Result<Option<ContainerDetails>, String> {
    let adapter = state.adapter_for(&container_id)?;
    let mut details = adapter
        .inspect(&container_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(details) = details.as_mut() {
        sandbox_core::mounts::classify(details);
        sandbox_core::paths::annotate_host_paths(details, cfg!(windows));
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
    let adapter = state.adapter_for(&container_id)?;
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

/// Show a mount's host path in the file manager. Only bind mount sources of
/// the given container are accepted, so the webview cannot open arbitrary
/// locations. On Windows the WSL spelling is translated first.
#[tauri::command]
async fn reveal_mount(
    app: AppHandle,
    state: State<'_, AppState>,
    container_id: String,
    path: String,
) -> Result<(), String> {
    let adapter = state.adapter_for(&container_id)?;
    let mut details = adapter
        .inspect(&container_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("container {container_id} not found"))?;
    sandbox_core::paths::annotate_host_paths(&mut details, cfg!(windows));
    let target = details
        .mounts
        .iter()
        .filter(|m| m.kind == "bind")
        .find(|m| m.source == path || m.host_path.as_deref() == Some(path.as_str()))
        .and_then(|m| m.host_path.clone())
        .ok_or_else(|| "path is not a mount of this container".to_string())?;
    app.opener()
        .reveal_item_in_dir(&target)
        .map_err(|e| e.to_string())
}

/// Stop one container. The UI asks for confirmation first.
#[tauri::command]
async fn stop_container(state: State<'_, AppState>, container_id: String) -> Result<(), String> {
    let adapter = state.adapter_for(&container_id)?;
    adapter
        .stop(&container_id, Some(STOP_TIMEOUT))
        .await
        .map_err(|e| e.to_string())
}

/// Stop every running sandbox container across runtimes.
#[tauri::command]
async fn stop_all(state: State<'_, AppState>) -> Result<Vec<StopResult>, String> {
    Ok(sandbox_core::actions::stop_all(&state.adapters(), STOP_TIMEOUT).await)
}

/// Host path of the container's project directory, if it has a bind-mounted one.
#[tauri::command]
async fn get_project_path(
    state: State<'_, AppState>,
    container_id: String,
) -> Result<Option<String>, String> {
    let adapter = state.adapter_for(&container_id)?;
    sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
        .await
        .map_err(|e| e.to_string())
}

/// Open the project directory in the file manager.
#[tauri::command]
async fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    container_id: String,
) -> Result<(), String> {
    let adapter = state.adapter_for(&container_id)?;
    let dir = sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "this container has no bind-mounted project directory".to_string())?;
    app.opener()
        .open_path(&dir, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Open a terminal in the project directory. Tries the platform's usual
/// terminals in order until one starts.
#[tauri::command]
async fn open_terminal(state: State<'_, AppState>, container_id: String) -> Result<String, String> {
    let adapter = state.adapter_for(&container_id)?;
    let dir = sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "this container has no bind-mounted project directory".to_string())?;
    let terminal_env = std::env::var("TERMINAL").ok();
    let launches = terminal_launches(std::env::consts::OS, &dir, terminal_env.as_deref());
    let mut errors = Vec::new();
    for launch in &launches {
        let mut command = std::process::Command::new(&launch.program);
        command.args(&launch.args);
        if let Some(cwd) = &launch.cwd {
            command.current_dir(cwd);
        }
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        match command.spawn() {
            Ok(_) => return Ok(launch.program.clone()),
            Err(e) => errors.push(format!("{}: {e}", launch.program)),
        }
    }
    Err(format!(
        "no terminal could be started ({})",
        errors.join("; ")
    ))
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
            stop_container,
            stop_all,
            get_project_path,
            open_project,
            open_terminal,
            detect_runtimes,
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sandbox Watcher");
}
