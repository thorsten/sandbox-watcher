import { describe, expect, it } from "vitest";
import { NOW, turn, turnReport } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { TurnsList, TurnsSection } from "../TurnsSection";

describe("TurnsSection", () => {
  it("waits for the user before reading the transcript", () => {
    const { text } = render(
      <TurnsSection containerId="c1" agent="claude-code" projectDir="/p" now={NOW} />,
    );
    expect(text).toContain("Turns");
    expect(text).toContain("Show turns");
  });
});

describe("TurnsList", () => {
  it("shows the newest turn first with its facts and files", () => {
    const report = turnReport({
      turns: [
        turn({ index: 0, prompt: "", tool_calls: 2, usage: null, files: [] }),
        turn({ index: 1, prompt: "Find the bug" }),
        turn({ index: 2, prompt: "Now add a test", started_at: "2026-10-30T11:58:00Z" }),
      ],
    });
    const { html, text } = render(
      <TurnsList report={report} containerId="c1" projectDir="/Users/dev/web-shop" now={NOW} />,
    );
    expect(text).toContain("3 turns, newest first");
    expect(text.indexOf("#2 Now add a test")).toBeLessThan(text.indexOf("#1 Find the bug"));
    expect(text).toContain("Earlier Activity before the first prompt read");
    expect(text).toContain("3m · 1.5k tokens · ≈$0.01 · 3 tool calls · 1 file changed");
    expect(text).toContain("claude-sonnet-4-5");
    expect(text).toContain("edited src/cart.ts");
    expect(count(html, "<details>")).toBe(3);
    expect(text).not.toContain("older turns not shown");
  });

  it("says when older turns were left out", () => {
    const { text } = render(
      <TurnsList report={turnReport({ truncated: true })} projectDir={null} now={NOW} />,
    );
    expect(text).toContain("1 turn, newest first · older turns not shown");
  });

  it("handles a transcript without prompts", () => {
    const { text } = render(
      <TurnsList report={turnReport({ turns: [] })} projectDir={null} now={NOW} />,
    );
    expect(text).toBe("No prompts yet.");
  });
});
