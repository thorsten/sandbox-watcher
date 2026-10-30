import { vi } from "vitest";

/** Replace the Tauri bridge: no backend exists under Vitest. Loaded via setupFiles. */
const bridge = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(async () => undefined),
  listen: vi.fn<() => Promise<() => void>>(async () => () => undefined),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));

export const invoke = bridge.invoke;
export const listen = bridge.listen;
