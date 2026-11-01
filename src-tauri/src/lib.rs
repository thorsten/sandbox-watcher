//! Tauri shell around `sandbox-core`. A background watcher refreshes the
//! overview, drives the tray and notifications, and pushes updates to the
//! webview; commands read its cache or perform actions.

mod app_menu;
#[cfg(test)]
mod contracts;
mod settings_store;
mod tray;
mod updates;
mod watcher;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sandbox_core::actions::{
    MacTerminal, STOP_TIMEOUT, StopReport, TerminalLaunch, custom_terminal_launch,
    terminal_launches,
};
use sandbox_core::activity::ContainerActivity;
use sandbox_core::adapter::LogOptions;
use sandbox_core::allowlist::{
    AllowResult, ConfigTarget, entries_for, existing_entries, project_config_available,
    project_config_path,
};
use sandbox_core::detect::{DetectedRuntime, Environment, detect};
use sandbox_core::diagnostics::DiagnosticsReport;
use sandbox_core::endpoints::SshContext;
use sandbox_core::history::{HistoryQuery, HistoryView};
use sandbox_core::host::Host;
use sandbox_core::model::AgentKind;
use sandbox_core::model::{ContainerDetails, ResourceSample};
use sandbox_core::network::NetworkReport;
use sandbox_core::overview::Overview;
use sandbox_core::remote::HostStatus;
use sandbox_core::runner::CommandRunner;
use sandbox_core::settings::{Settings, TerminalChoice};
use sandbox_core::turns::TurnReport;
use tauri::menu::MenuEvent;
use tauri::{AppHandle, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartExt};
use tauri_plugin_opener::OpenerExt;
use watcher::{Mode, Watcher};

type App = Arc<Watcher>;

/// Host-side actions need the container's files on this machine.
pub fn require_local(key: &str) -> Result<(), String> {
    if sandbox_core::host::parse_container_key(key).0.is_local() {
        Ok(())
    } else {
        Err("not available for remote hosts".into())
    }
}

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

/// What is happening inside a running container right now: processes under
/// each session, the agent's transcript where known, and git changes.
#[tauri::command]
async fn get_activity(
    state: State<'_, App>,
    container_key: String,
) -> Result<ContainerActivity, String> {
    let (adapter, container_id) = state.adapter_for(&container_key)?;
    let sessions = state
        .overview()
        .and_then(|o| o.containers.into_iter().find(|c| c.key == container_key))
        .map(|c| c.sessions)
        .unwrap_or_default();
    let prices = state.settings().prices;
    Ok(sandbox_core::activity::collect(
        &adapter,
        &container_id,
        &container_key,
        &sessions,
        Utc::now(),
        &prices,
        state.transcripts(),
    )
    .await)
}

/// Turn-by-turn detail of one agent's transcript: the one attributed to
/// `pid`, else the agent's newest. Reads the whole transcript, so the UI asks
/// for it on demand only. The transcript path is found inside the container,
/// never taken from the webview.
#[tauri::command]
async fn get_turns(
    state: State<'_, App>,
    container_key: String,
    agent: AgentKind,
    pid: Option<u32>,
) -> Result<Option<TurnReport>, String> {
    let (adapter, container_id) = state.adapter_for(&container_key)?;
    let sessions = state
        .overview()
        .and_then(|o| o.containers.into_iter().find(|c| c.key == container_key))
        .map(|c| c.sessions)
        .unwrap_or_default();
    let prices = state.settings().prices;
    sandbox_core::activity::read_turns(
        &adapter,
        &container_id,
        &container_key,
        agent,
        pid,
        &sessions,
        state.transcripts(),
        &prices,
    )
    .await
    .map_err(|e| e.to_string())
}

/// Mounts, ports, env names and user of one container, mounts classified
/// and host paths spelled the way this OS shows them.
#[tauri::command]
async fn get_container_details(
    state: State<'_, App>,
    container_key: String,
) -> Result<Option<ContainerDetails>, String> {
    let (adapter, container_id) = state.adapter_for(&container_key)?;
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
    container_key: String,
    tail: Option<usize>,
) -> Result<String, String> {
    let (adapter, container_id) = state.adapter_for(&container_key)?;
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
    container_key: String,
    path: String,
) -> Result<(), String> {
    require_local(&container_key)?;
    let (adapter, container_id) = state.adapter_for(&container_key)?;
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
async fn stop_container(state: State<'_, App>, container_key: String) -> Result<(), String> {
    let (adapter, container_id) = state.adapter_for(&container_key)?;
    adapter
        .stop(&container_id, Some(STOP_TIMEOUT))
        .await
        .map_err(|e| e.to_string())?;
    state.poke();
    Ok(())
}

/// Stop every running sandbox container across runtimes. Hosts whose
/// containers could not be listed are reported as failures, so the UI never
/// shows a partial stop as complete.
#[tauri::command]
async fn stop_all(state: State<'_, App>) -> Result<StopReport, String> {
    let report = sandbox_core::actions::stop_all(&state.adapters(), STOP_TIMEOUT).await;
    state.poke();
    Ok(report)
}

/// Host path of the container's project directory, if it has a bind-mounted one.
#[tauri::command]
async fn get_project_path(
    state: State<'_, App>,
    container_key: String,
) -> Result<Option<String>, String> {
    let (adapter, container_id) = state.adapter_for(&container_key)?;
    let local = sandbox_core::host::parse_container_key(&container_key)
        .0
        .is_local();
    sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows) && local)
        .await
        .map_err(|e| e.to_string())
}

