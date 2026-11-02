// The pure helpers in German; the English cases live next to each helper.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { describeInterruption, describeStopReport } from "../actions";
import { formatUptime } from "../format";
import { formatRelative } from "../network";
import { container, session } from "../test/fixtures";
import { formatCost, formatDuration, formatTokens } from "../usageFormat";
import { setLanguage } from "./index";

const now = new Date("2026-10-30T12:00:00Z");

beforeEach(() => setLanguage("de"));
afterEach(() => setLanguage("en"));

describe("German helpers", () => {
  it("uptime", () => {
    expect(formatUptime(null, now)).toBe("unbekannt");
    expect(formatUptime("2026-10-30T11:59:59Z", now)).toBe("Seit 1 Sekunde");
    expect(formatUptime("2026-10-30T11:15:00Z", now)).toBe("Seit 45 Minuten");
    expect(formatUptime("2026-10-30T11:00:00Z", now)).toBe("Seit 1 Stunde");
    expect(formatUptime("2026-10-27T12:00:00Z", now)).toBe("Seit 3 Tagen");
  });
  it("relative times", () => {
    expect(formatRelative("2026-10-30T11:59:30Z", now)).toBe("vor 30 s");
    expect(formatRelative("2026-10-30T11:15:00Z", now)).toBe("vor 45 min");
    expect(formatRelative("2026-10-30T09:00:00Z", now)).toBe("vor 3 h");
    expect(formatRelative("2026-10-29T10:00:00Z", now)).toBe("vor 1 d");
  });
  it("durations, tokens and cost", () => {
    expect(formatDuration(45)).toBe("45 s");
    expect(formatDuration(3660)).toBe("1 h 1 min");
    expect(formatDuration(90000)).toBe("1 d 1 h");
    expect(formatTokens(1500)).toBe("1,5k");
    expect(formatTokens(2_345_678)).toBe("2,35M");
    // Intl separates amount and symbol with a no-break space.
    expect(formatCost(1.234, "estimated")).toBe("≈1,23\u00a0$");
    expect(formatCost(0.0012, "reported")).toBe("0,0012\u00a0$");
    expect(formatCost(null)).toBe("kein Preis");
  });
  it("stop texts", () => {
    const one = container({ id: "a", name: "sandbox-a", sessions: [session({ pid: 7 })] });
    expect(describeInterruption([one])[0]).toMatch(/^sandbox-a: 1 Session wird unterbrochen: /);
    expect(describeInterruption([container({ id: "b", name: "b", sessions: [] })])).toEqual([
      "b: keine aktiven Sessions",
    ]);
    expect(describeStopReport({ results: [], failures: [] })).toBe("0 Container gestoppt");
    expect(
      describeStopReport({
        results: [],
        failures: [{ host: "local", runtime: "docker", error: "down" }],
      }),
    ).toBe("Docker-Container auf diesem Rechner konnten nicht aufgelistet werden: down");
  });
});
