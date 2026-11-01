// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advance, click, deferred, flush, mount, type Deferred } from "../../test/dom";
import { invoke } from "../../test/tauriMock";
import { HISTORY_POLL_MS, HistoryView } from "../HistoryView";
import type { Aggregate, HistoryView as HistoryData } from "../../types";

interface HistoryArgs {
  sinceHours: number | null;
  project: string | null;
  agent: string | null;
}

function historyCalls() {
  const calls: { args: HistoryArgs; d: Deferred<HistoryData> }[] = [];
  const cleared: number[] = [];
  invoke.mockImplementation((cmd, args) => {
    if (cmd === "clear_history") {
      cleared.push(1);
      return Promise.resolve(undefined);
    }
    if (cmd !== "get_history") return Promise.resolve(undefined);
    const d = deferred<HistoryData>();
    calls.push({ args: args as HistoryArgs, d });
    return d.promise;
  });
  return { calls, cleared };
}

function aggregate(sessions: number): Aggregate {
  return { key: "total", sessions, seconds: 0, tokens: 0, cost_usd: null, cost_incomplete: false };
}

function history(sessions: number, projects: string[] = []): HistoryData {
  return { sessions: [], by_agent: [], by_project: [], total: aggregate(sessions), projects };
}

function button(m: { container: HTMLElement }, label: string): HTMLButtonElement {
  const el = [...m.container.querySelectorAll("button")].find((b) => b.textContent === label);
  if (!el) throw new Error(`no button ${label}`);
  return el;
}

/** The number shown in the Sessions tile. */
function sessionsTile(m: { container: HTMLElement }): string | null {
  const tile = [...m.container.querySelectorAll(".stat")].find(
    (s) => s.querySelector(".stat-label")?.textContent === "Sessions",
  );
  return tile?.querySelector(".stat-value")?.textContent ?? null;
}

const noop = () => undefined;

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
  // jsdom has no <dialog> implementation.
  HTMLDialogElement.prototype.showModal ??= function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close ??= function () {
    this.removeAttribute("open");
  };
});
afterEach(() => {
  vi.useRealTimers();
  invoke.mockReset();
});

describe("HistoryView polling", () => {
  it("waits for a slow request instead of starting another", async () => {
    const { calls } = historyCalls();
    const m = await mount(<HistoryView onClose={noop} />);
    expect(calls).toHaveLength(1);
    expect(calls[0]!.args).toEqual({ sinceHours: 24 * 7, project: null, agent: null });
    await advance(HISTORY_POLL_MS * 3);
    expect(calls).toHaveLength(1);

    calls[0]!.d.resolve(history(3));
    await flush();
    expect(sessionsTile(m)).toBe("3");
    await advance(HISTORY_POLL_MS);
    expect(calls).toHaveLength(2);
    await m.unmount();
  });

  it("requests the new range and drops the previous range's late answer", async () => {
    const { calls } = historyCalls();
    const m = await mount(<HistoryView onClose={noop} />);
    await click(button(m, "Today"));
    expect(calls).toHaveLength(2);
    expect(calls[1]!.args.sinceHours).toBe(24);

    calls[0]!.d.resolve(history(7));
    await flush();
    expect(sessionsTile(m)).toBeNull();

    calls[1]!.d.resolve(history(1));
    await flush();
    expect(sessionsTile(m)).toBe("1");

    await advance(HISTORY_POLL_MS);
    expect(calls).toHaveLength(3);
    expect(calls[2]!.args.sinceHours).toBe(24);
    await m.unmount();
  });

  it("keeps the last data on error, shows the message and recovers", async () => {
    const { calls } = historyCalls();
    const m = await mount(<HistoryView onClose={noop} />);
    calls[0]!.d.resolve(history(3));
    await flush();

    await advance(HISTORY_POLL_MS);
    calls[1]!.d.reject(new Error("db locked"));
    await flush();
    expect(m.text()).toContain("db locked");
    expect(sessionsTile(m)).toBe("3");

    await advance(HISTORY_POLL_MS);
    expect(calls).toHaveLength(3);
    calls[2]!.d.resolve(history(4));
    await flush();
    expect(m.text()).not.toContain("db locked");
    expect(sessionsTile(m)).toBe("4");
    await m.unmount();
  });

  it("reloads once after clearing and keeps polling at the same rate", async () => {
    const { calls, cleared } = historyCalls();
    const m = await mount(<HistoryView onClose={noop} />);
    calls[0]!.d.resolve(history(3));
    await flush();

    await click(button(m, "Clear"));
    await click(button(m, "Clear history"));
    await flush();
    expect(cleared).toHaveLength(1);
    expect(calls).toHaveLength(2);

    calls[1]!.d.resolve(history(0));
    await flush();
    expect(sessionsTile(m)).toBe("0");

    await advance(HISTORY_POLL_MS);
    expect(calls).toHaveLength(3);
    await m.unmount();
  });

  it("stops polling on unmount and drops the in-flight answer", async () => {
    const { calls } = historyCalls();
    const errors = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const m = await mount(<HistoryView onClose={noop} />);
    await m.unmount();
    calls[0]!.d.resolve(history(9));
    await flush();
    await advance(HISTORY_POLL_MS * 5);
    expect(calls).toHaveLength(1);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });
});