/// Open the project directory in the file manager.
#[tauri::command]
async fn open_project(
    app: AppHandle,
    state: State<'_, App>,
    container_key: String,
) -> Result<(), String> {
    require_local(&container_key)?;
    let (adapter, container_id) = state.adapter_for(&container_key)?;
    let dir = sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "this container has no bind-mounted project directory".to_string())?;
    app.opener()
        .open_path(&dir, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Try launch candidates in order until one starts (or, for `wait`
/// launches, exits successfully). Returns a label for the one that worked.
async fn run_launches(launches: &[TerminalLaunch]) -> Result<String, String> {
    let mut errors = Vec::new();
    for launch in launches {
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
    Err(errors.join("; "))
}

/// Open the project, or a file inside it, in the editor chosen in Settings.
/// The path is checked to lie inside the container's project directory.
#[tauri::command]
async fn open_in_editor(
    app: AppHandle,
    state: State<'_, App>,
    container_key: String,
    path: Option<String>,
) -> Result<String, String> {
    require_local(&container_key)?;
    let (adapter, container_id) = state.adapter_for(&container_key)?;
    let root = sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "this container has no bind-mounted project directory".to_string())?;
    // Transcripts carry container paths; on Windows hosts translate them first.
    let requested = path.map(|p| sandbox_core::paths::display_host_path(&p, cfg!(windows)));
    let target =
        sandbox_core::editor::validate_target(std::path::Path::new(&root), requested.as_deref())
            .ok_or_else(|| "path is outside this container's project".to_string())?;
    let target = target.to_string_lossy().into_owned();
    let choice = state.settings().editor;
    let launches = sandbox_core::editor::editor_launches(std::env::consts::OS, &choice, &target);
    if launches.is_empty() {
        app.opener()
            .open_path(&target, None::<&str>)
            .map_err(|e| e.to_string())?;
        return Ok("default app".into());
    }
    run_launches(&launches)
        .await
        .map_err(|e| format!("no editor could be started ({e})"))
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
async fn open_terminal(state: State<'_, App>, container_key: String) -> Result<String, String> {
    require_local(&container_key)?;
    let (adapter, container_id) = state.adapter_for(&container_key)?;
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

    run_launches(&launches)
        .await
        .map_err(|e| format!("no terminal could be started ({e})"))
}

/// Session history for a time range, optionally narrowed to a project or agent.
#[tauri::command]
fn get_history(
    state: State<'_, App>,
    since_hours: Option<u64>,
    project: Option<String>,
    agent: Option<AgentKind>,
) -> HistoryView {
    let since = match since_hours {
        Some(h) => Utc::now() - chrono::Duration::hours(h as i64),
        None => chrono::DateTime::<Utc>::MIN_UTC,
    };
    state.history_view(&HistoryQuery {
        since,
        project,
        agent,
    })
}

#[tauri::command]
fn clear_history(state: State<'_, App>) {
    state.clear_history();
}

/// A blocked destination the user wants to allow.
#[derive(serde::Deserialize)]
struct AllowSelection {
    host: String,
    ports: Vec<u16>,
}

#[derive(serde::Serialize)]
struct AllowPreview {
    entries: Vec<String>,
    global_path: Option<String>,
    project_path: Option<String>,
    already_in_global: Vec<String>,
    already_in_project: Vec<String>,
    /// SHA-256 of the previewed files (remote only); handed back to apply.
    global_hash: Option<String>,
    project_hash: Option<String>,
    /// Why a target cannot be written (symlink rules, missing directory).
    global_unavailable: Option<String>,
    project_unavailable: Option<String>,
}

fn global_config_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    sandbox_core::allowlist::global_config_path(
        std::env::consts::OS,
        std::env::var("SANDBOX_CONFIG_DIR").ok().as_deref(),
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
        home.as_deref(),
    )
}

async fn project_root_of(
    state: &State<'_, App>,
    container_key: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    let (adapter, container_id) = state.adapter_for(container_key)?;
    Ok(
        sandbox_core::actions::project_path(&adapter, &container_id, cfg!(windows))
            .await
            .map_err(|e| e.to_string())?
            .map(std::path::PathBuf::from),
    )
}

const REMOTE_ALLOW_TIMEOUT: Duration = Duration::from_secs(15);

fn merged_entries(selections: &[AllowSelection]) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for sel in selections {
        for e in entries_for(&sel.host, &sel.ports.iter().copied().collect()) {
            if !entries.contains(&e) {
                entries.push(e);
            }
        }
    }
    entries
}

/// The entries already in the config. The preview only marks duplicates;
/// apply() aborts on a broken file.
fn present_in(entries: &[String], content: &str) -> Vec<String> {
    let existing = existing_entries(content).unwrap_or_default();
    entries
        .iter()
        .filter(|e| existing.contains(e))
        .cloned()
        .collect()
}

/// What allowing the selected destinations would write, and where.
#[tauri::command]
async fn preview_allow(
    state: State<'_, App>,
    container_key: String,
    selections: Vec<AllowSelection>,
) -> Result<AllowPreview, String> {
    let entries = merged_entries(&selections);
    let (host, _) = sandbox_core::host::parse_container_key(&container_key);
    let project_root = project_root_of(&state, &container_key).await?;
    match host {
        Host::Local => {
            let global = global_config_path();
            let project = project_root
                .filter(|r| project_config_available(r))
                .map(|r| project_config_path(&r));
            let present = |path: &Option<std::path::PathBuf>| -> Vec<String> {
                let Some(path) = path else { return vec![] };
                std::fs::read_to_string(path)
                    .map(|c| present_in(&entries, &c))
                    .unwrap_or_default()
            };
            Ok(AllowPreview {
                already_in_global: present(&global),
                already_in_project: present(&project),
                entries,
                global_path: global.map(|p| p.display().to_string()),
                project_path: project.map(|p| p.display().to_string()),
                global_hash: None,
                project_hash: None,
                global_unavailable: None,
                project_unavailable: None,
            })
        }
        Host::Remote(alias) => {
            use sandbox_core::allowlist_remote::preview_remote;
            let root = project_root
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned());
            let session = state.session_for(&alias).await?;
            let global = preview_remote(
                session.as_ref(),
                ConfigTarget::Global,
                None,
                REMOTE_ALLOW_TIMEOUT,
            )
            .await?;
            let project = preview_remote(
                session.as_ref(),
                ConfigTarget::Project,
                root.as_deref(),
                REMOTE_ALLOW_TIMEOUT,
            )
            .await?;
            Ok(AllowPreview {
                already_in_global: present_in(&entries, &global.content),
                already_in_project: present_in(&entries, &project.content),
                entries,
                global_path: global.path,
                project_path: project.path,
                global_hash: global.hash,
                project_hash: project.hash,
                global_unavailable: global.unavailable,
                project_unavailable: project.unavailable,
            })
        }
    }
}

