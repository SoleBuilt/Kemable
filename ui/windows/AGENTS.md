# Paboable for Windows

Tauri 2 over WebView2, with a Svelte 5 and TypeScript UI. The Rust side (`src-tauri/`) holds what is open and turns UI commands into core calls; the UI (`src/`) lays out, words and paints. The root `AGENTS.md` holds the rules every folder shares; `docs/decisions/0003-tauri-svelte-for-windows.md` has why this stack.

## Principles

1. **The core does the work.** No driver logic, SQL parsing or value formatting in TypeScript. The UI asks the core and shows the answer.
2. **Nothing heavy on the UI thread.** Every Tauri command is `async`, so it runs on the async runtime and never on the main thread. JavaScript never loops over a whole result.
3. **Bytes, not JSON, for rows.** Result rows reach the UI only as packed windows through `tauri::ipc::Response` (an `ArrayBuffer`). JSON carries metadata and events, never rows.
4. **Events are coalesced.** A run streams events on one `Channel`; row counts go out at most every 50 ms (`src-tauri/src/run.rs`).
5. **Draw, do not mount.** The data grid paints on a canvas over a native scroller. No DOM node per cell or per row (`.claude/rules/data-grid.md`).
6. **Small and native-feeling.** Segoe UI, compact density, the system's light or dark scheme, keyboard first. No animation that delays input.

## Map

- `src/App.svelte`: the window: toolbar, sidebar, editor, results, status bar.
- `src/lib/ipc.ts`: every command with its types. `src/lib/types.ts`: the Rust types as TypeScript. `src/lib/errors.ts`: the words for each error code.
- `src/lib/grid/`: `engine.ts` (the canvas grid), `window.ts` (the packed window reader), `DataGrid.svelte` (the thin Svelte wrapper).
- `src/lib/editor/SqlEditor.svelte`: CodeMirror 6 with the engine's SQL dialect.
- `src/lib/sidebar/ObjectTree.svelte`: schemas and their objects.
- `src/app.css`: Tailwind and the colour tokens (`.claude/rules/ui-theme.md`).
- `src-tauri/src/lib.rs`: the driver registry and the command list. `commands.rs`: the commands. `run.rs`: running a script and its events. `state.rs`: the open connections, result sets and runs, by id.
- `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`: the window, the CSP and what the UI may call.
- `assets/app-icon.png`: the source of `src-tauri/icons/` (`pnpm tauri icon assets/app-icon.png -o src-tauri/icons`).

## Commands

From `ui/windows/`:

```bash
pnpm install
pnpm tauri dev        # the app with hot reload
pnpm check            # svelte-check, no warnings allowed
pnpm build            # the UI bundle into dist/
pnpm tauri build      # the release app and its NSIS installer
```

The Rust side builds with the workspace from the root (`cargo clippy --workspace`). It embeds `dist/`, so run `pnpm build` once before a Rust-only build of this crate.

## Adding a command

1. Write it in `src-tauri/src/commands.rs` as `async`, returning `Result<T, DriverError>`.
2. List it in `generate_handler!` in `src-tauri/src/lib.rs`.
3. Add it to `src/lib/ipc.ts` with its TypeScript types, and any new type to `src/lib/types.ts`.
4. If it needs a plugin permission, add it to `src-tauri/capabilities/default.json`, and nothing broader.
