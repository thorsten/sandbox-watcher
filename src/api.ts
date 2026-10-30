import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DetectedRuntime, Overview, RuntimeEvent } from "./types";

export function getOverview(): Promise<Overview> {
  return invoke<Overview>("get_overview");
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
