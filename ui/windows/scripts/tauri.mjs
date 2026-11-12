// `pnpm tauri <command>`: the Tauri CLI, with `src-tauri/tauri.dev.conf.json` laid
// over `tauri.conf.json` for `dev`. A development build is `dev.paboable.app`, an
// installed one `paboable.app`, so their data folders and Windows' records of them
// never mix (docs/decisions/0019-data-folder.md).
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
if (args[0] === "dev" && !args.some((arg) => arg === "--config" || arg === "-c" || arg.startsWith("--config="))) {
  args.splice(1, 0, "--config", fileURLToPath(new URL("../src-tauri/tauri.dev.conf.json", import.meta.url)));
}
const cli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
const result = spawnSync(process.execPath, [cli, ...args], { stdio: "inherit" });
process.exit(result.status ?? 1);
