import { describe, expect, it } from "vitest";
import { applyLanguage } from "./language";

describe("applyLanguage", () => {
  it("sets the lang attribute on the root", () => {
    const root = { lang: "en" } as HTMLElement;
    applyLanguage("de", root);
    expect(root.lang).toBe("de");
    applyLanguage("en", root);
    expect(root.lang).toBe("en");
  });
});
