// Version = 0.2.<commits since 0.2.0>. Run by .githooks/pre-commit: every commit carries its own version,
// written into the 5 files that hold it. Idempotent. Pass --print to only show the version.
// shortcut: `git commit --amend` or a rebase leaves the count off by one, run `node scripts/bump-version.mjs` by hand.
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const count = Number(execSync("git rev-list --count HEAD", { encoding: "utf8" }).trim());
const BASE = 100; // the commit that is 0.2.0
const version = `0.2.${count + 1 - BASE}`;
if (process.argv.includes("--print")) {
  console.log(version);
  process.exit(0);
}

/** [file, regex with the version in group 2, how many matches to replace]. */
const FILES = [
  ["package.json", /("version":\s*")[^"]+/, 1],
  ["package-lock.json", /("version":\s*")[^"]+/g, 2], // the root and packages[""]
  ["src-tauri/tauri.conf.json", /("version":\s*")[^"]+/, 1],
  ["src-tauri/Cargo.toml", /(^version\s*=\s*")[^"]+/m, 1],
  ["src-tauri/Cargo.lock", /(name = "dynamic-notch"\r?\nversion = ")[^"]+/, 1],
];

const changed = [];
for (const [file, re, n] of FILES) {
  const text = readFileSync(file, "utf8");
  let seen = 0;
  const next = text.replace(re, (_, head) => (++seen <= n ? head + version : _));
  if (next !== text) {
    writeFileSync(file, next);
    changed.push(file);
  }
}
console.log(`version ${version}${changed.length ? `: ${changed.join(", ")}` : " (already set)"}`);
if (process.env.BUMP_STAGE) execSync(`git add ${FILES.map(([f]) => f).join(" ")}`);
