#!/usr/bin/env node
// The release workflow's two steps around the build (docs/decisions/0028-updates.md).
//
//   node scripts/release.mjs check v0.2.0      the tag matches the app's version, and
//                                              docs/releases/0.2.0.md holds its notes
//   node scripts/release.mjs manifest v0.2.0   after `pnpm tauri build`: writes
//                                              target/release/latest.json and the notes
//                                              the GitHub release shows, and prints the
//                                              files to attach, one per line
//
// The notes may start with front matter. `critical: true` marks a critical fix: the
// app installs it before it is used again.
//
//   ---
//   critical: true
//   ---
//   What changed, in Markdown.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const REPOSITORY = "https://github.com/SoleBuilt/Kemable";

function fail(message) {
  console.error(`release: ${message}`);
  process.exit(1);
}

function readJson(path) {
  return JSON.parse(readFileSync(join(root, path), "utf8"));
}

/** The version every manifest names; they must agree, and the tag must be `v` and it. */
function version(tag) {
  const app = readJson("ui/windows/src-tauri/tauri.conf.json").version;
  const ui = readJson("ui/windows/package.json").version;
  const cargo = /^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/ms.exec(readFileSync(join(root, "Cargo.toml"), "utf8"))?.[1];
  if (app !== ui || app !== cargo) fail(`the versions differ: tauri.conf.json ${app}, package.json ${ui}, Cargo.toml ${cargo}`);
  if (tag !== `v${app}`) fail(`the tag ${tag} is not v${app}, the version in tauri.conf.json`);
  return app;
}

/** The release's notes, from docs/releases/<version>.md, and whether it is a critical fix. */
function notes(version) {
  const path = join(root, "docs", "releases", `${version}.md`);
  if (!existsSync(path)) fail(`docs/releases/${version}.md is missing: write what changed there`);
  const text = readFileSync(path, "utf8").replace(/\r\n/g, "\n");
  const front = /^---\n([\s\S]*?)\n---\n?/.exec(text);
  const body = (front ? text.slice(front[0].length) : text).trim();
  if (body === "") fail(`docs/releases/${version}.md says nothing about what changed`);
  const critical = front ? /^critical:\s*true\s*$/m.test(front[1]) : false;
  return { body, critical };
}

const [command, tag] = process.argv.slice(2);
if (!tag || (command !== "check" && command !== "manifest")) fail("usage: release.mjs check|manifest <tag>");
const app = version(tag);
const { body, critical } = notes(app);

if (command === "check") {
  console.log(`release: ${tag}${critical ? ", a critical fix" : ""}`);
  process.exit(0);
}

const bundle = join(root, "target", "release", "bundle", "nsis");
const installer = `Kemable_${app}_x64-setup.exe`;
const installerPath = join(bundle, installer);
const signaturePath = `${installerPath}.sig`;
if (!existsSync(installerPath)) fail(`${installerPath} is missing: did the build run?`);
if (!existsSync(signaturePath)) fail(`${signaturePath} is missing: the build needs TAURI_SIGNING_PRIVATE_KEY and createUpdaterArtifacts`);

const manifest = {
  version: app,
  notes: body,
  pub_date: new Date().toISOString(),
  critical,
  platforms: {
    "windows-x86_64": {
      signature: readFileSync(signaturePath, "utf8").trim(),
      url: `${REPOSITORY}/releases/download/${tag}/${installer}`,
    },
  },
};
const manifestPath = join(root, "target", "release", "latest.json");
const notesPath = join(root, "target", "release", "release-notes.md");
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
writeFileSync(notesPath, `${body}\n`);
console.log([installerPath, signaturePath, manifestPath].join("\n"));
