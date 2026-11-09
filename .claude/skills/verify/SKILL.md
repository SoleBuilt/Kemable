---
name: verify
description: >-
  Check that Paboable builds, lints and passes its tests after a change, and read what failed.
  Use before saying any change is done, when the user asks to "verify", "check", "chạy test",
  "kiểm tra lại", or when a build, clippy, svelte-check or test failure needs explaining.
---

# Verify

Run from the repository root:

```bash
node scripts/verify.mjs            # everything, about a minute once warm
node scripts/verify.mjs --rust     # Rust only, when no UI file changed
node scripts/verify.mjs --ui       # UI only, when no Rust file changed
```

The last line is `PASS` or `FAIL: <steps>`. Only `PASS` means done. Report the verdict in one line, and for a failure the step, the first error and its file and line.

## The steps

| Step | Command | What it catches |
|---|---|---|
| ui install | `pnpm install --frozen-lockfile` in `ui/windows` | a `package.json` change without its lockfile |
| ui check | `pnpm check` (svelte-check, warnings fail) | type errors, Svelte misuse, a command whose types drifted from Rust |
| ui build | `pnpm build` | bundling errors; it also writes `dist/`, which the Tauri crate embeds |
| rust fmt | `cargo fmt --all -- --check` | formatting; fix with `cargo fmt --all` |
| rust clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | the workspace lints, `unwrap` outside tests |
| rust test | `cargo test --workspace --locked` | behavior |

## Reading a failure

- **Fix the code, not the test** (`AGENTS.md`, Principles). Change a test only when the behavior it pins changed on purpose, and say so.
- **`--locked` fails** when `Cargo.lock` needs an update: a dependency changed in a `Cargo.toml`. Run the same command without `--locked` once, check the lock diff is only what you meant, then rerun.
- **Clippy `unwrap_used`/`expect_used`** outside tests: return the error with `?` or handle the `None`.
- **A Tauri `generate_context!` error about `frontendDist`**: `ui/windows/dist` is missing; run `node scripts/verify.mjs --ui` first.
- **svelte-check `state_referenced_locally` or effect warnings**: read `.claude/rules/ui-windows.md`; usually an `$effect` that reads what it writes, or props read outside a closure.
- **A test that hangs**: a missing cancel or deadline. Rerun the one test with `cargo test -p <crate> <name> -- --nocapture`.

## Beyond the script

A change to the grid, the editor or a run's events also needs the app tried by hand: `pnpm tauri dev` in `ui/windows`. `.claude/rules/data-grid.md`, Checking a change, has the query to scroll through. Say in the report what was tried by hand and what was not.
