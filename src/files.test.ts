import { describe, expect, it } from "vitest";
import {
  changed,
  commonDir,
  describeFiles,
  fileStatus,
  relativePath,
  summarizeFiles,
} from "./files";
import { file } from "./test/fixtures";

describe("fileStatus", () => {
  it("ranks deleted over created over edited over read", () => {
    expect(fileStatus(file({ deleted: true, created: true, edits: 2 }))).toBe("deleted");
    expect(fileStatus(file({ created: true, edits: 2 }))).toBe("created");
    expect(fileStatus(file({ edits: 1 }))).toBe("edited");
    expect(fileStatus(file())).toBe("read");
  });
  it("counts only reads as unchanged", () => {
    expect(changed(file())).toBe(false);
    expect(changed(file({ edits: 1 }))).toBe(true);
    expect(changed(file({ deleted: true }))).toBe(true);
  });
});

describe("relativePath", () => {
  it("strips the project directory", () => {
    expect(relativePath("/Users/dev/web-shop/src/a.ts", "/Users/dev/web-shop")).toBe("src/a.ts");
    expect(relativePath("/Users/dev/web-shop/src/a.ts", "/Users/dev/web-shop/")).toBe("src/a.ts");
    expect(relativePath("/Users/dev/web-shop", "/Users/dev/web-shop")).toBe(".");
  });
  it("leaves paths outside the project and sibling prefixes alone", () => {
    expect(relativePath("/etc/hosts", "/Users/dev/web-shop")).toBe("/etc/hosts");
    expect(relativePath("/Users/dev/web-shop-2/a.ts", "/Users/dev/web-shop")).toBe(
      "/Users/dev/web-shop-2/a.ts",
    );
  });
  it("handles Windows separators and a missing project dir", () => {
    expect(relativePath("C:\\w\\app\\src\\a.ts", "C:\\w\\app")).toBe("src\\a.ts");
    expect(relativePath("/x/y", null)).toBe("/x/y");
  });
});

describe("summaries", () => {
  it("counts changed and read files", () => {
    const files = [file({ edits: 1 }), file({ created: true }), file(), file()];
    expect(summarizeFiles(files)).toEqual({ changed: 2, read: 2 });
    expect(describeFiles(files)).toBe("2 changed · 2 read");
    expect(describeFiles([file()])).toBe("1 read");
    expect(describeFiles([])).toBe("no files");
  });
});

describe("commonDir", () => {
  it("finds the shared parent", () => {
    expect(commonDir(["/p/src/a.ts", "/p/src/b.ts", "/p/README.md"])).toBe("/p");
    expect(commonDir(["/p/src/a.ts"])).toBe("/p/src");
  });
  it("returns null for relative or unrelated paths", () => {
    expect(commonDir([])).toBeNull();
    expect(commonDir(["src/a.ts"])).toBeNull();
    expect(commonDir(["/a/x", "/b/y"])).toBeNull();
  });
});
