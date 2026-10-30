import { describe, expect, it } from "vitest";
import { usage } from "./test/fixtures";
import { formatCost, formatDuration, formatTokens, totalTokens } from "./usageFormat";

describe("usage formatting, more", () => {
  it("sums all token kinds", () => {
    expect(
      totalTokens(
        usage({ input_tokens: 1, output_tokens: 2, cache_read_tokens: 3, cache_write_tokens: 4 }),
      ),
    ).toBe(10);
  });
  it("does not count reasoning tokens twice", () => {
    expect(totalTokens(usage({ input_tokens: 0, output_tokens: 10, reasoning_tokens: 7 }))).toBe(
      10,
    );
  });
  it("treats mixed and reported costs as exact amounts", () => {
    expect(formatCost(2, "mixed")).toBe("$2.00");
    expect(formatCost(2, "unknown")).toBe("$2.00");
  });
  it("formats zero", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatDuration(0)).toBe("0s");
    expect(formatCost(0, "reported")).toBe("$0.0000");
  });
  it("formats exact thousands", () => {
    expect(formatTokens(1000)).toBe("1.0k");
    expect(formatTokens(1_000_000)).toBe("1.00M");
  });
});
