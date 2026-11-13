# Kemable for Windows

Tauri 2 over WebView2, with a Svelte 5 and TypeScript UI. The Rust side (`src-tauri/`) holds what is open and turns UI commands into core calls; the UI (`src/`) lays out, words and paints. The root `AGENTS.md` holds the rules every folder shares; `docs/decisions/0003-tauri-svelte-for-windows.md` has why this stack.

## Principles

1. **The core does the work.** No driver logic, SQL parsing or value formatting in TypeScript. The UI asks the core and shows the answer.
2. **Nothing heavy on the UI thread.** Every Tauri command is `async`, so it runs on the async runtime and never on the main thread. JavaScript never loops over a whole result.
3. **Bytes, not JSON, for rows.** Result rows reach the UI only as packed windows through `tauri::ipc::Response` (an `ArrayBuffer`). JSON carries metadata and events, never rows.
4. **Events are coalesced.** A run streams events on one `Channel`; row counts go out at most every 50 ms (`src-tauri/src/run.rs`).
5. **Draw, do not mount.** The data grid paints on a canvas over a native scroller. No DOM node per cell or per row (`.claude/rules/data-grid.md`).
6. **One design system.** Geist, black and white, the `pb-*` classes in `src/components.css` over the tokens in `src/app.css`, light and dark, keyboard first (`docs/decisions/0012-design-system.md`). No animation that delays input.

## Map

- `src/App.svelte`: the window: toolbar, sidebar panes, the pages (query tabs, table tabs and Live tabs, `docs/decisions/0013-pages-and-tabs.md`) with their tools on the tab bar, the editor, the results, the status bar. A page's state is swapped by `stash` and `unstash`.
- `src/lib/ui/`: the small parts every screen is built from: Button, Input, Checkbox, Field, Menu, MenuItem, Dialog and the rest (`docs/decisions/0014-ui-components.md` lists them and the rules). `Combobox.svelte` is every picker, `CommandPalette.svelte` is Ctrl+K, `SettingsDialog.svelte` holds the settings and where they are kept. Outside this folder, no markup copies what a part draws.
- `src/lib/shell/`: the window's frame, built from those parts. `TitleBar.svelte` and `WindowControls.svelte` (the window has no system title bar), `PageTabs.svelte`, `SidePane.svelte`, `StatusBar.svelte`, `PaletteTrigger.svelte`, `Welcome.svelte`.
- `src/lib/pages.ts`: each connection's query pages kept across launches (ADR 0013), with the `.sql` file a page is and the recent files (ADR 0016). `src/lib/editor/UnsavedDialog.svelte`: Save changes to <file>? as its page closes.
- `src/lib/live/`: the Live page (`docs/decisions/0020-live-tab.md`). `session.svelte.ts` is one page's listening and what it heard, kept by its page so it goes on while another page is shown; `LivePage.svelte` is the channels and the grid; `LiveTools.svelte` its tools on the page bar. `src-tauri/src/live.rs` runs a listening and counts its events to the UI; `cancel_run` stops it.
- `src/lib/data/`: Export, Import and Generate Rows (ADR 0018). `src-tauri/src/export.rs` and `src-tauri/src/jobs.rs` run them in the background, `cancel_run` stopping them.
- `src/lib/ipc.ts`: every command with its types. `src/lib/types.ts`: the Rust types as TypeScript. `src/lib/errors.ts`: the words for each error code.
- `src/lib/grid/`: `engine.ts` (the canvas grid), `window.ts` (the packed window reader), `DataGrid.svelte` (the thin Svelte wrapper), `FilterRow.svelte` (the filter conditions of a table tab or a query result), `Pager.svelte` (a table tab's pager, in the status bar; the page and count statements are the core's), `CellEditor.svelte` (the editor laid over a cell), `EditPreview.svelte` (the statements a save runs), `ResultBar.svelte`, `MessageList.svelte`, `WhereStrip.svelte`, `KeyBar.svelte`, `TableTools.svelte` (the bars and tools around the grid).
- `src/lib/editor/RunControls.svelte`: Run or Stop, the row limit and Save query, on the page bar.
- `src/lib/editor/SqlEditor.svelte`: CodeMirror 6 with the engine's SQL dialect, and the completion source that asks `complete` for names (`src-tauri/src/catalog.rs` caches the catalog it reads).
- `src/lib/sidebar/ObjectTree.svelte`: the tree: databases (for an engine with them), their schemas and objects, loaded when opened. Ctrl+R refreshes it. `TreeMenu.svelte`: its right-click menu. `HistoryList.svelte` (the History pane), `QueryList.svelte` (the Queries pane), `SaveQueryDialog.svelte` (Ctrl+Shift+D). `KeyBrowser.svelte`: a key-value server's keys by `SCAN`, in place of the tree.
- `src/lib/connect/`: `ConnectionMenu.svelte` (the title bar's connection), `PasswordDialog.svelte`, `ConnectDialog.svelte` (the connect form for PostgreSQL, SQL Server, Redis and Cloudflare D1, which also saves), `ProfilesDialog.svelte` (Credential profiles, from the palette or the form's profile picker, ADR 0015), `DatabasePicker.svelte` (the toolbar database switcher). Saved connections open from the command palette.
- `src/app.css`: Tailwind and the tokens. `src/components.css`: the design system's `pb-*` classes (`.claude/rules/ui-theme.md`).
- `src-tauri/src/lib.rs`: the driver registry, the setup and the command list. `data.rs`: the data folder, `data` beside the app or `%APPDATA%\<identifier>` (`kemable.app` installed, `dev.kemable.app` from `pnpm tauri dev`), and what the UI keeps in it (`docs/decisions/0019-data-folder.md`); `src/lib/store.ts` is its UI side. `window.rs`: the main window, made in setup so its WebView keeps its files in the data folder, and put back where it was closed. `commands.rs`: the commands; `conn` names a server, `database` one of its databases. `run.rs`: running a script and its events. `library.rs`: the saved-connection library in the data folder, with `CredentialVault` over Windows Credential Manager (targets `connection/<id>/password.Kemable` and, for a credential profile, `profile/<id>/password.Kemable`). `state.rs`: the open servers (`OpenServer`, a session per database in use), result sets and runs, by id.
- `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`: the window, the CSP and what the UI may call. `src-tauri/installer/`: the installer's NSIS hooks (the `data` folder; keeping it on uninstall).
- `assets/app-icon.png`: the source of `src-tauri/icons/` (`pnpm tauri icon assets/app-icon.png -o src-tauri/icons`).

