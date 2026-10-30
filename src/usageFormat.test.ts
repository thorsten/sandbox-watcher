import { describe, expect, it } from "vitest";
import { formatCost, formatDuration, formatTokens } from "./usageFormat";

describe("usage formatting", () => {
  it("formats token counts compactly", () => {
    expect(formatTokens(999)).toBe("999");
    expect(formatTokens(1500)).toBe("1.5k");
    expect(formatTokens(64000)).toBe("64k");
    expect(formatTokens(2_345_678)).toBe("2.35M");
  });
  it("marks estimates and tiny amounts", () => {
    expect(formatCost(1.234, "estimated")).toBe("≈$1.23");
    expect(formatCost(0.0012, "reported")).toBe("$0.0012");
    expect(formatCost(null)).toBe("no price");
  });
  it("formats durations", () => {
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(600)).toBe("10m");
    expect(formatDuration(3660)).toBe("1h 1m");
    expect(formatDuration(90000)).toBe("1d 1h");
  });
});
