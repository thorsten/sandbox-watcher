#!/usr/bin/env node
// Generate a changelog from Conventional Commits. No dependencies.
//
//   node scripts/changelog.mjs                 # full CHANGELOG.md to stdout
//   node scripts/changelog.mjs --write         # rewrite CHANGELOG.md
//   node scripts/changelog.mjs --write --next v0.2.0   # unreleased commits under 0.2.0
//   node scripts/changelog.mjs --release v0.2.0   # notes for that tag only
//                                                 # (commits since the previous tag)
//   node scripts/changelog.mjs --unreleased    # notes for commits after the last tag
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

const REPO = "https://github.com/thorsten/sandbox-watcher";
const SECTIONS = [
  ["feat", "Features"],
  ["fix", "Fixes"],
  ["perf", "Performance"],
  ["refactor", "Refactoring"],
  ["docs", "Documentation"],
  ["test", "Tests"],
  ["build", "Build"],
  ["ci", "CI"],
  ["chore", "Chores"],
];
const HIDDEN = new Set(["style"]);

function git(args) {
  return execFileSync("git", args, { encoding: "utf8" }).trim();
}

function semverKey(tag) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?/.exec(tag);
  if (!m) return null;
  return [Number(m[1]), Number(m[2]), Number(m[3]), m[4] ?? "~"];
}

function compareTags(a, b) {
  const ka = semverKey(a);
  const kb = semverKey(b);
  for (let i = 0; i < 4; i++) {
    if (ka[i] === kb[i]) continue;
    return typeof ka[i] === "number" ? ka[i] - kb[i] : String(ka[i]).localeCompare(String(kb[i]));
  }
  return 0;
}

/** Version tags, oldest first. */
function tags() {
  const all = git(["tag", "--list", "v*"]).split("\n").filter(Boolean);
  return all.filter((t) => semverKey(t)).toSorted(compareTags);
}

function tagDate(tag) {
  return git(["log", "-1", "--format=%cs", tag]);
}

/** Commits in `range` as parsed conventional commits, newest first. */
function commits(range) {
  const out = git(["log", "--no-merges", "--format=%H%x1f%s%x1f%b%x1e", ...(range ? [range] : [])]);
  return out
    .split("\x1e")
    .map((s) => s.trim())
    .filter(Boolean)
    .map((entry) => {
      const [hash, subject, body = ""] = entry.split("\x1f");
      const m = /^(\w+)(?:\(([^)]*)\))?(!)?:\s*(.+)$/.exec(subject);
      const breaking = Boolean(m?.[3]) || /^BREAKING CHANGE:/m.test(body);
      return {
        hash,
        type: m ? m[1] : "other",
        scope: m?.[2] ?? null,
        breaking,
        description: m ? m[4] : subject,
      };
    })
    .filter((c) => !HIDDEN.has(c.type));
}

function renderCommits(list) {
  if (list.length === 0) return "_No notable changes._\n";
  const byType = new Map();
  for (const c of list) {
    const key = c.breaking ? "breaking" : c.type;
    if (!byType.has(key)) byType.set(key, []);
    byType.get(key).push(c);
  }
  let out = "";
  const order = [["breaking", "Breaking changes"], ...SECTIONS, ["other", "Other"]];
  const known = new Set(order.map(([k]) => k));
  for (const key of byType.keys()) if (!known.has(key)) order.push([key, key]);
  for (const [key, title] of order) {
    const items = byType.get(key);
    if (!items) continue;
    out += `### ${title}\n\n`;
    for (const c of items) {
      const scope = c.scope ? `**${c.scope}:** ` : "";
      const short = c.hash.slice(0, 7);
      out += `- ${scope}${c.description} ([${short}](${REPO}/commit/${c.hash}))\n`;
    }
    out += "\n";
  }
  return out;
}

function releaseNotes(tag, previous) {
  const range = previous ? `${previous}..${tag}` : tag;
  let out = renderCommits(commits(range));
  if (previous)
    out += `**Full changelog:** [${previous}...${tag}](${REPO}/compare/${previous}...${tag})\n`;
  return out;
}

function fullChangelog(nextVersion) {
  const list = tags();
  let out =
    "# Changelog\n\nAll notable changes to Sandbox Watcher. Generated from\n[Conventional Commits](https://www.conventionalcommits.org/) by `scripts/changelog.mjs`.\n\n";
  const last = list.at(-1);
  const unreleased = commits(last ? `${last}..HEAD` : undefined);
  if (nextVersion) {
    const today = new Date().toISOString().slice(0, 10);
    out += `## ${nextVersion.replace(/^v/, "")} (${today})\n\n` + renderCommits(unreleased) + "\n";
  } else if (unreleased.length > 0) {
    out += "## Unreleased\n\n" + renderCommits(unreleased);
  }
  for (let i = list.length - 1; i >= 0; i--) {
    const tag = list[i];
    const previous = list[i - 1];
    out += `## ${tag.replace(/^v/, "")} (${tagDate(tag)})\n\n` + releaseNotes(tag, previous) + "\n";
  }
  return out.trimEnd() + "\n";
}

const args = process.argv.slice(2);
const releaseIndex = args.indexOf("--release");
if (releaseIndex !== -1) {
  const tag = args[releaseIndex + 1];
  if (!tag) throw new Error("--release needs a tag");
  const list = tags();
  const previous = list.filter((t) => compareTags(t, tag) < 0).at(-1);
  process.stdout.write(releaseNotes(tag, previous));
} else if (args.includes("--unreleased")) {
  const last = tags().at(-1);
  process.stdout.write(renderCommits(commits(last ? `${last}..HEAD` : undefined)));
} else if (args.includes("--write")) {
  const nextIndex = args.indexOf("--next");
  const next = nextIndex === -1 ? undefined : args[nextIndex + 1];
  writeFileSync("CHANGELOG.md", fullChangelog(next));
  process.stderr.write("CHANGELOG.md written\n");
} else {
  process.stdout.write(fullChangelog());
}
