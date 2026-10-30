import { describe, expect, it } from "vitest";
import { areaPath, formatBytes, formatPercent, linePath, sparkPoints } from "./sparkline";

describe("sparkPoints", () => {
  it("anchors the y axis at zero and spreads x across the width", () => {
    const points = sparkPoints([0, 50, 100], 200, 40, 100);
    expect(points).toEqual([
      { x: 0, y: 40 },
      { x: 100, y: 20 },
      { x: 200, y: 0 },
    ]);
  });
  it("scales to the data max when none is pinned", () => {
    const points = sparkPoints([1, 2], 10, 10);
    expect(points[1]?.y).toBe(0);
    expect(points[0]?.y).toBe(5);
  });
  it("handles single and empty series", () => {
    expect(sparkPoints([], 10, 10)).toEqual([]);
    expect(sparkPoints([3], 10, 10)).toEqual([{ x: 10, y: 0 }]);
  });
});

describe("paths", () => {
  it("builds line and closed area paths", () => {
    const points = sparkPoints([0, 10], 10, 10, 10);
    expect(linePath(points)).toBe("M0.0,10.0 L10.0,0.0");
    expect(areaPath(points, 10)).toBe("M0.0,10.0 L10.0,0.0 L10.0,10 L0.0,10 Z");
    expect(areaPath([], 10)).toBe("");
  });
});

describe("formatting", () => {
  it("formats bytes in binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.50 KiB");
    expect(formatBytes(734003200)).toBe("700 MiB");
    expect(formatBytes(4 * 1024 ** 3)).toBe("4.00 GiB");
    expect(formatBytes(-1)).toBe("–");
  });
  it("formats percentages with sensible precision", () => {
    expect(formatPercent(3.456)).toBe("3.5%");
    expect(formatPercent(42.4)).toBe("42%");
  });
});
