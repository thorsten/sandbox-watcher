import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentKind,
  AllowPreview,
  AllowResult,
  ConfigTarget,
  ContainerActivity,
  HistoryView,
  ContainerDetails,
  Settings,
  StopResult,
  UpdateInfo,
  DetectedRuntime,
  HostStatus,
  NetworkReport,
  Overview,
  ResourceSample,
  SshContext,
  TurnReport,
} from "./types";

export function getOverview(): Promise<Overview> {
  return invoke<Overview>("get_overview");
}

export function refreshNow(): Promise<Overview> {
  return invoke<Overview>("refresh_now");
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("save_settings", { settings });
}

/** The Rust watcher pushes a fresh overview after every refresh. */
export function onOverviewUpdated(handler: (overview: Overview) => void): Promise<UnlistenFn> {
  return listen<Overview>("overview-updated", (e) => handler(e.payload));
}

export function onNetworkUpdated(handler: (reports: NetworkReport[]) => void): Promise<UnlistenFn> {
  return listen<NetworkReport[]>("network-updated", (e) => handler(e.payload));
}

export function onSettingsUpdated(handler: (settings: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>("settings-updated", (e) => handler(e.payload));
}

/** Tray menu picked a project: select this container in the window. */
export function onSelectContainer(handler: (containerKey: string) => void): Promise<UnlistenFn> {
  return listen<string>("select-container", (e) => handler(e.payload));
}

/** Tray menu asked for the settings panel. */
export function onOpenSettings(handler: () => void): Promise<UnlistenFn> {
  return listen<null>("open-settings", () => handler());
}

export function getNetworkOverview(): Promise<NetworkReport[]> {
  return invoke<NetworkReport[]>("get_network_overview");
}

export function getStats(): Promise<Record<string, ResourceSample[]>> {
  return invoke<Record<string, ResourceSample[]>>("get_stats");
}

export function getActivity(containerKey: string): Promise<ContainerActivity> {
  return invoke<ContainerActivity>("get_activity", { containerKey });
}

/** Turn-by-turn detail of an agent's transcript; reads it in full, so on demand only. */
export function getTurns(containerKey: string, agent: AgentKind): Promise<TurnReport | null> {
  return invoke<TurnReport | null>("get_turns", { containerKey, agent });
}

export function getContainerDetails(containerKey: string): Promise<ContainerDetails | null> {
  return invoke<ContainerDetails | null>("get_container_details", { containerKey });
}

export function getLogs(containerKey: string, tail = 200): Promise<string> {
  return invoke<string>("get_logs", { containerKey, tail });
}

export function revealMount(containerKey: string, path: string): Promise<void> {
  return invoke<void>("reveal_mount", { containerKey, path });
}

export function stopContainer(containerKey: string): Promise<void> {
  return invoke<void>("stop_container", { containerKey });
}

export function stopAll(): Promise<StopResult[]> {
  return invoke<StopResult[]>("stop_all");
}

export function getProjectPath(containerKey: string): Promise<string | null> {
  return invoke<string | null>("get_project_path", { containerKey });
}

export function openProject(containerKey: string): Promise<void> {
  return invoke<void>("open_project", { containerKey });
}

/** Resolves with the terminal program that was started. */
export function openTerminal(containerKey: string): Promise<string> {
  return invoke<string>("open_terminal", { containerKey });
}

export function getHistory(
  sinceHours: number | null,
  project: string | null,
  agent: AgentKind | null,
): Promise<HistoryView> {
  return invoke<HistoryView>("get_history", { sinceHours, project, agent });
}

export function clearHistory(): Promise<void> {
  return invoke<void>("clear_history");
}

export function previewAllow(
  containerKey: string,
  selections: { host: string; ports: number[] }[],
): Promise<AllowPreview> {
  return invoke<AllowPreview>("preview_allow", { containerKey, selections });
}

export function applyAllow(
  containerKey: string,
  target: ConfigTarget,
  entries: string[],
): Promise<AllowResult> {
  return invoke<AllowResult>("apply_allow", { containerKey, target, entries });
}

export function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update");
}

/** Downloads, installs and restarts; resolves only if something failed before restart. */
export function installUpdate(): Promise<void> {
  return invoke<void>("install_update");
}

export function onUpdateAvailable(handler: (info: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-available", (e) => handler(e.payload));
}

export function onUpdateProgress(
  handler: (p: { downloaded: number; total: number | null }) => void,
): Promise<UnlistenFn> {
  return listen<{ downloaded: number; total: number | null }>("update-progress", (e) =>
    handler(e.payload),
  );
}

/** Open the project (no path) or a file inside it in the configured editor. */
export function openInEditor(containerKey: string, path?: string): Promise<string> {
  return invoke<string>("open_in_editor", { containerKey, path: path ?? null });
}

export function detectRuntimes(): Promise<DetectedRuntime[]> {
  return invoke<DetectedRuntime[]>("detect_runtimes");
}

/** Open a throwaway connection to a remote host and report what it found. */
export function testRemoteHost(alias: string): Promise<HostStatus> {
  return invoke<HostStatus>("test_remote_host", { alias });
}

/** The `ssh://` Docker contexts on this machine. Never connects to them. */
export function listDockerSshContexts(): Promise<SshContext[]> {
  return invoke<SshContext[]>("list_docker_ssh_contexts");
}

export function appVersion(): Promise<string> {
  return invoke<string>("app_version");
}
