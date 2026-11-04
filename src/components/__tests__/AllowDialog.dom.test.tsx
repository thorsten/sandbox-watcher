// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { click, flush, mount } from "../../test/dom";
import { invoke } from "../../test/tauriMock";
import type { AllowPreview, CommandError } from "../../types";
import { AllowDialog } from "../AllowDialog";

const preview: AllowPreview = {
  entries: ["api.example.com:443"],
  global_path: "/home/dev/.config/sandbox/config.toml",
  project_path: null,
  already_in_global: [],
  already_in_project: [],
  global_hash: "abc",
  project_hash: null,
  global_unavailable: null,
  project_unavailable: null,
};

/** Answer the preview, then reject the write with `error`. */
function script(error: unknown) {
  const previews: number[] = [];
  invoke.mockImplementation((cmd) => {
    if (cmd === "preview_allow") {
      previews.push(previews.length);
      return Promise.resolve(preview);
    }
    if (cmd === "apply_allow") return Promise.reject(error);
    return Promise.resolve(undefined);
  });
  return previews;
}

function applyButton(m: { container: HTMLElement }): HTMLButtonElement {
  return m.container.querySelector<HTMLButtonElement>("button.primary")!;
}

const noop = vi.fn<() => void>();

afterEach(() => invoke.mockReset());

describe("AllowDialog errors", () => {
  it("refreshes the preview when the remote config changed", async () => {
    const changed: CommandError = { code: "config-changed", host: "devbox" };
    const previews = script(changed);
    const m = await mount(<AllowDialog containerKey="devbox/b" selections={[]} onClose={noop} />);
    await flush();
    await click(applyButton(m));
    await flush();
    expect(m.text()).toContain(
      "The config on devbox changed since the preview. The preview was refreshed",
    );
    expect(previews).toHaveLength(2);
    await m.unmount();
  });

  it("translates other command errors and keeps the detail", async () => {
    const refused: CommandError = {
      code: "config-refused",
      path: "/home/dev/.config/sandbox/config.toml",
      reason: "is a link",
    };
    const previews = script(refused);
    const m = await mount(<AllowDialog containerKey="devbox/b" selections={[]} onClose={noop} />);
    await flush();
    await click(applyButton(m));
    await flush();
    expect(m.text()).toContain(
      "Writing /home/dev/.config/sandbox/config.toml was refused: is a link",
    );
    expect(m.text()).toContain("If the write may have gone through");
    expect(previews).toHaveLength(1);
    await m.unmount();
  });
});
