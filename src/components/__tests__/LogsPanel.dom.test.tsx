// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advance, click, deferred, fire, flush, mount, type Deferred } from "../../test/dom";
import { invoke } from "../../test/tauriMock";
import { FOLLOW_MS, LogsPanel } from "../LogsPanel";

function logCalls() {
  const calls: { key: string; d: Deferred<string> }[] = [];
  invoke.mockImplementation((cmd, args) => {
    if (cmd !== "get_logs") return Promise.resolve(undefined);
    const d = deferred<string>();
    calls.push({ key: (args as { containerKey: string }).containerKey, d });
    return d.promise;
  });
  return calls;
}

/** jsdom has no layout: give the <pre> a scrollable size. */
function sizePre(el: HTMLElement, scrollHeight: number, clientHeight = 100) {
  Object.defineProperty(el, "scrollHeight", { value: scrollHeight, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: clientHeight, configurable: true });
}

function logView(m: { container: HTMLElement }): HTMLPreElement {
  const el = m.container.querySelector("pre.log-view");
  if (!el) throw new Error("no log view rendered");
  return el as HTMLPreElement;
}

function followBox(m: { container: HTMLElement }): HTMLInputElement {
  return m.container.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});
afterEach(() => {
  vi.useRealTimers();
  invoke.mockReset();
});

describe("LogsPanel polling", () => {
  it("does not start a second read while one is pending", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    expect(m.text()).toContain("Reading container log");
    await advance(FOLLOW_MS * 4);
    expect(calls).toHaveLength(1);
    calls[0]!.d.resolve("line 1");
    await flush();
    expect(logView(m).textContent).toBe("line 1");
    await advance(FOLLOW_MS);
    expect(calls).toHaveLength(2);
    await m.unmount();
  });

  it("drops the previous container's late log after switching", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    await m.rerender(<LogsPanel containerKey="c2" />);
    calls[1]!.d.resolve("from c2");
    await flush();
    calls[0]!.d.resolve("from c1");
    await flush();
    expect(logView(m).textContent).toBe("from c2");
    await m.unmount();
  });

  it("stops reading after unmount", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    await m.unmount();
    calls[0]!.d.resolve("late");
    await flush();
    await advance(FOLLOW_MS * 3);
    expect(calls).toHaveLength(1);
  });

  it("shows a read error, keeps the last text and recovers", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    calls[0]!.d.resolve("old text");
    await flush();
    await advance(FOLLOW_MS);
    calls[1]!.d.reject(new Error("container gone"));
    await flush();
    expect(m.text()).toContain("container gone");
    expect(logView(m).textContent).toBe("old text");
    await advance(FOLLOW_MS);
    expect(calls).toHaveLength(3);
    calls[2]!.d.resolve("new text");
    await flush();
    expect(m.text()).not.toContain("container gone");
    expect(logView(m).textContent).toBe("new text");
    await m.unmount();
  });
});

describe("LogsPanel follow", () => {
  it("scrolls to the end when new log text arrives while Follow is on", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    calls[0]!.d.resolve("a");
    await flush();
    const view = logView(m);
    sizePre(view, 500);
    await advance(FOLLOW_MS);
    calls[1]!.d.resolve("a\nb");
    await flush();
    expect(view.scrollTop).toBe(500);
    await m.unmount();
  });

  it("switching Follow off stops polling and leaves the scroll position alone", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    calls[0]!.d.resolve("a");
    await flush();
    const view = logView(m);
    sizePre(view, 500);
    view.scrollTop = 120;

    await click(followBox(m));
    expect(followBox(m).checked).toBe(false);
    // Switching the mode re-reads once; the pending follow request is dropped.
    expect(calls).toHaveLength(2);
    calls[1]!.d.resolve("a\nb");
    await flush();
    expect(view.scrollTop).toBe(120);
    await advance(FOLLOW_MS * 3);
    expect(calls).toHaveLength(2);
    await m.unmount();
  });

  it("switching Follow on reads again and jumps to the end", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    calls[0]!.d.resolve("a");
    await flush();
    await click(followBox(m));
    calls[1]!.d.resolve("a");
    await flush();
    const view = logView(m);
    sizePre(view, 800);
    view.scrollTop = 10;

    await click(followBox(m));
    expect(followBox(m).checked).toBe(true);
    expect(view.scrollTop).toBe(800);
    expect(calls).toHaveLength(3);
    await advance(FOLLOW_MS);
    expect(calls).toHaveLength(3); // still waiting for the third read
    calls[2]!.d.resolve("a\nb");
    await flush();
    await advance(FOLLOW_MS);
    expect(calls).toHaveLength(4);
    await m.unmount();
  });

  it("scrolling up by hand switches Follow off; staying at the end keeps it on", async () => {
    const calls = logCalls();
    const m = await mount(<LogsPanel containerKey="c1" />);
    calls[0]!.d.resolve("a");
    await flush();
    const view = logView(m);
    sizePre(view, 500, 100);

    view.scrollTop = 400; // at the end
    await fire(view, "scroll");
    expect(followBox(m).checked).toBe(true);

    view.scrollTop = 200;
    await fire(view, "scroll");
    expect(followBox(m).checked).toBe(false);

    // No auto-scroll any more, even when text arrives.
    calls[1]!.d.resolve("a\nb");
    await flush();
    expect(view.scrollTop).toBe(200);

    view.scrollTop = 400; // back at the end: Follow stays off until the checkbox is used
    await fire(view, "scroll");
    expect(followBox(m).checked).toBe(false);
    await m.unmount();
  });
});
