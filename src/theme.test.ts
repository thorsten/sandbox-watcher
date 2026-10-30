import { describe, expect, it } from "vitest";
import { applyTheme } from "./theme";

function fakeRoot() {
  return { dataset: {} as Record<string, string> } as unknown as HTMLElement;
}

describe("applyTheme", () => {
  it("sets explicit themes on the root", () => {
    const root = fakeRoot();
    applyTheme("dark", root);
    expect(root.dataset.theme).toBe("dark");
    applyTheme("light", root);
    expect(root.dataset.theme).toBe("light");
  });
  it("removes the attribute for system", () => {
    const root = fakeRoot();
    applyTheme("dark", root);
    applyTheme("system", root);
    expect(root.dataset.theme).toBeUndefined();
  });
});
