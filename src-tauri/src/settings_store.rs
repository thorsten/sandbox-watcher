//! Persist `Settings` as JSON in the app's config directory.

use std::path::PathBuf;

use sandbox_core::settings::Settings;
use tauri::{AppHandle, Manager};

pub fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("settings.json"))
}

/// Load settings, falling back to defaults plus the legacy env variables.
pub fn load(app: &AppHandle) -> Settings {
    let from_file = path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| Settings::from_json(&text))
        .unwrap_or_default();
    from_file.with_env_fallbacks(
        std::env::var("SANDBOX_WATCHER_RUNTIME").ok().as_deref(),
        std::env::var("SANDBOX_WATCHER_TERMINAL").ok().as_deref(),
    )
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = path(app).ok_or("no config directory available")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, settings.to_json()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}
