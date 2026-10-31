#!/usr/bin/env node
// Check whether the sandbox files the watcher depends on changed upstream.
//
// The watcher copies labels, paths, scripts and log formats from a specific
// sandbox release (see crates/sandbox-core/src/sandbox.rs). This script
// downloads a published @maibornwolff/sandbox package, hashes the files listed
// in scripts/sandbox-upstream.json and reports every one that differs from the
// recorded hashes. The package is only unpacked and hashed, never executed.
//
//   node scripts/check-sandbox-upstream.mjs              # compare against latest
//   node scripts/check-sandbox-upstream.mjs 0.74.0       # compare against a version
//   node scripts/check-sandbox-upstream.mjs --update     # record hashes of TESTED_SANDBOX_VERSION
//
// Exit codes: 0 no relevant change, 1 relevant files changed, 2 usage or setup error.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PACKAGE = "@maibornwolff/sandbox";
const MANIFEST = "scripts/sandbox-upstream.json";
const CONVENTIONS = "crates/sandbox-core/src/sandbox.rs";

const args = process.argv.slice(2);
const update = args.includes("--update");
const requested = args.find((a) => !a.startsWith("--"));
const inCi = process.env.GITHUB_ACTIONS === "true";

function run(cmd, cmdArgs) {
  try {
    return execFileSync(cmd, cmdArgs, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    }).trim();
  } catch {
    console.error(`failed: ${cmd} ${cmdArgs.join(" ")}`);
    process.exit(2);
  }
}

function isNewer(a, b) {
  const [x, y] = [a, b].map((v) => v.split("-")[0].split(".").map(Number));
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] > y[i];
  return false;
}

function testedVersion() {
  const match = readFileSync(CONVENTIONS, "utf8").match(/TESTED_SANDBOX_VERSION: &str = "([^"]+)"/);
  if (!match) throw new Error(`${CONVENTIONS}: TESTED_SANDBOX_VERSION not found`);
  return match[1];
}

// Unpack the published package into a fresh directory and return its root.
function fetchPackage(version, into) {
  const tarball = run("npm", [
    "pack",
    `${PACKAGE}@${version}`,
    "--loglevel=error",
    "--pack-destination",
    into,
  ]);
  run("tar", ["-xzf", join(into, tarball.split("\n").pop()), "-C", into]);
  return join(into, "package");
}

function filesUnder(root, path) {
  const full = join(root, path);
  let stat;
  try {
    stat = statSync(full);
  } catch {
    return [];
  }
  if (stat.isFile()) return [path];
  return readdirSync(full)
    .toSorted()
    .flatMap((entry) => filesUnder(root, `${path.replace(/\/$/, "")}/${entry}`));
}

function hashes(root, watch, { strict }) {
  const result = {};
  for (const path of watch) {
    const files = filesUnder(root, path);
    if (strict && files.length === 0) throw new Error(`${path}: not in the package`);
    for (const file of files) {
      result[file] = createHash("sha256")
        .update(readFileSync(join(root, file)))
        .digest("hex");
    }
  }
  return result;
}

const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
const tested = testedVersion();
const version = update ? tested : (requested ?? run("npm", ["view", PACKAGE, "version"]));

const work = mkdtempSync(join(tmpdir(), "sandbox-upstream-"));
try {
  const current = hashes(fetchPackage(version, work), manifest.watch, { strict: update });

  if (update) {
    writeFileSync(
      MANIFEST,
      `${JSON.stringify({ ...manifest, version, files: current }, null, 2)}\n`,
    );
    console.log(
      `recorded ${Object.keys(current).length} files of ${PACKAGE} ${version} in ${MANIFEST}`,
    );
    process.exit(0);
  }

  if (manifest.version !== tested) {
    console.error(
      `${MANIFEST} records ${manifest.version} but TESTED_SANDBOX_VERSION is ${tested}; run with --update`,
    );
    process.exit(2);
  }

  const recorded = manifest.files;
  const changed = Object.keys(current).filter((f) => f in recorded && recorded[f] !== current[f]);
  const added = Object.keys(current).filter((f) => !(f in recorded));
  const removed = Object.keys(recorded).filter((f) => !(f in current));

  console.log(`${PACKAGE}: tested ${tested}, compared ${version}`);
  if (changed.length + added.length + removed.length === 0) {
    if (isNewer(version, tested)) {
      const text = `sandbox ${version} leaves every watched file unchanged; bump TESTED_SANDBOX_VERSION and run --update`;
      console.log(inCi ? `::notice::${text}` : text);
    } else {
      console.log("no relevant changes");
    }
    process.exit(0);
  }

  for (const [label, list] of [
    ["changed", changed],
    ["added", added],
    ["removed", removed],
  ]) {
    for (const file of list) console.log(inCi ? `::error::${label}: ${file}` : `${label}: ${file}`);
  }
  console.log(
    `\nRe-check these against the watcher, then bump TESTED_SANDBOX_VERSION to ${version} and run --update.`,
  );
  process.exit(1);
} finally {
  rmSync(work, { recursive: true, force: true });
}
