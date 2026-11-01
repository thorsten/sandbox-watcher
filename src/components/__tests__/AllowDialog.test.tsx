import { describe, expect, it, vi } from "vitest";
import { render } from "../../test/render";
import { AllowDialog, TargetChoice } from "../AllowDialog";

const noop = vi.fn<() => void>();

describe("AllowDialog", () => {
  it("renders before the preview arrives", () => {
    const { text } = render(<AllowDialog containerKey="devbox/b" selections={[]} onClose={noop} />);
    expect(text).toContain("Checking configuration");
  });

  it("names the host in the heading for remote containers", () => {
    const { text } = render(<AllowDialog containerKey="devbox/b" selections={[]} onClose={noop} />);
    expect(text).toContain("Allow blocked destinations on devbox");
  });

  it("shows why a target is unavailable and disables it", () => {
    const { html, text } = render(
      <TargetChoice
        target="project"
        selected="global"
        path="/home/dev/p/.sandbox/config.toml"
        unavailable="/home/dev/p/.sandbox is a symbolic link; refusing to write the project config"
        onSelect={noop}
      />,
    );
    expect(html).toContain("disabled");
    expect(text).toContain("is a symbolic link");
  });
});
