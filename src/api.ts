import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ContainerDetails,
  DetectedRuntime,
  NetworkReport,
  Overview,
  ResourceSample,
  RuntimeEvent,
} from "./types";

export function getOverview(): Promise<Overview> {
  return invoke<Overview>("get_overview");
}

export function getNetworkOverview(): Promise<NetworkReport[]> {
  return invoke<NetworkReport[]>("get_network_overview");
}

export function getStats(): Promise<Record<string, ResourceSample[]>> {
  return invoke<Record<string, ResourceSample[]>>("get_stats");
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

export function detectRuntimes(): Promise<DetectedRuntime[]> {
  return invoke<DetectedRuntime[]>("detect_runtimes");
}

export function appVersion(): Promise<string> {
  return invoke<string>("app_version");
}

/** Subscribe to runtime lifecycle events forwarded by the Rust side. */
export function onRuntimeEvent(handler: (event: RuntimeEvent) => void): Promise<UnlistenFn> {
  return listen<RuntimeEvent>("runtime-event", (e) => handler(e.payload));
}
