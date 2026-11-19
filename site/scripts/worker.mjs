// `pnpm worker`: the Worker on this PC (http://127.0.0.1:8787) with the built pages and a local D1,
// for trying /api/* (the waitlist, the visit counts). `pnpm dev` sends /api/* here (astro.config.mjs),
// so the two side by side give hot reload and a working API. site/README.md, On this PC.
import { spawnSync } from "node:child_process";

const WRANGLER = "wrangler@4.156.0";

function run(command, env = {}) {
  // A shell resolves pnpm.cmd and npx.cmd on Windows and pnpm and npx elsewhere.
  const result = spawnSync(command, { stdio: "inherit", shell: true, env: { ...process.env, ...env } });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

// The pages the Worker serves, as they are now.
run("pnpm build");
// The local database's tables; one already there is skipped. CI=1 answers Wrangler's "continue?" with yes.
run(`npx --yes ${WRANGLER} d1 migrations apply kemable-site --local`, { CI: "1" });
run(`npx --yes ${WRANGLER} dev --port 8787`);
