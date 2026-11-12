# 0019: One data folder, beside the app where it was installed; an installer that asks where

- **Status**: Accepted
- **Date**: 2026-11-12
- **Amends**: `docs/decisions/0013-pages-and-tabs.md` (query pages were kept in the WebView's storage). Decided with the user on 2026-11-12.

## Context

What the app remembered was in three places: the saved connections, profiles, queries and history in `%APPDATA%\<identifier>`; the query pages, settings, recent files and a few choices in the WebView's storage under `%LOCALAPPDATA%\<identifier>\EBWebView`; and the window's place beside the first. The user wants it all in one place, and that place where the app was installed: installed on D:, its data on D:. It must stay as fast as it is.

## Decision

- **The data folder** is `data` beside `Paboable.exe` when it is there and this user can write it, else `%APPDATA%\<identifier>`: a development build, or an exe copied without its folder (`ui/windows/src-tauri/src/data.rs`, `DataFolder::find`). The installer makes `data`. Copying the install folder elsewhere takes everything with it. Settings › Data shows the folder and opens it.
- **What is in it:** `connections.json`, `profiles.json`, `queries.json`, `history.db` as before; `window-state.json`, the window's place; `ui.json`, the UI's preferences (settings, recent files, the page size, the tree's scope, the sidebar's width, the editor's height); `pages/<hash>.json`, one connection's query pages each, the 40 written last kept; `webview/`, the WebView's own files. Passwords stay in Windows Credential Manager.
- **As fast as before.** The window is made in `setup` (`window.rs`) with an initialization script that puts `ui.json` and the pages of files opened with no connection on `window.__PABOABLE__` before the UI's first line runs, so they read at once, as the WebView's storage did, and the theme is right on the first paint. A connection's pages are read when it opens, one small file. A change is held in the UI and written a moment later (preferences 300 ms, pages 400 ms after typing stops) on a blocking thread, through a temporary file renamed over the old one, so a crash never leaves half a file. Closing the window, however it closes, waits for what is not written (`onCloseRequested`, `flushStore`).
- **The window's place** is the app's own (`window.rs`), not `tauri-plugin-window-state`, which writes into `%APPDATA%` whatever its file name. The file keeps the plugin's field names, so the place an earlier version kept still reads. A place on a screen no longer there is left to Windows; a maximized window comes back maximized and restores to where it was.
- **Moving from an older version**, once: a new `data` folder takes a copy of the files in `%APPDATA%\<identifier>` (`carry_over`); the first launch without `ui.json` runs on the old WebView folder, and the UI moves the old storage's keys into files (`import_local_storage`), `ui.json` last, as the sign it is done. The next launch uses `data\webview`. The old folders are left as they were, a backup to delete by hand.
- **The terms** are asked in the app, once a version of them (`TERMS_VERSION` in `src/lib/shell/TermsDialog.svelte`), as CapCut asks: one sentence with the two links (opened in the browser by `open_terms`, which knows only those two addresses), **Agree and continue** or **Disagree and close**; no Esc, no close button. The app keeps which terms were agreed and when (`termsAccepted`, `termsAcceptedAt` in `ui.json`), on this computer only: there is no account and nothing is sent. The installer has no terms page of its own (decided with the user on 2026-11-12, the app's dialog being enough). Until Paboable has its own site, the links are CapCut's pages, to be rewritten for this app (decided with the user on 2026-11-12).
- **The installer** (Tauri's NSIS, `ui/windows/src-tauri/installer/`) asks Just me or Everyone, then where to install; everything goes there. The installer has no terms page: the app asks (above). The last page says it is done and offers to start the app and make a desktop shortcut. Installed for Everyone, `data` lets the PC's users change it, as the app runs without admin rights. Uninstalling keeps `data`, so a reinstall finds it, unless "Delete the application data" is ticked; an update never deletes it.

## Consequences

- Installed for Everyone, the PC's users share one data folder: the same saved connections and pages. Their passwords stay each user's own, in Credential Manager.
- A folder this user cannot write, such as an old install in Program Files, falls back to `%APPDATA%`, and Settings says so.
- The links point at CapCut's terms until Paboable's landing page has its own; they are named once, in `open_terms` (`commands.rs`).
- The installed app is `paboable.app`; `pnpm tauri dev` is `dev.paboable.app` (`src-tauri/tauri.dev.conf.json`, laid over by `scripts/tauri.mjs`) and keeps its data in `%APPDATA%\dev.paboable.app`, so uninstalling with "Delete the application data" never touches a development build's data, and a fresh install starts empty. Decided with the user on 2026-11-12.

## Alternatives considered

- **One `session.json` with everything.** Every keystroke would rewrite every connection's pages, and the launch would read them all.
- **The WebView's storage, moved with the WebView's folder.** One place, but only the UI can read it, and a moved storage cannot be read by the next version's Rust side.
- **Keeping `tauri-plugin-window-state`.** It always writes into `%APPDATA%\<identifier>`, a second place.
