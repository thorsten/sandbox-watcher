import { describe, expect, it } from "vitest";
import { progressPercent, summarizeNotes } from "./updateFormat";

describe("progressPercent", () => {
  it("is null without a known size", () => {
    expect(progressPercent(100, null)).toBeNull();
    expect(progressPercent(100, 0)).toBeNull();
  });
  it("rounds and clamps", () => {
    expect(progressPercent(1, 3)).toBe(33);
    expect(progressPercent(150, 100)).toBe(100);
    expect(progressPercent(-5, 100)).toBe(0);
  });
});

describe("summarizeNotes", () => {
  const notes = `### Features

- **core:** token usage and history ([bd57fa8](https://github.com/x/commit/bd57fa8aaaa))
- history view in the app ([b48a56e](https://github.com/x/commit/b48a56e))

### Fixes

- space the live badge ([c7c5cb8](https://github.com/x/commit/c7c5cb8))

Unsigned builds for macOS, Windows and Linux.`;

  it("keeps bullet items without links and bold scopes", () => {
    expect(summarizeNotes(notes)).toEqual([
      "core: token usage and history",
      "history view in the app",
      "space the live badge",
    ]);
  });
  it("limits the number of lines", () => {
    expect(summarizeNotes(notes, 1)).toHaveLength(1);
  });
  it("handles missing notes", () => {
    expect(summarizeNotes(null)).toEqual([]);
    expect(summarizeNotes("no bullets here")).toEqual([]);
  });
});