## Commands

From `ui/windows/`:

```bash
pnpm install
pnpm tauri dev        # the app with hot reload, as dev.kemable.app
pnpm check            # svelte-check, no warnings allowed
pnpm build            # the UI bundle into dist/
pnpm tauri build      # the release app and its NSIS installer, as kemable.app
```

The Rust side builds with the workspace from the root (`cargo clippy --workspace`). It embeds `dist/`, so run `pnpm build` once before a Rust-only build of this crate.

`pnpm tauri` is `scripts/tauri.mjs`: the Tauri CLI, with `src-tauri/tauri.dev.conf.json` laid over the config for `dev`, so a development build and an installed one keep apart data folders (ADR 0019).

## Renaming the app

The name and the identifier reach past the code: the data folder, the installer and what Windows keeps. A rename changes all of these in the same change:

- `src-tauri/tauri.conf.json`: `productName`, the window's `title`, `identifier` (the installed app's: `%APPDATA%\<identifier>`, the uninstaller's "Delete the application data", Windows' records); `src-tauri/tauri.dev.conf.json`: the development `identifier` (`dev.` and the installed one); `index.html`: `<title>`.
- `src-tauri/installer/hooks.nsh`, and the terms' sentence in `src/lib/shell/TermsDialog.svelte`.
- `src-tauri/src/library.rs`: `SERVICE`, the Credential Manager target's suffix (`connection/<id>/password.<SERVICE>`); a new suffix cannot read the passwords kept under the old one, so read the old one once and keep it under the new.
- A new identifier is a new `%APPDATA%` folder: carry the old one's files over once (`DataFolder::carry_over` in `src-tauri/src/data.rs` takes the folder to copy from).
- The docs that name the folders: `docs/decisions/0019-data-folder.md`, this file, `docs/testing/`.

The rename from Paboable to Kemable changed all of these but carried nothing over, since only its author had used the app (`docs/decisions/0022-rename-to-kemable.md`). A rename once others use the app carries their data and passwords over.

## Adding a command

1. Write it in `src-tauri/src/commands.rs` as `async`, returning `Result<T, DriverError>`.
2. List it in `generate_handler!` in `src-tauri/src/lib.rs`.
3. Add it to `src/lib/ipc.ts` with its TypeScript types, and any new type to `src/lib/types.ts`.
4. If it needs a plugin permission, add it to `src-tauri/capabilities/default.json`, and nothing broader.
