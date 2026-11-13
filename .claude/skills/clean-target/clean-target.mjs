// Shrinks target/ to what the verify commands and `tauri dev` use.
// Run from anywhere: node .claude/skills/clean-target/clean-target.mjs
import { spawnSync } from "node:child_process";
import { existsSync, lstatSync, readdirSync, rmSync } from "node:fs";
import { basename, dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const target = join(root, "target");
const debug = join(target, "debug");
const deps = join(debug, "deps");
const build = join(debug, "build");

if (!existsSync(target)) {
  console.log("target/ does not exist, nothing to clean");
  process.exit(0);
}

// The clippy flags match scripts/verify.mjs: clippy arguments are part of cargo's
// fingerprint, so other flags would keep a different set of artifacts.
const commands = [
  ["clippy", "--workspace", "--all-targets", "--locked", "--message-format=json", "--", "-D", "warnings"],
  ["test", "--workspace", "--locked", "--no-run", "--message-format=json"],
  ["build", "--workspace", "--locked", "--message-format=json"],
];

// rlib, rmeta, d, pdb, exe and dll of one unit share the stem `name-hash`.
const stem = (file) => basename(file).replace(/^lib/, "").split(".")[0];
const keepStems = new Set();
const keepBuildDirs = new Set();
const noteBuildDir = (path) => {
  const rel = relative(build, path);
  if (rel && !rel.startsWith("..")) keepBuildDirs.add(rel.split(sep)[0]);
};

// A failed or interrupted command leaves the keep lists short, and pruning against a
// short list would delete artifacts that are still in use.
let complete = true;
for (const args of commands) {
  const run = spawnSync("cargo", args, { cwd: root, maxBuffer: 1 << 30, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  let finished = false;
  for (const line of (run.stdout ?? "").split("\n")) {
    if (!line.startsWith("{")) continue;
    const msg = JSON.parse(line);
    if (msg.reason === "compiler-artifact") {
      for (const file of msg.filenames) {
        keepStems.add(stem(file));
        noteBuildDir(file);
      }
    } else if (msg.reason === "build-script-executed" && msg.out_dir) {
      noteBuildDir(msg.out_dir);
    } else if (msg.reason === "build-finished") {
      finished = msg.success;
    }
  }
  // Clippy exits non-zero on a lint while every artifact is still built and reported.
  if (!finished && !(args[0] === "clippy" && run.status === 1 && keepStems.size > 0)) {
    complete = false;
    console.log(`cargo ${args[0]} did not finish`);
  }
}

let freed = 0;
let removed = 0;
let locked = 0;
const sizeOf = (path) => {
  const st = lstatSync(path);
  if (!st.isDirectory()) return st.size;
  return readdirSync(path).reduce((sum, name) => sum + sizeOf(join(path, name)), 0);
};
const remove = (path) => {
  if (!existsSync(path)) return;
  const size = sizeOf(path);
  try {
    rmSync(path, { recursive: true, force: true });
    freed += size;
    removed += 1;
  } catch {
    // The running app or rust-analyzer can hold a file open on Windows.
    locked += 1;
  }
};

remove(join(target, "release"));
remove(join(debug, "incremental"));

if (complete && keepStems.size > 0) {
  if (existsSync(deps)) {
    for (const name of readdirSync(deps)) {
      if (!keepStems.has(stem(name))) remove(join(deps, name));
    }
  }
  if (existsSync(build)) {
    for (const name of readdirSync(build)) {
      if (!keepBuildDirs.has(name)) remove(join(build, name));
    }
  }
} else {
  console.log("skipped deps/ and build/: a cargo command failed, so the list of artifacts in use is incomplete");
}

const gb = (bytes) => (bytes / 2 ** 30).toFixed(2);
console.log(`removed ${removed} items, freed ${gb(freed)} GB${locked ? `, ${locked} locked and left in place` : ""}`);
console.log(`target/ is now ${gb(sizeOf(target))} GB`);
