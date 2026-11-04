//! Persist `Settings` as JSON in the app's config directory.

use std::path::PathBuf;

use sandbox_core::CommandError;
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

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), CommandError> {
    let path = path(app).ok_or(CommandError::NoConfigDirectory)?;
    let failed = |at: &std::path::Path| {
        let at = at.display().to_string();
        move |e: std::io::Error| CommandError::SettingsWriteFailed {
            path: at,
            detail: e.to_string(),
        }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed(dir))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, settings.to_json()).map_err(failed(&tmp))?;
    std::fs::rename(&tmp, &path).map_err(failed(&path))
}
