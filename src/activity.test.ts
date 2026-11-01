import { describe, expect, it } from "vitest";
import {
  describeAttribution,
  describeCoverage,
  sessionsWithoutTranscript,
  transcriptTitle,
} from "./activity";
import { agentActivity, containerActivity, session } from "./test/fixtures";

describe("describeAttribution", () => {
  it("names the session and the evidence", () => {
    expect(describeAttribution({ kind: "session", pid: 4242, evidence: "open-file" })).toBe(
      "Session PID 4242 (holds the file open)",
    );
    expect(describeAttribution({ kind: "session", pid: 1, evidence: "only-session" })).toContain(
      "only session",
    );
    expect(describeAttribution({ kind: "session", pid: 1, evidence: "start-time" })).toContain(
      "started right before",
    );
  });
  it("lists the candidates of a shared transcript", () => {
    expect(describeAttribution({ kind: "agent", candidates: [4242, 4300] })).toBe(
      "Agent-level: could be PID 4242 or 4300",
    );
  });
});

describe("describeCoverage", () => {
  it("is silent for a fully read file", () => {
    expect(
      describeCoverage({ complete: true, pending_bytes: 0, skipped_bytes: 0, restarts: 0 }),
    ).toBeNull();
  });
  it("names pending bytes, skipped lines and replacements", () => {
    const text = describeCoverage({
      complete: false,
      pending_bytes: 12 * 1024 * 1024,
      skipped_bytes: 5 * 1024 * 1024,
      restarts: 1,
    });
    expect(text).toContain("Totals incomplete");
    expect(text).toContain("12.0 MiB to go");
    expect(text).toContain("5.00 MiB of oversized lines skipped");
    expect(text).toContain("replaced once; earlier totals kept");
    expect(
      describeCoverage({ complete: false, pending_bytes: 0, skipped_bytes: 0, restarts: 0 }),
    ).toContain("still reading the transcript");
  });
});

describe("sessionsWithoutTranscript", () => {
  it("skips sessions covered by a transcript and shells", () => {
    const activity = containerActivity({
      sessions: [
        { session: session({ pid: 1 }), started_at: null, tree: [], running_now: [] },
        {
          session: session({ pid: 2, agent: "codex", command: "codex" }),
          started_at: null,
          tree: [],
          running_now: [],
        },
        {
          session: session({ pid: 3, agent: "shell", command: "zsh" }),
          started_at: null,
          tree: [],
          running_now: [],
        },
        {
          session: session({ pid: 4, agent: "claude-code" }),
          started_at: null,
          tree: [],
          running_now: [],
        },
      ],
      agents: [agentActivity({ attribution: { kind: "agent", candidates: [1, 4] } })],
    });
    expect(sessionsWithoutTranscript(activity).map((s) => s.pid)).toEqual([2]);
  });
});

describe("transcriptTitle", () => {
  it("shows the pid or the number of sharing sessions", () => {
    expect(transcriptTitle(agentActivity())).toBe("Claude Code · PID 4242");
    expect(
      transcriptTitle(agentActivity({ attribution: { kind: "agent", candidates: [1, 2, 3] } })),
    ).toBe("Claude Code · shared by 3 sessions");
  });
});