/// Append the entries to the chosen sandbox config. Locally only the two
/// paths sandbox itself would write are accepted; on a remote host the write
/// is a compare-and-swap against the hash the preview returned.
#[tauri::command]
async fn apply_allow(
    state: State<'_, App>,
    container_key: String,
    target: ConfigTarget,
    entries: Vec<String>,
    expected_hash: Option<String>,
) -> Result<AllowResult, String> {
    let (host, _) = sandbox_core::host::parse_container_key(&container_key);
    let Host::Remote(alias) = host else {
        let path = match target {
            ConfigTarget::Global => global_config_path().ok_or("no global config path")?,
            ConfigTarget::Project => {
                let root = project_root_of(&state, &container_key)
                    .await?
                    .ok_or("no project directory")?;
                if !project_config_available(&root) {
                    return Err("this project has no .sandbox directory".into());
                }
                project_config_path(&root)
            }
        };
        return sandbox_core::allowlist::apply(&path, target, &entries)
            .map_err(|e| format!("{}: {e}", path.display()));
    };
    use sandbox_core::allowlist_remote::{RemoteApplyError, apply_remote};
    let expected = expected_hash.ok_or("preview is required before writing to a remote host")?;
    let root = project_root_of(&state, &container_key)
        .await?
        .map(|p| p.to_string_lossy().into_owned());
    let session = state.session_for(&alias).await?;
    apply_remote(
        session.as_ref(),
        target,
        root.as_deref(),
        &expected,
        &entries,
        REMOTE_ALLOW_TIMEOUT,
    )
    .await
    .map_err(|e| match e {
        RemoteApplyError::Changed(msg) => format!("changed: {msg}"),
        RemoteApplyError::Refused { path, reason } => format!("{path}: {reason}"),
        RemoteApplyError::Other(e) => e,
    })
}

