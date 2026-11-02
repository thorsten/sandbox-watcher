import { afterEach, describe, expect, it } from "vitest";
import { de } from "./de";
import { en } from "./en";
import { formatDateTime, formatNumber, formatTime, getLanguage, setLanguage, t } from "./index";

afterEach(() => setLanguage("en"));

const placeholders = (v: string | { one: string; other: string }) =>
  [...new Set((typeof v === "string" ? v : `${v.one} ${v.other}`).match(/\{\w+\}/g) ?? [])]
    .toSorted()
    .join(" ");

describe("dictionaries", () => {
  it("have the same keys and the same placeholders per key", () => {
    expect(Object.keys(de).toSorted()).toEqual(Object.keys(en).toSorted());
    const keys = Object.keys(en) as (keyof typeof en)[];
    expect(keys.map((key) => `${key}: ${placeholders(de[key])}`)).toEqual(
      keys.map((key) => `${key}: ${placeholders(en[key])}`),
    );
  });
});

describe("t", () => {
  it("defaults to English", () => {
    expect(getLanguage()).toBe("en");
    expect(t("common.unknown")).toBe("unknown");
  });
  it("switches language", () => {
    setLanguage("de");
    expect(t("common.unknown")).toBe("unbekannt");
  });
  it("interpolates and leaves unknown placeholders alone", () => {
    expect(t("stop.noSessions", { name: "web" })).toBe("web: no active sessions");
    expect(t("stop.noSessions")).toBe("{name}: no active sessions");
  });
  it("picks plural forms from n", () => {
    expect(t("stop.stopped", { n: 1 })).toBe("Stopped 1 container");
    expect(t("stop.stopped", { n: 0 })).toBe("Stopped 0 containers");
    setLanguage("de");
    expect(t("uptime.days", { n: 1 })).toBe("Seit 1 Tag");
    expect(t("uptime.days", { n: 3 })).toBe("Seit 3 Tagen");
  });
  it("formats numbers for the language", () => {
    expect(t("stop.stoppedPart", { n: 1234 })).toBe("stopped 1,234");
    setLanguage("de");
    expect(t("stop.stoppedPart", { n: 1234 })).toBe("1.234 gestoppt");
    expect(formatNumber(1.5)).toBe("1,5");
  });
});

describe("dates", () => {
  const iso = "2026-10-30T12:34:56Z";
  it("follow the language", () => {
    const enText = formatDateTime(iso);
    expect(enText).toContain("2026");
    setLanguage("de");
    const deText = formatDateTime(iso);
    expect(deText).toContain("2026");
    expect(deText).not.toBe(enText);
    expect(deText).toMatch(/\d{2}\.\d{2}\.2026/);
  });
  it("report invalid input as unknown", () => {
    expect(formatDateTime("garbage")).toBe("unknown");
  });
  it("include seconds for the refresh time", () => {
    expect(formatTime(new Date(iso))).toMatch(/\d{1,2}:\d{2}:\d{2}/);
  });
});
