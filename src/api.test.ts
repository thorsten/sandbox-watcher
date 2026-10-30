import { beforeEach, describe, expect, it } from "vitest";
import * as api from "./api";
import { invoke } from "./test/tauriMock";

describe("api command names and arguments", () => {
  beforeEach(() => invoke.mockClear());

  const cases: [string, () => Promise<unknown>, string, unknown][] = [
    ["overview", () => api.getOverview(), "get_overview", undefined],
    ["refresh", () => api.refreshNow(), "refresh_now", undefined],
    ["activity", () => api.getActivity("c1"), "get_activity", { containerId: "c1" }],
    [
      "turns",
      () => api.getTurns("c1", "codex"),
      "get_turns",
      { containerId: "c1", agent: "codex" },
    ],
    ["logs default tail", () => api.getLogs("c1"), "get_logs", { containerId: "c1", tail: 200 }],
    ["logs tail", () => api.getLogs("c1", 50), "get_logs", { containerId: "c1", tail: 50 }],
    ["stop", () => api.stopContainer("c1"), "stop_container", { containerId: "c1" }],
    ["stop all", () => api.stopAll(), "stop_all", undefined],
    [
      "reveal",
      () => api.revealMount("c1", "/p"),
      "reveal_mount",
      { containerId: "c1", path: "/p" },
    ],
    ["terminal", () => api.openTerminal("c1"), "open_terminal", { containerId: "c1" }],
    ["folder", () => api.openProject("c1"), "open_project", { containerId: "c1" }],
    [
      "history",
      () => api.getHistory(24, "web-shop", "codex"),
      "get_history",
      { sinceHours: 24, project: "web-shop", agent: "codex" },
    ],
    ["clear history", () => api.clearHistory(), "clear_history", undefined],
    [
      "preview allow",
      () => api.previewAllow("c1", [{ host: "a.b", ports: [443] }]),
      "preview_allow",
      { containerId: "c1", selections: [{ host: "a.b", ports: [443] }] },
    ],
    [
      "apply allow",
      () => api.applyAllow("c1", "project", ["a.b"]),
      "apply_allow",
      { containerId: "c1", target: "project", entries: ["a.b"] },
    ],
    ["check update", () => api.checkForUpdate(), "check_for_update", undefined],
    ["install update", () => api.installUpdate(), "install_update", undefined],
    ["version", () => api.appVersion(), "app_version", undefined],
    [
      "editor project",
      () => api.openInEditor("c1"),
      "open_in_editor",
      { containerId: "c1", path: null },
    ],
    [
      "editor file",
      () => api.openInEditor("c1", "/p/a.ts"),
      "open_in_editor",
      { containerId: "c1", path: "/p/a.ts" },
    ],
  ];

  for (const [name, call, command, args] of cases) {
    it(`${name} invokes ${command}`, async () => {
      await call();
      expect(invoke).toHaveBeenCalledTimes(1);
      const [cmd, payload] = invoke.mock.calls[0] ?? [];
      expect(cmd).toBe(command);
      expect(payload).toEqual(args);
    });
  }
});
