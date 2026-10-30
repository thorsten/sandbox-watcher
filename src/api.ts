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
  DetectedRuntime,
  NetworkReport,
  Overview,
  ResourceSample,
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
export function onSelectContainer(handler: (containerId: string) => void): Promise<UnlistenFn> {
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

export function getActivity(containerId: string): Promise<ContainerActivity> {
  return invoke<ContainerActivity>("get_activity", { containerId });
}

export function getContainerDetails(containerId: string): Promise<ContainerDetails | null> {
  return invoke<ContainerDetails | null>("get_container_details", { containerId });
}

export function getLogs(containerId: string, tail = 200): Promise<string> {
  return invoke<string>("get_logs", { containerId, tail });
}

export function revealMount(containerId: string, path: string): Promise<void> {
  return invoke<void>("reveal_mount", { containerId, path });
}

export function stopContainer(containerId: string): Promise<void> {
  return invoke<void>("stop_container", { containerId });
}

export function stopAll(): Promise<StopResult[]> {
  return invoke<StopResult[]>("stop_all");
}

export function getProjectPath(containerId: string): Promise<string | null> {
  return invoke<string | null>("get_project_path", { containerId });
}

export function openProject(containerId: string): Promise<void> {
  return invoke<void>("open_project", { containerId });
}

/** Resolves with the terminal program that was started. */
export function openTerminal(containerId: string): Promise<string> {
  return invoke<string>("open_terminal", { containerId });
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
  containerId: string,
  selections: { host: string; ports: number[] }[],
): Promise<AllowPreview> {
  return invoke<AllowPreview>("preview_allow", { containerId, selections });
}

export function applyAllow(
  containerId: string,
  target: ConfigTarget,
  entries: string[],
): Promise<AllowResult> {
  return invoke<AllowResult>("apply_allow", { containerId, target, entries });
}

export function detectRuntimes(): Promise<DetectedRuntime[]> {
  return invoke<DetectedRuntime[]>("detect_runtimes");
}

export function appVersion(): Promise<string> {
  return invoke<string>("app_version");
}
