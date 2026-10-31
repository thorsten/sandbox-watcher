//! Tray / menu bar icon with a per-project summary.

use sandbox_core::overview::Overview;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const TRAY_ID: &str = "main-tray";
pub const MENU_SHOW: &str = "tray-show";
pub const MENU_STOP_ALL: &str = "tray-stop-all";
pub const MENU_SETTINGS: &str = "tray-settings";
pub const MENU_QUIT: &str = "tray-quit";
/// Prefix of per-project items; the rest of the id is a container key.
pub const MENU_SELECT_PREFIX: &str = "tray-select:";
/// Emitted with a container key when the user picks a project in the tray.
pub const SELECT_CONTAINER_EVENT: &str = "select-container";
/// Emitted to the webview when the user picks "Settings" in the tray.
pub const OPEN_SETTINGS_EVENT: &str = "open-settings";

/// Bring the main window to the front, unhiding it if it lives in the tray.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    overview: Option<&Overview>,
) -> tauri::Result<Menu<R>> {
    let mut builder = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id(MENU_SHOW, "Show Sandbox Watcher").build(app)?)
        .separator();

    let mut running = 0usize;
    if let Some(overview) = overview {
        // slug, containers, sessions, id of the first container (click target)
        let mut projects: Vec<(String, usize, usize, String)> = Vec::new();
        for c in &overview.containers {
            if !c.container.state.is_running() {
                continue;
            }
            running += 1;
            match projects
                .iter_mut()
                .find(|(slug, ..)| *slug == c.container.project_slug)
            {
                Some(entry) => {
                    entry.1 += 1;
                    entry.2 += c.sessions.len();
                }
                None => projects.push((
                    c.container.project_slug.clone(),
                    1,
                    c.sessions.len(),
                    c.key.clone(),
                )),
            }
        }
        if projects.is_empty() {
            builder = builder.item(
                &MenuItemBuilder::with_id("tray-empty", "No running sandboxes")
                    .enabled(false)
                    .build(app)?,
            );
        }
        for (slug, containers, sessions, id) in &projects {
            let label = match (containers, sessions) {
                (1, 0) => format!("{slug}: idle"),
                (1, n) => format!("{slug}: {n} session{}", if *n == 1 { "" } else { "s" }),
                (c, n) => format!(
                    "{slug}: {c} containers, {n} session{}",
                    if *n == 1 { "" } else { "s" }
                ),
            };
            builder = builder.item(
                &MenuItemBuilder::with_id(format!("{MENU_SELECT_PREFIX}{id}"), label).build(app)?,
            );
        }
        builder = builder.separator();
    }

    builder
        .item(
            &MenuItemBuilder::with_id(MENU_STOP_ALL, "Stop all sandboxes")
                .enabled(running > 0)
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id(MENU_SETTINGS, "Settings…").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(MENU_QUIT, "Quit Sandbox Watcher").build(app)?)
        .build()
}

fn tooltip(overview: Option<&Overview>) -> String {
    match overview {
        None => "Sandbox Watcher".to_string(),
        Some(o) => {
            let running = o
                .containers
                .iter()
                .filter(|c| c.container.state.is_running())
                .count();
            let sessions: usize = o.containers.iter().map(|c| c.sessions.len()).sum();
            format!(
                "Sandbox Watcher: {running} running, {sessions} session{}",
                if sessions == 1 { "" } else { "s" }
            )
        }
    }
}

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let menu = build_menu(app, None)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(tooltip(None))
        .show_menu_on_left_click(cfg!(not(target_os = "macos")))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { .. } = event
                && cfg!(target_os = "macos")
            {
                // Left click on macOS shows the window; the menu is on right click.
                show_main_window(tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    {
        let bytes = include_bytes!("../icons/tray-template@2x.png");
        if let Ok(icon) = tauri::image::Image::from_bytes(bytes) {
            builder = builder.icon(icon).icon_as_template(true);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(icon) = app.default_window_icon().cloned() {
            builder = builder.icon(icon);
        }
    }
    builder.build(app)
}

/// Refresh menu and tooltip after an overview change.
pub fn update<R: Runtime>(app: &AppHandle<R>, overview: &Overview) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app, Some(overview)) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip(Some(overview))));
}

/// Handle tray menu clicks that the shell owns. Returns false for ids it
/// does not know so the caller can handle them.
pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    match id {
        MENU_SHOW => {
            show_main_window(app);
            true
        }
        MENU_SETTINGS => {
            show_main_window(app);
            let _ = app.emit(OPEN_SETTINGS_EVENT, ());
            true
        }
        MENU_QUIT => {
            app.exit(0);
            true
        }
        id if id.starts_with(MENU_SELECT_PREFIX) => {
            show_main_window(app);
            let _ = app.emit(SELECT_CONTAINER_EVENT, &id[MENU_SELECT_PREFIX.len()..]);
            true
        }
        _ => false,
    }
}
