//! Self-update through GitHub Releases. The release workflow publishes a
//! signed `latest.json`; the public key in `tauri.conf.json` verifies every
//! download before it is installed.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};

/// Emitted with `UpdateInfo` when a newer release is found.
pub const UPDATE_AVAILABLE: &str = "update-available";
/// Emitted with `{ downloaded, total }` while downloading.
pub const UPDATE_PROGRESS: &str = "update-progress";

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// The update found by the last check, kept so "install" needs no second request.
#[derive(Default)]
pub struct PendingUpdate(pub Mutex<Option<Update>>);

fn info(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        date: update.date.map(|d| d.to_string()),
    }
}

/// Ask the release endpoint. `Ok(None)` when already up to date.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let found = update.as_ref().map(info);
    *app.state::<PendingUpdate>()
        .0
        .lock()
        .expect("pending update poisoned") = update;
    Ok(found)
}

/// Download, verify, install, then restart into the new version.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let update = app
        .state::<PendingUpdate>()
        .0
        .lock()
        .expect("pending update poisoned")
        .take()
        .ok_or("no update pending; check again")?;
    let mut downloaded: u64 = 0;
    let handle = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = handle.emit(UPDATE_PROGRESS, Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

/// Background loop: first check shortly after start, then every few hours.
/// Notifies once per newly seen version.
pub fn spawn_periodic(app: AppHandle, enabled: impl Fn() -> bool + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        let mut announced: Option<String> = None;
        loop {
            if enabled()
                && let Ok(Some(found)) = check(&app).await
            {
                let _ = app.emit(UPDATE_AVAILABLE, &found);
                if announced.as_deref() != Some(found.version.as_str()) {
                    let _ = app
                        .notification()
                        .builder()
                        .title(format!("Sandbox Watcher {} is available", found.version))
                        .body("Open the app to install the update.")
                        .show();
                    announced = Some(found.version.clone());
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}
