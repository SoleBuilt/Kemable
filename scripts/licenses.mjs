#!/usr/bin/env node
// The third-party notices shipped with the Windows app, and the license check of
// its npm packages (docs/decisions/0023-licensing.md).
//
//   node scripts/licenses.mjs           writes ui/windows/THIRD-PARTY-NOTICES.md
//   node scripts/licenses.mjs --check   fails when an npm package's license is not
//                                       allowed, or when the file is out of date
//
// The Rust crates come from cargo-about (about.toml, about.hbs); deny.toml checks
// their licenses. The npm packages come from `pnpm licenses list`, so run
// `pnpm install` in ui/windows first. Run it after every change to Cargo.lock or
// ui/windows/pnpm-lock.yaml, and commit what it writes.

import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const ui = join(root, "ui", "windows");
const output = join(ui, "THIRD-PARTY-NOTICES.md");
// Another version can write the same lock file differently, which --check reads as out of date.
const CARGO_ABOUT = "cargo-about 0.9.3";

// The npm packages' licenses: deny.toml's, and the fonts' OFL.
const NPM_ALLOWED = new Set([
  "MIT",
  "MIT-0",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "0BSD",
  "CC0-1.0",
  "Unlicense",
  "MPL-2.0",
  "OFL-1.1",
]);

// Kept in the source, not installed from npm: their license file lives beside them.
const VENDORED = [
  {
    name: "Lucide icons",
    use: "the icons' path data in ui/windows/src/lib/ui/Icon.svelte",
    license: "ISC, with MIT for the icons derived from Feather",
    file: join(ui, "src", "lib", "ui", "LUCIDE-LICENSE"),
  },
];

const check = process.argv[2] === "--check";
if (process.argv.length > 3 || (process.argv[2] !== undefined && !check)) {
  console.log("usage: node scripts/licenses.mjs [--check]");
  process.exit(2);
}

function run(command, cwd) {
  // A shell resolves pnpm.cmd on Windows and pnpm elsewhere.
  const result = spawnSync(command, { cwd, shell: true, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) {
    console.error(result.stderr || result.stdout);
    throw new Error(`failed: ${command}`);
  }
  return result.stdout;
}

/** True when an SPDX expression such as `MIT OR Apache-2.0` allows use under the list. */
function allowed(expression) {
  return expression
    .replace(/[()]/g, "")
    .split(/\s+OR\s+/)
    .some((choice) => choice.split(/\s+AND\s+/).every((license) => NPM_ALLOWED.has(license.trim())));
}

function rustNotices() {
  const version = run("cargo about --version", root).trim();
  if (version !== CARGO_ABOUT) {
    throw new Error(`needs ${CARGO_ABOUT}, found ${version}: cargo install --locked --features cli cargo-about@${CARGO_ABOUT.split(" ")[1]}`);
  }
  const manifest = join(ui, "src-tauri", "Cargo.toml");
  // cargo-about quotes each crate's own license file, so the sources must be on
  // disk; then it reads them offline, so the notices are the same on every machine.
  run("cargo fetch --locked", root);
  return run(`cargo about generate --frozen --fail --config about.toml --manifest-path "${manifest}" about.hbs`, root);
}

function npmNotices(problems) {
  const listed = JSON.parse(run("pnpm licenses list --prod --json", ui));
  const packages = Object.values(listed)
    .flat()
    .sort((a, b) => a.name.localeCompare(b.name));
  const sections = [];
  for (const pkg of packages) {
    if (!allowed(pkg.license)) problems.push(`npm package ${pkg.name} is ${pkg.license}, which deny.toml's list does not allow`);
    const folder = pkg.paths[0];
    const file = readdirSync(folder).find((name) => /^(licen[cs]e|copying|ofl)(\.|$)/i.test(name));
    if (file === undefined) problems.push(`npm package ${pkg.name} has no license file to quote`);
    const text = file === undefined ? "" : readFileSync(join(folder, file), "utf8").replace(/\r\n/g, "\n").trim();
    sections.push(`### ${pkg.name} ${pkg.versions.join(", ")}\n\nLicense: ${pkg.license}\n\n~~~~text\n${text}\n~~~~\n`);
  }
  return sections.join("\n");
}

function vendoredNotices() {
  return VENDORED.map(
    (item) =>
      `### ${item.name}\n\nUsed for ${item.use}. License: ${item.license}.\n\n~~~~text\n${readFileSync(item.file, "utf8").replace(/\r\n/g, "\n").trim()}\n~~~~\n`,
  ).join("\n");
}

const problems = [];
const notices = [
  "# Third-party notices",
  "",
  "Kemable is licensed under the GNU Affero General Public License, version 3 only",
  "(https://github.com/SoleBuilt/Kemable/blob/main/LICENSE). It includes the software",
  "below, each under its own license, quoted in full. Written by scripts/licenses.mjs;",
  "do not edit by hand.",
  "",
  "## Rust crates",
  "",
  rustNotices().replace(/\r\n/g, "\n").trim(),
  "",
  "## npm packages",
  "",
  npmNotices(problems).trim(),
  "",
  "## Kept in the source",
  "",
  vendoredNotices().trim(),
  "",
].join("\n");

for (const problem of problems) console.error(problem);
if (problems.length > 0) process.exit(1);

if (check) {
  const kept = existsSync(output) ? readFileSync(output, "utf8").replace(/\r\n/g, "\n") : "";
  if (kept !== notices) {
    console.error("ui/windows/THIRD-PARTY-NOTICES.md is out of date: run node scripts/licenses.mjs and commit it");
    process.exit(1);
  }
  console.log("licenses: ok");
} else {
  writeFileSync(output, notices);
  console.log(`wrote ${output}`);
}
