import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyConfirmStop,
  confirmStopEnabled,
  copyText,
  describeInterruption,
  networkLogsCommand,
} from "./actions";
import { container, session } from "./test/fixtures";

describe("confirm-stop flag", () => {
  afterEach(() => applyConfirmStop(true));
  it("defaults to asking", () => {
    expect(confirmStopEnabled()).toBe(true);
  });
  it("follows the settings value", () => {
    applyConfirmStop(false);
    expect(confirmStopEnabled()).toBe(false);
    applyConfirmStop(true);
    expect(confirmStopEnabled()).toBe(true);
  });
});

describe("describeInterruption", () => {
  it("pluralises sessions", () => {
    const [line] = describeInterruption([
      container({
        sessions: [session({ pid: 1 }), session({ pid: 2, command: "zsh -l", agent: "shell" })],
      }),
    ]);
    expect(line).toContain("2 sessions will be interrupted");
    expect(line).toContain("zsh -l (PID 2)");
  });
  it("returns one line per container", () => {
    expect(describeInterruption([container({ id: "a" }), container({ id: "b" })])).toHaveLength(2);
  });
});

describe("networkLogsCommand", () => {
  it("quotes spaces", () => {
    expect(networkLogsCommand("/a b/c")).toBe("cd '/a b/c' && sandbox network logs --status=all");
  });
});

describe("copyText", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("reports success from the clipboard API", async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    expect(await copyText("hello")).toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });
  it("reports failure when the clipboard rejects", async () => {
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: vi.fn<() => Promise<void>>(async () => Promise.reject(new Error("denied"))),
      },
    });
    expect(await copyText("x")).toBe(false);
  });
  it("reports failure when there is no clipboard", async () => {
    vi.stubGlobal("navigator", {});
    expect(await copyText("x")).toBe(false);
  });
});
