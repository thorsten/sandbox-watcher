import { describe, expect, it } from "vitest";
import { file, turn, usage } from "./test/fixtures";
import { turnDurationSecs, turnFacts, turnLabel } from "./turns";

describe("turn helpers", () => {
  it("labels activity before the first prompt", () => {
    expect(turnLabel(turn({ index: 4 }))).toBe("#4");
    expect(turnLabel(turn({ index: 0 }))).toBe("Earlier");
  });

  it("measures duration only with both ends", () => {
    expect(turnDurationSecs(turn())).toBe(200);
    expect(turnDurationSecs(turn({ ended_at: null }))).toBeNull();
    expect(
      turnDurationSecs(
        turn({ started_at: "2026-10-30T12:00:00Z", ended_at: "2026-10-30T11:00:00Z" }),
      ),
    ).toBeNull();
  });

  it("lists duration, tokens, cost, tools and changed files", () => {
    expect(turnFacts(turn())).toEqual([
      "3m",
      "1.5k tokens",
      "≈$0.01",
      "3 tool calls",
      "1 file changed",
    ]);
  });

  it("mentions failures and skips what is missing", () => {
    const facts = turnFacts(
      turn({
        started_at: null,
        usage: null,
        tool_calls: 1,
        tool_errors: 1,
        files: [file(), file({ path: "/a", created: true }), file({ path: "/b", deleted: true })],
      }),
    );
    expect(facts).toEqual(["1 tool call, 1 failed", "2 files changed"]);
    expect(turnFacts(turn({ started_at: null, usage: null, tool_calls: 0, files: [] }))).toEqual(
      [],
    );
  });

  it("shows reported cost without the estimate mark", () => {
    const facts = turnFacts(turn({ usage: usage({ cost_usd: 0.5, cost_source: "reported" }) }));
    expect(facts).toContain("$0.50");
  });
});
