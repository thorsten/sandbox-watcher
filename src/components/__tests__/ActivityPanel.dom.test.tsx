// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { advance, deferred, flush, mount, type Deferred } from "../../test/dom";
import { agentActivity, containerActivity, NOW } from "../../test/fixtures";
import { invoke } from "../../test/tauriMock";
import { ActivityPanel, POLL_MS } from "../ActivityPanel";
import type { ContainerActivity } from "../../types";

/** Every get_activity call becomes one deferred the test settles. */
function activityCalls() {
  const calls: { key: string; d: Deferred<ContainerActivity> }[] = [];
  invoke.mockImplementation((cmd, args) => {
    if (cmd !== "get_activity") return Promise.resolve(undefined);
    const d = deferred<ContainerActivity>();
    calls.push({ key: (args as { containerKey: string }).containerKey, d });
    return d.promise;
  });
  return calls;
}

function withBranch(branch: string): ContainerActivity {
  return containerActivity({
    working_tree: {
      is_repo: true,
      branch,
      changes: [],
      shortstat: "",
      truncated: false,
    },
  });
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});
afterEach(() => {
  vi.useRealTimers();
  invoke.mockReset();
});

describe("ActivityPanel polling", () => {
  it("waits for a slow request instead of starting another", async () => {
    const calls = activityCalls();
    const m = await mount(<ActivityPanel containerKey="c1" now={NOW} />);
    expect(m.text()).toContain("Looking inside the sandbox");
    await advance(POLL_MS * 3);
    expect(calls).toHaveLength(1);

    calls[0]!.d.resolve(withBranch("main"));
    await flush();
    expect(m.text()).toContain("branch main");
    await advance(POLL_MS);
    expect(calls).toHaveLength(2);
    await m.unmount();
  });

  it("ignores the old container's late answer after switching", async () => {
    const calls = activityCalls();
    const m = await mount(<ActivityPanel containerKey="c1" now={NOW} />);
    await m.rerender(<ActivityPanel containerKey="c2" now={NOW} />);
    expect(calls.map((c) => c.key)).toEqual(["c1", "c2"]);
    expect(m.text()).toContain("Looking inside the sandbox");

    calls[1]!.d.resolve(withBranch("two"));
    await flush();
    calls[0]!.d.resolve(withBranch("one"));
    await flush();
    expect(m.text()).toContain("branch two");
    expect(m.text()).not.toContain("branch one");

    await advance(POLL_MS);
    expect(calls).toHaveLength(3);
    expect(calls[2]!.key).toBe("c2");
    await m.unmount();
  });

  it("shows the loading state again after switching while data is on screen", async () => {
    const calls = activityCalls();
    const m = await mount(<ActivityPanel containerKey="c1" now={NOW} />);
    calls[0]!.d.resolve(withBranch("one"));
    await flush();
    expect(m.text()).toContain("branch one");
    await m.rerender(<ActivityPanel containerKey="c2" now={NOW} />);
    expect(m.text()).toContain("Looking inside the sandbox");
    await m.unmount();
  });

  it("stops polling on unmount and drops the in-flight answer", async () => {
    const calls = activityCalls();
    const errors = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const m = await mount(<ActivityPanel containerKey="c1" now={NOW} />);
    await m.unmount();
    calls[0]!.d.resolve(withBranch("gone"));
    await flush();
    await advance(POLL_MS * 5);
    expect(calls).toHaveLength(1);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });

  it("shows the error, keeps the last activity and recovers on the next poll", async () => {
    const calls = activityCalls();
    const m = await mount(<ActivityPanel containerKey="c1" now={NOW} />);
    calls[0]!.d.resolve(containerActivity({ agents: [agentActivity()] }));
    await flush();
    expect(m.text()).toContain("Claude Code");

    await advance(POLL_MS);
    calls[1]!.d.reject(new Error("docker exec failed"));
    await flush();
    expect(m.text()).toContain("docker exec failed");
    expect(m.text()).toContain("Claude Code");

    await advance(POLL_MS);
    expect(calls).toHaveLength(3);
    calls[2]!.d.resolve(containerActivity({ agents: [agentActivity()] }));
    await flush();
    expect(m.text()).not.toContain("docker exec failed");
    await m.unmount();
  });

  it("shows only the error when the first request fails", async () => {
    const calls = activityCalls();
    const m = await mount(<ActivityPanel containerKey="c1" now={NOW} />);
    calls[0]!.d.reject(new Error("no such container"));
    await flush();
    expect(m.text()).toBe("no such container");
    await m.unmount();
  });
});
