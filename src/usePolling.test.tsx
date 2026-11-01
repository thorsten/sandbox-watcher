// @vitest-environment jsdom
import { useCallback } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advance, deferred, flush, mount, type Deferred } from "./test/dom";
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
  const { data, error } = usePolling(load, interval);
  return (
    <output>
      data={data ?? "none"} error={error ?? "none"}
    </output>
  );
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
});
