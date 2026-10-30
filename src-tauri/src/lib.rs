//! Tauri shell around `sandbox-core`. Commands are thin: resolve the adapter,
//! call core, return serialisable state. Runtime events are forwarded to the
//! webview as the `runtime-event` Tauri event.

use std::sync::Arc;

use sandbox_core::RuntimeAdapter;
use sandbox_core::detect::{DetectedRuntime, Environment, detect};
use sandbox_core::live::LiveRuntime;
use sandbox_core::mock::MockAdapter;
use sandbox_core::overview::{DataSource, Overview};
use tauri::{Emitter, Manager, State};

/// Name of the Tauri event the UI listens to for refresh triggers.
pub const RUNTIME_EVENT: &str = "runtime-event";

/// Shared application state.
pub enum AppState {
    /// `SANDBOX_WATCHER_MOCK=1`: demo data, no runtime access.
    Mock(Arc<dyn RuntimeAdapter>),
    /// Detect the runtime and keep the adapter between refreshes.
    Live(Arc<LiveRuntime>),
}

impl AppState {
    pub fn from_env() -> Self {
        let mock = std::env::var("SANDBOX_WATCHER_MOCK").is_ok_and(|v| v == "1" || v == "true");
        if mock {
            Self::Mock(Arc::new(MockAdapter::demo()))
        } else {
            Self::Live(Arc::new(LiveRuntime::new()))
        }
    }
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    Ok(match state.inner() {
        AppState::Mock(adapter) => {
            sandbox_core::overview::load(adapter.clone(), DataSource::Mock, vec![]).await
        }
        AppState::Live(live) => live.overview().await,
    })
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
        .manage(AppState::from_env())
        .setup(|app| {
            if let AppState::Live(live) = app.state::<AppState>().inner() {
                let handle = app.handle().clone();
                live.set_listener(Arc::new(move |event| {
                    let _ = handle.emit(RUNTIME_EVENT, &event);
                }));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            detect_runtimes,
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sandbox Watcher");
}
