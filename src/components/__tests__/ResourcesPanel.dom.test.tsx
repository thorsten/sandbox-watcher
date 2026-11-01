// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advance, deferred, flush, mount, type Deferred } from "../../test/dom";
import { invoke } from "../../test/tauriMock";
import { POLL_MS, ResourcesPanel } from "../ResourcesPanel";
import type { ResourceSample } from "../../types";

type Stats = Record<string, ResourceSample[]>;

/** Every get_stats call becomes one deferred the test settles. */
function statsCalls() {
  const calls: Deferred<Stats>[] = [];
  invoke.mockImplementation((cmd) => {
    if (cmd !== "get_stats") return Promise.resolve(undefined);
    const d = deferred<Stats>();
    calls.push(d);
    return d.promise;
  });
  return calls;
}

function sample(cpu: number): ResourceSample {
  return {
    container_id: "id",
    at: "2026-10-30T12:00:00Z",
    cpu_percent: cpu,
    memory_used_bytes: 1024,
    memory_limit_bytes: null,
    net_rx_bytes: 0,
    net_tx_bytes: 0,
    block_read_bytes: 0,
    block_write_bytes: 0,
  };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});
afterEach(() => {
  vi.useRealTimers();
  invoke.mockReset();
});

describe("ResourcesPanel polling", () => {
  it("waits for a slow stats request instead of starting another", async () => {
    const calls = statsCalls();
    const m = await mount(<ResourcesPanel containerKey="c1" />);
    expect(m.text()).toContain("Collecting resource samples");
    await advance(POLL_MS * 3);
    expect(calls).toHaveLength(1);

    calls[0]!.resolve({ c1: [sample(42)] });
    await flush();
    expect(m.text()).toContain("42%");
    await advance(POLL_MS);
    expect(calls).toHaveLength(2);
    await m.unmount();
  });

  it("shows the other container's samples from the same answer after switching", async () => {
    const calls = statsCalls();
    const m = await mount(<ResourcesPanel containerKey="c1" />);
    calls[0]!.resolve({ c1: [sample(10)], c2: [sample(90)] });
    await flush();
    expect(m.text()).toContain("10%");

    await m.rerender(<ResourcesPanel containerKey="c2" />);
    expect(m.text()).toContain("90%");
    expect(calls).toHaveLength(1); // one stats call covers every container

    await m.rerender(<ResourcesPanel containerKey="c3" />);
    expect(m.text()).toContain("No samples yet");
    await m.unmount();
  });

  it("keeps the last samples on error, shows the message and recovers", async () => {
    const calls = statsCalls();
    const m = await mount(<ResourcesPanel containerKey="c1" />);
    calls[0]!.resolve({ c1: [sample(10)] });
    await flush();

    await advance(POLL_MS);
    calls[1]!.reject(new Error("stats unavailable"));
    await flush();
    expect(m.text()).toContain("stats unavailable");
    expect(m.text()).toContain("10%");

    await advance(POLL_MS);
    expect(calls).toHaveLength(3);
    calls[2]!.resolve({ c1: [sample(10), sample(20)] });
    await flush();
    expect(m.text()).not.toContain("stats unavailable");
    expect(m.text()).toContain("20%");
    await m.unmount();
  });

  it("shows only the error when the first request fails", async () => {
    const calls = statsCalls();
    const m = await mount(<ResourcesPanel containerKey="c1" />);
    calls[0]!.reject(new Error("no runtime"));
    await flush();
    expect(m.text()).toContain("no runtime");
    expect(m.text()).not.toContain("No samples yet");
    await m.unmount();
  });

  it("stops polling on unmount and drops the in-flight answer", async () => {
    const calls = statsCalls();
    const errors = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const m = await mount(<ResourcesPanel containerKey="c1" />);
    await m.unmount();
    calls[0]!.resolve({ c1: [sample(1)] });
    await flush();
    await advance(POLL_MS * 5);
    expect(calls).toHaveLength(1);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });
});
