//! Tray / menu bar icon with a per-project summary.

use sandbox_core::host::Host;
use sandbox_core::overview::{ContainerOverview, Overview};
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

/// Running containers whose host answered. Greyed ones (`last_seen_at` set)
/// come from the cache of an unreachable host and do not count.
fn running_containers(overview: &Overview) -> impl Iterator<Item = &ContainerOverview> {
    overview
        .containers
        .iter()
        .filter(|c| c.container.state.is_running() && c.last_seen_at.is_none())
}

/// Label of a host: its display name, else the alias.
fn host_label(overview: &Overview, alias: &str) -> String {
    overview
        .hosts
        .iter()
        .find(|h| h.alias == alias)
        .map(|h| h.display_name.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(alias)
        .to_string()
}

/// One entry per project and host: menu label and the key of the first
/// container (click target). Remote projects carry their host label, so the
/// same slug on two hosts gives two entries.
fn quick_list(overview: &Overview) -> Vec<(String, String)> {
    // host, slug, containers, sessions, key of the first container
    let mut projects: Vec<(&Host, &str, usize, usize, &str)> = Vec::new();
    for c in running_containers(overview) {
        match projects
            .iter_mut()
            .find(|(host, slug, ..)| **host == c.host && *slug == c.container.project_slug)
        {
            Some(entry) => {
                entry.2 += 1;
                entry.3 += c.sessions.len();
            }
            None => projects.push((
                &c.host,
                &c.container.project_slug,
                1,
                c.sessions.len(),
                &c.key,
            )),
        }
    }
    projects
        .into_iter()
        .map(|(host, slug, containers, sessions, key)| {
            let name = match host {
                Host::Local => slug.to_string(),
                Host::Remote(alias) => format!("{}: {slug}", host_label(overview, alias)),
            };
            let label = match (containers, sessions) {
                (1, 0) => format!("{name}: idle"),
                (1, n) => format!("{name}: {n} session{}", if n == 1 { "" } else { "s" }),
                (c, n) => format!(
                    "{name}: {c} containers, {n} session{}",
                    if n == 1 { "" } else { "s" }
                ),
            };
            (label, key.to_string())
        })
        .collect()
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
        running = running_containers(overview).count();
        let entries = quick_list(overview);
        if entries.is_empty() {
            builder = builder.item(
                &MenuItemBuilder::with_id("tray-empty", "No running sandboxes")
                    .enabled(false)
                    .build(app)?,
            );
        }
        for (label, key) in &entries {
            builder = builder.item(
                &MenuItemBuilder::with_id(format!("{MENU_SELECT_PREFIX}{key}"), label)
                    .build(app)?,
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
            let running = running_containers(o).count();
            let sessions: usize = running_containers(o).map(|c| c.sessions.len()).sum();
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

#[cfg(test)]
mod tests {
    use super::*;
    use sandbox_core::mock::mock_container;
    use sandbox_core::model::{AgentKind, Session};
    use sandbox_core::model::{Readiness, RuntimeKind};
    use sandbox_core::overview::{DataSource, RuntimeStatus};
    use sandbox_core::remote::{HostState, HostStatus};

    fn entry(host: &str, slug: &str, sessions: usize) -> ContainerOverview {
        let container = mock_container(slug, 1, "running", "h").details.container;
        let (host, key) = if host == "local" {
            (Host::Local, container.id.clone())
        } else {
            (
                Host::Remote(host.into()),
                format!("{host}/{}", container.id),
            )
        };
        ContainerOverview {
            container,
            host,
            key,
            runtime: RuntimeKind::Docker,
            readiness: Readiness::Ready,
            sessions: (0..sessions)
                .map(|i| Session {
                    pid: 100 + i as u32,
                    command: "claude".into(),
                    agent: AgentKind::ClaudeCode,
                })
                .collect(),
            last_seen_at: None,
        }
    }

    fn overview(containers: Vec<ContainerOverview>) -> Overview {
        Overview {
            source: DataSource::Mock,
            runtime: RuntimeStatus::NotDetected,
            containers,
            tested_sandbox_version: "test".into(),
            hosts: vec![HostStatus {
                alias: "devbox".into(),
                display_name: "Dev VM".into(),
                state: HostState::Connected,
                runtimes: vec![RuntimeKind::Docker],
                probes: vec![],
            }],
        }
    }

    #[test]
    fn same_slug_on_two_hosts_gives_two_entries() {
        let o = overview(vec![entry("local", "web", 1), entry("devbox", "web", 2)]);
        let list = quick_list(&o);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "web: 1 session");
        assert_eq!(list[1].0, "Dev VM: web: 2 sessions");
        assert!(list[1].1.starts_with("devbox/"), "selects its own key");
        assert!(!list[0].1.contains('/'));
    }

    #[test]
    fn greyed_containers_are_not_running() {
        let mut greyed = entry("devbox", "web", 1);
        greyed.last_seen_at = Some(chrono::Utc::now());
        let o = overview(vec![entry("local", "api", 0), greyed]);
        assert_eq!(running_containers(&o).count(), 1);
        assert_eq!(quick_list(&o).len(), 1);
        assert_eq!(tooltip(Some(&o)), "Sandbox Watcher: 1 running, 0 sessions");
    }
}
