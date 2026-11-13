# Kemable

A fast, light database client. One Rust core does all the work; each operating system gets its own UI under `ui/`. Windows comes first (Tauri 2, Svelte 5, TypeScript); `ui/apple` and `ui/linux` come later.

Read the guide of the folder you work in before editing it: `core/AGENTS.md` or `ui/windows/AGENTS.md`. Claude Code loads each through the folder's `CLAUDE.md`.

## Repository map

- `core/`: the Rust crates with no UI. `core/ARCHITECTURE.md` has the crates, the graph and the driver contract.
- `ui/windows/`: the Windows app. `src/` is the Svelte UI, `src-tauri/` the Rust crate that turns UI commands into core calls.
- `ui/apple/`, `ui/linux/`: not started. Each will own its UI and call the same core.
- `docs/decisions/`: the decisions (ADRs) that hold for the whole repo. `docs/roadmap.md`: the phases and the engines. `docs/testing/`: the manual checks for each phase, in Vietnamese for the person who runs them; a feature that adds something only a person can check adds its steps there.
- `scripts/verify.mjs`: the one check every change passes (`node scripts/verify.mjs` from any shell; `scripts/verify.sh` calls it from bash).
- `.claude/rules/`: path-scoped rules, loaded when a matching file is read. `.claude/skills/`: `verify`, `add-driver`, `fix-issue`, `clean-target`.
- `Cargo.toml` at the top is the workspace for every Rust crate, the Tauri app's included.

## Principles

1. **Fast is the feature.** Every change keeps startup, scrolling and query streaming smooth. Heavy work runs in Rust, off the UI thread. The UI paints what is on screen and nothing more. A change that risks speed says how it was measured.
2. **The core does the work, the UI words and paints.** Drivers, sessions, result sets, SQL splitting, value formatting and error codes live in `core/`. A UI never re-implements them; what is missing is added to the core with its tests.
3. **Tests define behavior.** When a test fails, fix the code, never the test. A test that pins behavior a change alters on purpose is updated in the same change and named in its description.
4. **Errors carry a code, not a sentence.** The core returns `DriverError { code, message, ... }`; each UI words the code (`ui/windows/src/lib/errors.ts`).
5. **Decisions are written down.** A choice that shapes more than one file gets an ADR in `docs/decisions/` (`docs/decisions/README.md`).
6. **One home per fact.** Other docs point to it by path and section instead of repeating it.

## Every change

- Run `node scripts/verify.mjs` from the repository root. It prints `PASS` or `FAIL`; only `PASS` is done. The `verify` skill explains each step.
- Rust: edition 2024, the workspace lints (no `unwrap` or `expect` outside tests, no `unsafe`), `cargo fmt`. Comments say why, never what.
- TypeScript: strict, `pnpm check` with no warnings. No `any`.
- A rename or signature change updates every caller and test in the same change.
- Writing: short sentences, plain words. Repo text is English; talk to the user in their language.

## Commits

- Commit only when the user asks. Subject `type(scope): description`, for example `feat(core-store): pack windows column by column`. Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `chore`. Scopes: `core`, `core-<crate>`, `driver-<engine>`, `windows`, `docs`, `repo`.
- Never commit generated output: `target/`, `node_modules/`, `dist/`, `ui/windows/src-tauri/gen/`.
