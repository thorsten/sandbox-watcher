// @vitest-environment jsdom
import { useCallback } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advance, click, deferred, fire, flush, mount, type Deferred } from "./test/dom";
import { usePolling } from "./usePolling";

/** Renders the hook state; `fetch` is called with the current key for every request. */
function Probe({
  fetch,
  id,
  interval,
}: {
  fetch: (id: string) => Promise<string>;
  id: string;
  interval: number | null;
}) {
  const load = useCallback(() => fetch(id), [fetch, id]);
  const { data, error, refresh } = usePolling(load, interval);
  return (
    <output>
      data={data ?? "none"} error={error ?? "none"}
      <button type="button" onClick={refresh}>
        refresh
      </button>
    </output>
  );
}

/** Pretend the window was hidden (closed to the tray) or shown again. */
async function setVisibility(state: "hidden" | "visible") {
  Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
  await fire(document, "visibilitychange");
}

/** A fetch whose every call is settled by the test, in order. */
function controlled() {
  const calls: { id: string; d: Deferred<string> }[] = [];
  const fetch = vi.fn<(id: string) => Promise<string>>((id) => {
    const d = deferred<string>();
    calls.push({ id, d });
    return d.promise;
  });
  return { fetch, calls };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});
afterEach(() => {
  vi.useRealTimers();
});

describe("usePolling", () => {
  it("never overlaps requests: the next poll is scheduled after the previous one settled", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(m.text()).toContain("data=none");

    await advance(350); // longer than three intervals while the first request is still pending
    expect(fetch).toHaveBeenCalledTimes(1);

    calls[0]!.d.resolve("first");
    await flush();
    expect(m.text()).toContain("data=first");
    expect(fetch).toHaveBeenCalledTimes(1);

    await advance(99);
    expect(fetch).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(fetch).toHaveBeenCalledTimes(2);
    await m.unmount();
  });

  it("drops a late response after the key changed", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    await m.rerender(<Probe fetch={fetch} id="b" interval={100} />);
    expect(calls.map((c) => c.id)).toEqual(["a", "b"]);
    expect(m.text()).toContain("data=none");

    calls[1]!.d.resolve("from b");
    await flush();
    expect(m.text()).toContain("data=from b");

    calls[0]!.d.resolve("stale a");
    await flush();
    expect(m.text()).toContain("data=from b");

    // Only one timer is alive: the one that belongs to "b".
    await advance(100);
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(calls[2]!.id).toBe("b");
    await m.unmount();
  });

  it("drops a late response and stops polling after unmount", async () => {
    const { fetch, calls } = controlled();
    const errors = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    await m.unmount();
    calls[0]!.d.resolve("late");
    await flush();
    await advance(1000);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });

  it("drops a late response after the interval changed and restarts from the new mode", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    await m.rerender(<Probe fetch={fetch} id="a" interval={null} />);
    expect(fetch).toHaveBeenCalledTimes(2);

    calls[0]!.d.resolve("old mode");
    await flush();
    expect(m.text()).toContain("data=none");

    calls[1]!.d.resolve("single");
    await flush();
    expect(m.text()).toContain("data=single");
    await advance(1000);
    expect(fetch).toHaveBeenCalledTimes(2); // interval null: no repeat
    await m.unmount();
  });

  it("keeps the last data on error, retries at the same rate and recovers", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    calls[0]!.d.resolve("ok");
    await flush();

    await advance(100);
    calls[1]!.d.reject(new Error("boom"));
    await flush();
    expect(m.text()).toContain("data=ok error=boom");

    await advance(100);
    calls[2]!.d.reject(new Error("boom"));
    await flush();
    await advance(100);
    expect(fetch).toHaveBeenCalledTimes(4); // one retry per interval, no growth
    expect(calls[3]!.d).toBeDefined();

    calls[3]!.d.resolve("back");
    await flush();
    expect(m.text()).toContain("data=back error=none");
    await m.unmount();
  });

  it("keeps the data when only the interval changes", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    calls[0]!.d.resolve("kept");
    await flush();
    await m.rerender(<Probe fetch={fetch} id="a" interval={null} />);
    expect(m.text()).toContain("data=kept");
    await m.unmount();
  });

  it("stops polling while the document is hidden and resumes at once when it is shown", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    calls[0]!.d.resolve("first");
    await flush();
    expect(fetch).toHaveBeenCalledTimes(1);

    // Hidden with a poll scheduled: the timer is dropped and nothing runs.
    await setVisibility("hidden");
    await advance(1000);
    expect(fetch).toHaveBeenCalledTimes(1);

    // Shown again: one request at once, then the usual rhythm.
    await setVisibility("visible");
    expect(fetch).toHaveBeenCalledTimes(2);
    calls[1]!.d.resolve("second");
    await flush();
    await advance(100);
    expect(fetch).toHaveBeenCalledTimes(3);

    // Hidden while a request is in flight: its answer lands, no poll follows,
    // and showing the document again does not start a second request.
    await setVisibility("hidden");
    await setVisibility("visible");
    expect(fetch).toHaveBeenCalledTimes(3);
    calls[2]!.d.resolve("third");
    await flush();
    expect(m.text()).toContain("data=third");
    await advance(100);
    expect(fetch).toHaveBeenCalledTimes(4);

    // Hidden while in flight, settled while still hidden: paused until shown.
    await setVisibility("hidden");
    calls[3]!.d.resolve("fourth");
    await flush();
    await advance(1000);
    expect(fetch).toHaveBeenCalledTimes(4);
    await setVisibility("visible");
    expect(fetch).toHaveBeenCalledTimes(5);

    await setVisibility("visible");
    await m.unmount();
  });

  it("refresh requests now, keeps the data, drops the pending answer and restarts the timer", async () => {
    const { fetch, calls } = controlled();
    const m = await mount(<Probe fetch={fetch} id="a" interval={100} />);
    calls[0]!.d.resolve("first");
    await flush();
    await advance(100); // second request pending
    expect(fetch).toHaveBeenCalledTimes(2);

    await click(m.container.querySelector("button")!);
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(m.text()).toContain("data=first");

    calls[1]!.d.resolve("stale");
    await flush();
    expect(m.text()).toContain("data=first");

    calls[2]!.d.resolve("fresh");
    await flush();
    expect(m.text()).toContain("data=fresh");
    await advance(100);
    expect(fetch).toHaveBeenCalledTimes(4);
    await m.unmount();
  });
});
