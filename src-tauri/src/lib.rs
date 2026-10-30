//! Tauri shell around `sandbox-core`. Commands are thin: resolve the adapter,
//! call core, return serialisable state.

use std::sync::Arc;

use sandbox_core::RuntimeAdapter;
use sandbox_core::detect::{DetectedRuntime, Environment, detect};
use sandbox_core::live::LiveRuntime;
use sandbox_core::mock::MockAdapter;
use sandbox_core::overview::{DataSource, Overview};
use tauri::State;

/// Shared application state.
pub enum AppState {
    /// `SANDBOX_WATCHER_MOCK=1`: demo data, no runtime access.
    Mock(Arc<dyn RuntimeAdapter>),
    /// Detect the runtime and keep the adapter between refreshes.
    Live(LiveRuntime),
}

impl AppState {
    pub fn from_env() -> Self {
        let mock = std::env::var("SANDBOX_WATCHER_MOCK").is_ok_and(|v| v == "1" || v == "true");
        if mock {
            Self::Mock(Arc::new(MockAdapter::demo()))
        } else {
            Self::Live(LiveRuntime::new())
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
        .invoke_handler(tauri::generate_handler![
            get_overview,
            detect_runtimes,
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sandbox Watcher");
}
