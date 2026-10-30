//! Tauri shell around `sandbox-core`. A background watcher refreshes the
//! overview, drives the tray and notifications, and pushes updates to the
//! webview; commands read its cache or perform actions.

mod settings_store;
mod tray;
mod watcher;

use std::collections::HashMap;
use std::sync::Arc;

use sandbox_core::actions::{
    MacTerminal, STOP_TIMEOUT, StopResult, custom_terminal_launch, terminal_launches,
};
use sandbox_core::adapter::LogOptions;
use sandbox_core::detect::{DetectedRuntime, Environment, detect};
use sandbox_core::model::{ContainerDetails, ResourceSample};
use sandbox_core::network::NetworkReport;
use sandbox_core::overview::Overview;
use sandbox_core::settings::{Settings, TerminalChoice};
use tauri::menu::MenuEvent;
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartExt};
use tauri_plugin_opener::OpenerExt;
use watcher::{Mode, Watcher};

type App = Arc<Watcher>;

#[tauri::command]
async fn get_overview(state: State<'_, App>) -> Result<Overview, String> {
    if let Some(overview) = state.overview() {
        return Ok(overview);
    }
    state.refresh_and_wait().await;
    state
        .overview()
        .ok_or_else(|| "overview not available yet".to_string())
}

/// Force a refresh and return the result.
#[tauri::command]
async fn refresh_now(state: State<'_, App>) -> Result<Overview, String> {
    state.refresh_and_wait().await;
    state
        .overview()
        .ok_or_else(|| "overview not available yet".to_string())
}

#[tauri::command]
fn get_network_overview(state: State<'_, App>) -> Vec<NetworkReport> {
    state.network()
}

/// Resource history per running container, oldest sample first.
#[tauri::command]
fn get_stats(state: State<'_, App>) -> HashMap<String, Vec<ResourceSample>> {
    state.stats.snapshot()
}

#[tauri::command]
fn get_settings(state: State<'_, App>) -> Settings {
    state.settings()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, App>,
    settings: Settings,
) -> Result<Settings, String> {
    let settings = settings.normalised();
    settings_store::save(&app, &settings)?;
    apply_autostart(&app, settings.launch_at_login);
    state.apply_settings(settings.clone());
    Ok(settings)
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    if enabled && !current {
        let _ = manager.enable();
    } else if !enabled && current {
        let _ = manager.disable();
    }
}

/// Mounts, ports, env names and user of one container, mounts classified
/// and host paths spelled the way this OS shows them.
#[tauri::command]
async fn get_container_details(
    state: State<'_, App>,
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
    state: State<'_, App>,
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
    state: State<'_, App>,
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
async fn stop_container(state: State<'_, App>, container_id: String) -> Result<(), String> {
    let adapter = state.adapter_for(&container_id)?;
    adapter
        .stop(&container_id, Some(STOP_TIMEOUT))
        .await
        .map_err(|e| e.to_string())?;
    state.poke();
    Ok(())
}

/// Stop every running sandbox container across runtimes.
#[tauri::command]
async fn stop_all(state: State<'_, App>) -> Result<Vec<StopResult>, String> {
    let results = sandbox_core::actions::stop_all(&state.adapters(), STOP_TIMEOUT).await;
    state.poke();
    Ok(results)
}

/// Host path of the container's project directory, if it has a bind-mounted one.
#[tauri::command]
async fn get_project_path(
    state: State<'_, App>,
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
    state: State<'_, App>,
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

/// Terminal apps to try on macOS, from settings, then what is installed.
fn mac_terminals(choice: &TerminalChoice) -> Vec<MacTerminal> {
    match choice {
        TerminalChoice::Iterm => return vec![MacTerminal::ITerm],
        TerminalChoice::TerminalApp => return vec![MacTerminal::TerminalApp],
        TerminalChoice::Auto | TerminalChoice::Custom(_) => {}
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let iterm_installed = [
        "/Applications/iTerm.app",
        &format!("{home}/Applications/iTerm.app"),
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists());
    if iterm_installed {
        vec![MacTerminal::ITerm]
    } else {
        vec![]
    }
}

/// Open a terminal in the project directory. Tries the configured or the
/// platform's usual terminals in order until one succeeds.
#[tauri::command]
async fn open_terminal(state: State<'_, App>, container_id: String) -> Result<String, String> {
    let adapter = state.adapter_for(&container_id)?;
    let dir = sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "this container has no bind-mounted project directory".to_string())?;
    let settings = state.settings();
    let os = std::env::consts::OS;
    let mut launches = Vec::new();
    if let TerminalChoice::Custom(template) = &settings.terminal {
        launches.push(custom_terminal_launch(os, template, &dir));
    }
    let terminal_env = std::env::var("TERMINAL").ok();
    launches.extend(terminal_launches(
        os,
        &dir,
        terminal_env.as_deref(),
        &mac_terminals(&settings.terminal),
    ));

    let mut errors = Vec::new();
    for launch in &launches {
        let mut command = tokio::process::Command::new(&launch.program);
        command.args(&launch.args);
        if let Some(cwd) = &launch.cwd {
            command.current_dir(cwd);
        }
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::piped());
        if !launch.wait {
            match command.spawn() {
                Ok(_) => return Ok(launch.program.clone()),
                Err(e) => errors.push(format!("{}: {e}", launch.program)),
            }
            continue;
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                errors.push(format!("{}: {e}", launch.program));
                continue;
            }
        };
        match tokio::time::timeout(std::time::Duration::from_secs(20), child.wait_with_output())
            .await
        {
            Ok(Ok(output)) if output.status.success() => {
                let label = launch
                    .args
                    .iter()
                    .find_map(|a| {
                        a.strip_prefix("tell application \"")
                            .map(|r| r.trim_end_matches('"').to_string())
                    })
                    .unwrap_or_else(|| launch.program.clone());
                return Ok(label);
            }
            Ok(Ok(output)) => errors.push(format!(
                "{}: {}",
                launch.program,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Ok(Err(e)) => errors.push(format!("{}: {e}", launch.program)),
            Err(_) => errors.push(format!("{}: timed out", launch.program)),
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

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    if tray::handle_menu_event(app, id) {
        return;
    }
    if id == tray::MENU_STOP_ALL {
        let watcher = app.state::<App>().inner().clone();
        tauri::async_runtime::spawn(async move {
            let _ = sandbox_core::actions::stop_all(&watcher.adapters(), STOP_TIMEOUT).await;
            watcher.poke();
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = settings_store::load(&handle);
            let watcher = Watcher::new(handle.clone(), settings.clone());
            app.manage(watcher.clone());

            // Runtime events ask the watcher for a refresh.
            if let Mode::Live(live) = &watcher.mode {
                let poke = watcher.clone();
                live.set_listener(Arc::new(move |_event| poke.poke()));
            }

            tray::create(&handle)?;
            app.on_menu_event(on_menu_event);
            apply_autostart(&handle, settings.launch_at_login);

            if settings.start_minimized
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }

            watcher.start();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let watcher = window.app_handle().state::<App>();
                if watcher.settings().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            refresh_now,
            get_network_overview,
            get_stats,
            get_settings,
            save_settings,
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