/// Check GitHub Releases for a newer version now.
#[tauri::command]
async fn check_for_update(app: AppHandle) -> Result<Option<updates::UpdateInfo>, String> {
    updates::check(&app).await
}

/// Install the update found by the last check and restart.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updates::install(&app).await
}

/// Open a throwaway connection to a host, report what it found, and retry
/// unreachable hosts right away.
#[tauri::command]
async fn test_remote_host(state: State<'_, App>, alias: String) -> Result<HostStatus, String> {
    sandbox_core::host::validate_alias(&alias)?;
    let status = sandbox_core::live::test_remote_host(&alias).await;
    state.reset_backoff();
    Ok(status)
}

/// Probe every runtime candidate and every remote host now. On demand only.
#[tauri::command]
async fn run_diagnostics(state: State<'_, App>) -> Result<DiagnosticsReport, String> {
    let version = env!("CARGO_PKG_VERSION");
    if matches!(state.mode, Mode::Mock(_)) {
        return Ok(sandbox_core::mock::demo_diagnostics(version));
    }
    let settings = state.settings();
    // Name where the pin came from, so the report can say why runtimes were skipped.
    // settings_store folds SANDBOX_WATCHER_RUNTIME into the override.
    let from_env = std::env::var("SANDBOX_WATCHER_RUNTIME").ok().and_then(|v| {
        Settings::default()
            .with_env_fallbacks(Some(&v), None)
            .runtime_override
    });
    let preferred = if let Some(kind) = settings.runtime_override {
        let label = if from_env == Some(kind) {
            "SANDBOX_WATCHER_RUNTIME"
        } else {
            "Settings"
        };
        Some((kind, label.to_string()))
    } else {
        sandbox_core::live::preferred_runtime().map(|kind| (kind, "sandbox config".to_string()))
    };
    Ok(sandbox_core::diagnostics::run(version, preferred, &settings.remote_hosts).await)
}

/// The report as plain text, for copying into an issue.
#[tauri::command]
fn diagnostics_text(report: DiagnosticsReport) -> String {
    sandbox_core::diagnostics::render_text(&report)
}

/// The `ssh://` Docker contexts that can be imported as remote hosts.
/// Reads only; nothing is connected or saved.
#[tauri::command]
async fn list_docker_ssh_contexts(state: State<'_, App>) -> Result<Vec<SshContext>, String> {
    if matches!(state.mode, Mode::Mock(_)) {
        return Ok(sandbox_core::mock::demo_ssh_contexts());
    }
    if cfg!(not(unix)) {
        return Ok(vec![]);
    }
    Ok(sandbox_core::endpoints::docker_ssh_contexts(&CommandRunner::Local).await)
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::PendingUpdate::default())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = settings_store::load(&handle);
            let history_path = handle
                .path()
                .app_data_dir()
                .ok()
                .map(|d| d.join("history.json"));
            let watcher = Watcher::new(handle.clone(), settings.clone(), history_path);
            app.manage(watcher.clone());

            // Runtime events ask the watcher for a refresh.
            if let Mode::Live(live) = &watcher.mode {
                let poke = watcher.clone();
                live.set_listener(Arc::new(move |_event| poke.poke()));
            }

            app.set_menu(app_menu::build(&handle)?)?;
            tray::create(&handle)?;
            app.on_menu_event(on_menu_event);
            apply_autostart(&handle, settings.launch_at_login);

            if settings.start_minimized
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }

            watcher.start();
            if !matches!(watcher.mode, Mode::Mock(_)) {
                let w = watcher.clone();
                updates::spawn_periodic(handle.clone(), move || w.settings().check_for_updates);
            }
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
            run_diagnostics,
            diagnostics_text,
            get_overview,
            refresh_now,
            get_network_overview,
            get_stats,
            get_settings,
            save_settings,
            get_activity,
            get_turns,
            get_history,
            clear_history,
            preview_allow,
            apply_allow,
            get_container_details,
            get_logs,
            reveal_mount,
            stop_container,
            stop_all,
            get_project_path,
            open_project,
            open_terminal,
            open_in_editor,
            check_for_update,
            install_update,
            detect_runtimes,
            test_remote_host,
            list_docker_ssh_contexts,
            app_version
        ])
        .build(tauri::generate_context!())
        .expect("error while building Sandbox Watcher")
        .run(|app, event| {
            if let RunEvent::Exit = event
                && let Some(watcher) = app.try_state::<App>()
            {
                // Close ssh master connections and their private socket directory.
                let watcher = watcher.inner().clone();
                tauri::async_runtime::block_on(watcher.shutdown());
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_side_actions_refuse_remote_keys() {
        assert!(super::require_local("abc").is_ok());
        assert_eq!(
            super::require_local("devbox/abc").unwrap_err(),
            "not available for remote hosts"
        );
    }
}
