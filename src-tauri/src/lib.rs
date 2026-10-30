//! Tauri shell around `sandbox-core`. Commands are thin: resolve the adapter,
//! call core, return serialisable state.

use std::sync::Arc;

use sandbox_core::RuntimeAdapter;
use sandbox_core::detect::{DetectedRuntime, Environment, detect};
use sandbox_core::mock::MockAdapter;
use sandbox_core::overview::{DataSource, Overview};
use tauri::State;

/// Shared application state.
pub struct AppState {
    /// `Some` when an adapter is wired. In M0 that is only the mock adapter,
    /// enabled with `SANDBOX_WATCHER_MOCK=1`. The Docker adapter arrives in M1.
    adapter: Option<Arc<dyn RuntimeAdapter>>,
    source: DataSource,
}

impl AppState {
    pub fn from_env() -> Self {
        let mock = std::env::var("SANDBOX_WATCHER_MOCK").is_ok_and(|v| v == "1" || v == "true");
        if mock {
            Self {
                adapter: Some(Arc::new(MockAdapter::demo())),
                source: DataSource::Mock,
            }
        } else {
            Self {
                adapter: None,
                source: DataSource::Live,
            }
        }
    }
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    if let Some(adapter) = &state.adapter {
        return Ok(sandbox_core::overview::load(adapter.clone(), state.source).await);
    }
    let candidates = detect(&Environment::current());
    if candidates.is_empty() {
        Ok(Overview::not_detected(DataSource::Live))
    } else {
        Ok(Overview::detected_only(candidates))
    }
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
