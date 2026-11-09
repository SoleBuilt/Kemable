#!/usr/bin/env node
// The check every change passes. The last line is PASS or FAIL.
// Node, so it runs the same from cmd, PowerShell and bash:
//
//   node scripts/verify.mjs          everything
//   node scripts/verify.mjs --rust   only the Rust steps (needs ui/windows/dist from an earlier build)
//   node scripts/verify.mjs --ui     only the UI steps
//
// The UI builds first: the Tauri crate embeds ui/windows/dist when it compiles.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const ui = join(root, "ui", "windows");

const flag = process.argv[2] ?? "";
if (!["", "--rust", "--ui"].includes(flag)) {
  console.log("usage: node scripts/verify.mjs [--rust | --ui]");
  process.exit(2);
}
const runUi = flag !== "--rust";
const runRust = flag !== "--ui";

const failed = [];

function step(name, command, cwd = root) {
  console.log(`==> ${name}`);
  // A shell resolves pnpm.cmd on Windows and pnpm elsewhere.
  const result = spawnSync(command, { cwd, stdio: "inherit", shell: true });
  if (result.status === 0) {
    console.log(`    ok: ${name}`);
  } else {
    console.log(`    FAILED: ${name}`);
    failed.push(name);
  }
}

if (runUi) {
  step("ui install", "pnpm install --frozen-lockfile", ui);
  step("ui check", "pnpm check", ui);
  step("ui build", "pnpm build", ui);
}

if (runRust) {
  if (!existsSync(join(ui, "dist", "index.html"))) {
    console.log("ui/windows/dist is missing: run node scripts/verify.mjs --ui first");
    failed.push("ui dist");
  } else {
    step("rust fmt", "cargo fmt --all -- --check");
    step("rust clippy", "cargo clippy --workspace --all-targets --locked -- -D warnings");
    step("rust test", "cargo test --workspace --locked");
  }
}

if (failed.length === 0) {
  console.log("PASS");
} else {
  console.log(`FAIL: ${failed.join(", ")}`);
  process.exit(1);
}
