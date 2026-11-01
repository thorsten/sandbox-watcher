#!/usr/bin/env node
// Every third-party GitHub Action in .github/workflows must be pinned to a
// full commit SHA with the release it stands for in a trailing comment:
//
//   - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
//
// Dependabot (.github/dependabot.yml) bumps the SHA and the comment together.
//
// Usage: node scripts/check-action-pins.mjs [--verify]
//   default   offline: every `uses:` is a 40-hex SHA with a version comment
//   --verify  also ask api.github.com whether the commented tag or branch
//             still resolves to the pinned SHA (needs network; GH_TOKEN
//             raises the rate limit)

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workflowsDir = join(root, ".github", "workflows");
const verify = process.argv.includes("--verify");

const USES = /^\s*-?\s*uses:\s*["']?([^\s"'#]+)["']?\s*(#\s*(.*))?$/;
const PINNED = /^([\w.-]+\/[\w.-]+)(\/[^@]+)?@([0-9a-f]{40})$/;

const pins = [];
const problems = [];
for (const file of readdirSync(workflowsDir)
  .filter((f) => /\.ya?ml$/.test(f))
  .toSorted()) {
  const lines = readFileSync(join(workflowsDir, file), "utf8").split("\n");
  lines.forEach((line, index) => {
    const match = USES.exec(line);
    if (!match) return;
    const where = `${file}:${index + 1}`;
    const uses = match[1];
    if (uses.startsWith("./") || uses.startsWith("docker://")) return;
    const pinned = PINNED.exec(uses);
    if (!pinned) {
      problems.push(`${where}: "${uses}" is not pinned to a full commit SHA`);
      return;
    }
    const comment = (match[3] ?? "").trim();
    if (!/^v?\d+(\.\d+)*(\.\d+)?$|^[\w.-]+$/.test(comment) || comment === "") {
      problems.push(`${where}: "${uses}" needs a trailing "# <tag or branch>" comment`);
      return;
    }
    pins.push({ where, repo: pinned[1], sha: pinned[3], ref: comment });
  });
}

if (problems.length > 0) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`${pins.length} action pins have a full SHA and a version comment`);

if (!verify) process.exit(0);

const headers = { "User-Agent": "sandbox-watcher-check-action-pins" };
if (process.env.GH_TOKEN) headers.Authorization = `Bearer ${process.env.GH_TOKEN}`;

async function resolveRef(repo, ref) {
  // Tags first, then branches (dtolnay/rust-toolchain@stable is a branch).
  for (const kind of ["tags", "heads"]) {
    const res = await fetch(`https://api.github.com/repos/${repo}/git/ref/${kind}/${ref}`, {
      headers,
    });
    if (res.status === 404) continue;
    if (!res.ok) throw new Error(`${repo} ${kind}/${ref}: HTTP ${res.status}`);
    const body = await res.json();
    if (body.object.type === "commit") return body.object.sha;
    // Annotated tag: dereference to the commit.
    const tag = await fetch(body.object.url, { headers });
    if (!tag.ok) throw new Error(`${repo} tag object: HTTP ${tag.status}`);
    return (await tag.json()).object.sha;
  }
  return null;
}

let mismatches = 0;
const seen = new Map();
for (const pin of pins) {
  const key = `${pin.repo}@${pin.ref}`;
  if (!seen.has(key)) seen.set(key, resolveRef(pin.repo, pin.ref));
  const actual = await seen.get(key);
  if (actual === null) {
    console.error(`${pin.where}: ${key} does not exist upstream`);
    mismatches += 1;
  } else if (actual !== pin.sha) {
    console.error(`${pin.where}: ${key} is ${actual}, pinned ${pin.sha}`);
    mismatches += 1;
  } else {
    console.log(`${pin.where}: ${key} = ${pin.sha}`);
  }
}
if (mismatches > 0) {
  console.error(`${mismatches} pins do not match upstream; re-pin or fix the comment`);
  process.exit(1);
}
console.log("all pins verified against GitHub");
