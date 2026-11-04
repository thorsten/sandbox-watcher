import { afterEach, describe, expect, it } from "vitest";
import { describeError, isCommandError } from "./errors";
import { setLanguage } from "./i18n";
import type { CommandError } from "./types";

afterEach(() => setLanguage("en"));

describe("describeError", () => {
  it("translates a command error and fills in its fields", () => {
    const e: CommandError = { code: "config-changed", host: "devbox" };
    expect(describeError(e)).toBe("The config on devbox changed since the preview.");
    setLanguage("de");
    expect(describeError(e)).toBe(
      "Die Konfiguration auf devbox hat sich seit der Vorschau geändert.",
    );
  });

  it("keeps what the runtime said as untranslated detail", () => {
    const e: CommandError = {
      code: "runtime-unavailable",
      runtime: "docker",
      reason: "connection refused",
    };
    expect(describeError(e)).toBe("Cannot connect to docker: connection refused");
    setLanguage("de");
    expect(describeError(e)).toBe("Keine Verbindung zu docker: connection refused");
    expect(describeError({ code: "other", detail: "unexpected apply reply: x" })).toBe(
      "unexpected apply reply: x",
    );
  });

  it("shows an Error's message and anything else as text", () => {
    expect(describeError(new Error("boom"))).toBe("boom");
    expect(describeError("plain")).toBe("plain");
    expect(describeError(42)).toBe("42");
  });

  it("does not mistake other objects for command errors", () => {
    expect(isCommandError({ code: "made-up" })).toBe(false);
    expect(isCommandError({ code: 7 })).toBe(false);
    expect(isCommandError(null)).toBe(false);
    expect(describeError({ code: "made-up" })).toBe("[object Object]");
  });

  it("matches a specific code", () => {
    const e: CommandError = { code: "local-only" };
    expect(isCommandError(e, "local-only")).toBe(true);
    expect(isCommandError(e, "config-changed")).toBe(false);
  });
});
