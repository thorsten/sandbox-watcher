import { describe, expect, it } from "vitest";
import { file, NOW } from "../../test/fixtures";
import { count, render } from "../../test/render";
import { FilesList } from "../FilesList";

const files = [
  file({ path: "/p/src/cart.ts", edits: 2, reads: 1 }),
  file({ path: "/p/src/cart.test.ts", created: true, reads: 0 }),
  file({ path: "/p/old.ts", deleted: true, reads: 0 }),
  file({ path: "/p/README.md" }),
];

describe("FilesList", () => {
  it("says when nothing was touched", () => {
    expect(render(<FilesList files={[]} projectDir="/p" now={NOW} />).text).toContain(
      "No files touched yet.",
    );
  });

  it("shows changed files with relative paths and hides reads by default", () => {
    const { text } = render(<FilesList files={files} projectDir="/p" now={NOW} />);
    expect(text).toContain("3 changed · 1 read");
    expect(text).toContain("edited src/cart.ts 2× edited · 1× read · 2m ago");
    expect(text).toContain("created src/cart.test.ts");
    expect(text).toContain("deleted old.ts");
    expect(text).not.toContain("README.md");
    expect(text).toContain("Show 1 read");
  });

  it("shows read files when not collapsed", () => {
    const { text } = render(
      <FilesList files={files} projectDir="/p" now={NOW} collapseReads={false} />,
    );
    expect(text).toContain("read README.md");
    expect(text).not.toContain("Show 1 read");
  });

  it("makes existing files clickable only with a container", () => {
    const withContainer = render(
      <FilesList files={files} containerId="c1" projectDir="/p" now={NOW} />,
    );
    expect(count(withContainer.html, 'class="link file-path"')).toBe(2);
    expect(withContainer.html).toContain('title="Open /p/src/cart.ts"');
    const without = render(<FilesList files={files} projectDir="/p" now={NOW} />);
    expect(count(without.html, 'class="link file-path"')).toBe(0);
  });

  it("derives a base directory when none is given", () => {
    const { text } = render(
      <FilesList files={files} projectDir={null} now={NOW} collapseReads={false} />,
    );
    expect(text).toContain("src/cart.ts");
    expect(text).not.toContain("/p/src/cart.ts");
  });

  it("says nothing changed when only reads exist", () => {
    const { text } = render(<FilesList files={[file()]} projectDir="/p" now={NOW} />);
    expect(text).toContain("Nothing changed yet.");
  });
});
