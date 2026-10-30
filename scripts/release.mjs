#!/usr/bin/env node
// Prepare a release: bump the version everywhere, regenerate CHANGELOG.md,
// commit and tag. Pushing is left to you so nothing leaves the machine by
// accident.
//
//   pnpm release 0.1.1
//   pnpm release 0.1.1 --no-tag     # bump and commit only
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const [version, ...flags] = process.argv.slice(2);
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: pnpm release <semver> [--no-tag]");
  process.exit(2);
}
const tag = `v${version}`;

function run(cmd, args) {
  return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }).trim();
}

if (run("git", ["status", "--porcelain"]).length > 0) {
  console.error("working tree is not clean; commit or stash first");
  process.exit(1);
}
if (run("git", ["tag", "--list", tag])) {
  console.error(`tag ${tag} already exists`);
  process.exit(1);
}

function edit(path, replacer) {
  const before = readFileSync(path, "utf8");
  const after = replacer(before);
  if (after === before) throw new Error(`${path}: nothing changed`);
  writeFileSync(path, after);
  console.log(`updated ${path}`);
}

// Workspace version: first `version = "..."` under [workspace.package].
edit("Cargo.toml", (s) =>
  s.replace(/(\[workspace\.package\][\s\S]*?\nversion = ")[^"]+(")/, `$1${version}$2`),
);
for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  edit(file, (s) => s.replace(/("version":\s*")[^"]+(")/, `$1${version}$2`));
}
// Keep Cargo.lock in step for the two workspace crates.
edit("Cargo.lock", (s) =>
  s.replace(/(name = "sandbox-(?:core|watcher)"\nversion = ")[^"]+(")/g, `$1${version}$2`),
);

run("node", ["scripts/changelog.mjs", "--write", "--next", tag]);
// The pre-commit hook checks formatting; format the generated file first.
// Call the binary directly: `pnpm exec` may run a dependency check that
// reinstalls node_modules when the store differs.
run(process.platform === "win32" ? "node_modules\\.bin\\oxfmt.cmd" : "node_modules/.bin/oxfmt", [
  "CHANGELOG.md",
]);
console.log("updated CHANGELOG.md");

const files = [
  "Cargo.toml",
  "Cargo.lock",
  "package.json",
  "src-tauri/tauri.conf.json",
  "CHANGELOG.md",
];
run("git", ["add", ...files]);
try {
  run("git", ["commit", "-q", "-m", `chore(release): ${tag}`]);
} catch (error) {
  // Leave the tree as it was so the release can simply be rerun.
  run("git", ["restore", "--staged", "--worktree", "--", ...files]);
  console.error(`\ncommit failed, version bump rolled back: ${error.message}`);
  process.exit(1);
}
console.log(`committed chore(release): ${tag}`);

if (!flags.includes("--no-tag")) {
  run("git", ["tag", "-a", tag, "-m", `Sandbox Watcher ${version}`]);
  console.log(`tagged ${tag}`);
  console.log(`\nNext: git push origin main && git push origin ${tag}`);
} else {
  console.log("\nNext: git push origin main");
}
