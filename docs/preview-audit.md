# Before the first preview: audit of 2026-11-15

What a read of the code found that could hurt a person using Kemable, before its first public preview. Each item says what happens, where in the code, how it could be fixed, and what is decided. An item is fixed through the `fix-issue` skill, ticked here, and moved to the ADR or roadmap entry that owns its subject.

Groups: **A** loses or corrupts data, or crashes; **B** hurts everyday use; **C** release and legal; **D** small, after the preview; **E** found by running `docs/testing/` on the installed build, 2026-11-21.

The paths name a function or a place; line numbers move, so search for the name.

Fixed 2026-11-15 on `task/2026-11-15`, one branch an item: every item but C19. Each fix's steps for a person to check are in `docs/testing/preview.md`, under the item's code.

## A. Loses or corrupts data, or crashes

- [x] **A1. SQL Server: `USE x` leaves the first database's session in `x`.** Connect with `Sales`, run `USE Sales_Test`, then pick `Sales` again, open `Sales > dbo.Orders`, edit it or truncate it: the statements run in `Sales_Test`, while the tree, read on the catalog session, still shows `Sales`. A `USE` that is not the batch's first word moves the session with nothing in the UI changing.
  - Where: `ui/windows/src-tauri/src/state.rs` (`OpenServer::session`, sessions kept by database name); `run.rs` (`statement_in`, `follow`: the run moves to the new database's session, the old session is never put back); `core/crates/drivers/mssql/src/connection.rs` (`execute`, no reset; the database is read once at connect); `core/crates/core/src/sql/database_change.rs` (only a leading `USE`).
  - Fix: a SQL Server session knows its database and puts it back (`USE <its own>`) after any batch that may have changed it, or reads `db_name()` after each batch; the run follows the change through the database's own session.
- [x] **A2. CSV and JSON export drop every binary value.** A `bytea`, `varbinary` or `blob` column is exported empty (CSV) or `""` (JSON), and the export reports success. In CSV an empty field cannot be told from NULL.
  - Where: `core/crates/core/src/format.rs` (`write_display` writes nothing for `Value::Bytes`), used by `ui/windows/src-tauri/src/export.rs` (`text`, `csv_row`, `json_row`). Only the INSERT format writes bytes (`value_literal`). No export test has bytes.
  - Fix: CSV and JSON write bytes as hex (`\x...` or `0x...`, said in the export dialog), or base64 for JSON; a test per format.
- [x] **A3. Saving edits, Import and Generate Rows end the user's own open transaction (PostgreSQL, SQL Server).** In the editor `BEGIN; UPDATE ...` left open; then saving a table edit in the same database: on PostgreSQL the edit's `BEGIN` only warns and its `COMMIT` commits the user's pending work, or its `ROLLBACK` (on `edit.row_gone`) throws it away; on SQL Server the CATCH's `ROLLBACK TRANSACTION` rolls back to the outermost transaction. Import and Generate Rows do the same. SQLite refuses (`BEGIN IMMEDIATE` fails first).
  - Where: one session per database for the editor, table tabs, edits and jobs (`state.rs`); `core/crates/drivers/postgres/src/connection.rs` (`apply_edits`); `core/crates/drivers/mssql/src/connection.rs` (`edit_batch`); `ui/windows/src-tauri/src/jobs.rs` with `core/crates/core/src/import.rs`.
  - Fix: inside an open transaction, edits and jobs run under a savepoint and never commit (PostgreSQL knows the status: the row cap's probe, ADR 0004; SQL Server reads `@@TRANCOUNT` and uses `SAVE TRANSACTION`); or they refuse with a clear error while a transaction is open.
- [x] **A4. Editing a binary cell writes the hex text as the bytes.** The cell editor starts from `0x00ff`; saving `0x00fe` stores six ASCII bytes on PostgreSQL and TEXT in a SQLite blob column. SQL Server refuses (no implicit conversion), which is safe.
  - Where: `core/crates/store/src/store.rs` (`cell_text` gives hex); `ui/windows/src/App.svelte` (the editor's text, sent as `NewValue::Text`); `core/crates/core/src/sql/literal.rs` (`typed` quotes it).
  - Fix: a binary column's edit is a bytes value (`NewValue::Bytes` from hex, written as `'\x..'::bytea`, `X'..'`, `0x..`), or binary cells are read only until then.
- [x] **A5. SQL Server: `datetime` literals are read by the login's date format.** Dates are written as `'YYYY-MM-DD hh:mm:ss'` with no `DATEFORMAT` or `LANGUAGE`; with a dmy login (British, French, German...) `2024-01-02 10:00` is 1 February, with no error for days up to 12. Reaches edits (and a datetime key in an edit's `WHERE`, which can then match another row), Import, and INSERT exports.
  - Where: `core/crates/core/src/format.rs` (timestamps), `core/crates/core/src/sql/literal.rs`, `core/crates/drivers/mssql/src/decode.rs` (`datetime` as `Timestamp`).
  - Fix: SQL Server literals in the unambiguous ISO 8601 form `'YYYY-MM-DDThh:mm:ss.fff'` (read the same under every language for `datetime` and `datetime2`), or `CONVERT(datetime2, '...', 126)`; a test under `SET LANGUAGE British`.
- [x] **A6. SQLite: a typed value that looks like a number goes in bare.** Into a TEXT column, `1.10` is stored as `1.1`, `10.00` as `10.0`, a 20-digit code as `1.23456789012346e+19`.
  - Where: `core/crates/core/src/sql/literal.rs` (`typed`, `is_number`).
  - Fix: the literal follows the column's declared type (quoted for TEXT affinity, and for any number SQLite cannot hold exactly); a test per affinity.
- [x] **A7. Closing the window drops unsaved table edits and stops running work without asking.** Closing a page, disconnecting and switching connections all refuse while edits wait, but the window's close (X, Alt+F4, the taskbar) only keeps the query pages. A running query and listening Live tabs stop without a word. Unsaved `.sql` pages are safe (kept as dirty).
  - Where: `ui/windows/src/App.svelte` (`onCloseRequested` keeps pages and flushes, never asks); the same guards exist for switching and disconnecting, and the update's list of what it would stop (ADR 0028).
  - Fix: on close, when edits wait, a run goes on or a Live tab listens, `preventDefault` and ask, with the list the update already builds.
- [x] **A8. Fetch All and "No limit" have no memory bound.** On a big table, rows arrive until the process runs out of memory and aborts. Firebase's Realtime Database with no limit reads a whole subtree as one JSON value first.
  - Where: `core/crates/store/src/store.rs` (`push_rows` checks only `row_limit`); `ui/windows/src/App.svelte` (Fetch all, Run Without Limit send `rowLimit: null`); `ui/windows/src/lib/ui/SettingsDialog.svelte` (`ROW_CAPS` holds `null`); `core/crates/drivers/firebase/src/lib.rs`.
  - Decided 2026-11-15: a result stops at about 25% of the machine's memory, and says "Stopped at N MB", as it says it stopped at the row cap.
- [x] **A9. SQL Server: Stop resets the session.** tiberius 0.13 reads the attention's acknowledgement only from the response in flight; when the server had already sent the whole response, the acknowledgement comes in the next message, tiberius reports "Never got a DONE token acknowledging the Attention signal", and the driver drops the session and opens another: temp tables, `SET` options, `USE` and the open transaction are gone, with nothing said. Measured 2026-11-14 (ADR 0004, The row cap, where the row cap no longer sends one).
  - Where: `core/crates/drivers/mssql/src/connection.rs` (`attention`); tiberius `src/tds/stream/token.rs` (`flush_done_attention`).
  - Decided 2026-11-15: tiberius 0.13.0 is forked into the workspace as our own crate, `tabstream`, which Kemable maintains: its MIT and Apache-2.0 license files and copyright lines stay, each changed file says so, and `THIRD-PARTY-NOTICES.md` lists it. Its attention reads on into the following message until the acknowledgement, and it tells its caller whether a transaction is open. Upstream fixes are taken one by one when they matter (security, protocol), never merged whole. The fork gets an ADR.

## B. Hurts everyday use

- [x] **B10. No reconnect after sleep, a VPN drop or a server restart.** Every action says "The connection was lost" until the user disconnects and connects again. Only SQL Server (when the error maps to `driver.lost`) and Redis reopen their link.
  - Where: `ui/windows/src-tauri/src/state.rs` (`session` returns the cached session with no check; nothing removes a dead one; `Connection::ping` is never called); the PostgreSQL clients are fixed for the session's life; `core/crates/tunnel/src/lib.rs` (the listener stops with the SSH session and is not reopened; SQL Server and Redis then dial a dead local port); `core/crates/drivers/rabbitmq/src/lib.rs` (one connection and channel).
  - Fix: a session that answers `driver.lost` is dropped from the map and the call is tried once on a new one (never for a statement that may have run); the tunnel reopens with its server.
- [x] **B11. SQLite: completion or the tree stops the user's running query.** While a query runs longer than 1.5 s, a completion lookup or a tree call waits for the connection's lock, and its deadline fires the connection's one `InterruptHandle`, which stops the user's query, reported as `driver.timeout`.
  - Where: `core/crates/drivers/sqlite/src/lib.rs` (`blocking`: the watcher interrupts at its own deadline even while its work waits for the lock); `ui/windows/src-tauri/src/catalog.rs` (`LOOKUP_TIMEOUT`), `commands.rs` (`METADATA_TIMEOUT`).
  - Fix: interrupt only when the call's own work holds the connection (set a flag once the lock is taken); a call still waiting gives up without interrupting; or a second read-only connection for the catalog.
- [x] **B12. Opening another connection while a run goes on mixes the two.** The old run's sets, errors and "finished" land on the new connection's page; on SQL Server, closing waits for the editor lock the query holds, so the new connection sits in "Running".
  - Where: `ui/windows/src/App.svelte` (`openWith` has no `running` check, unlike `disconnect`; `onEvent` applies the old run's events); `ui/windows/src-tauri/src/commands.rs` (the run's cancel token is not tied to `server.closing`, as `start_live`'s is); `core/crates/drivers/mssql/src/connection.rs` (`close` waits for the editor lock).
  - Fix: `openWith` asks to stop the run first, as `disconnect` does; a run's token is a child of its server's `closing`; events carry their run id and stale ones are dropped.
- [x] **B13. When the app cannot start, it closes without a word.** Errors in setup (the WebView, the data folder) end in `eprintln!` and `exit(1)`, and a release build has no console. Seen when WebView2 is missing or blocked by policy; suspected when a second Windows user opens an install for everyone while the first has it open (one `data\webview` folder for both accounts).
  - Where: `ui/windows/src-tauri/src/lib.rs` (`run`), `window.rs`.
  - Fix: a native message box with the error before exiting, and a WebView folder per Windows user.
- [x] **B14. A corrupt `connections.json` hides every saved connection with no message.** The read fails on bad JSON; both callers turn the error into an empty list, so the palette is empty; the user learns only when a save fails.
  - Where: `core/crates/storage/src/lib.rs` (read); `ui/windows/src/App.svelte` and `src/lib/connect/ConnectDialog.svelte` (the error becomes `[]`).
  - Fix: show the error with the file's path and a way to open the folder; saves keep refusing to write over it.
- [x] **B15. An install for everyone lets one Windows user take another's saved password.** `data\` is writable by every user; each user's Credential Manager password is keyed by the connection id only; so user A changes the host of a connection user B saved, and B's next connect sends B's password to A's server. The shared `known_hosts` lets A plant SSH host keys B's tunnels then trust.
  - Where: `ui/windows/src-tauri/installer/hooks.nsh` (BUILTIN\Users get Modify); `core/crates/storage/src/lib.rs` (`password_key`, `resolve`); `ui/windows/src-tauri/src/lib.rs` (one `known_hosts`).
  - Decided 2026-11-15: a kept password is bound to the host, the port and the user it was saved for; when any of them changes, the app asks for the password again and never sends the kept one. `known_hosts` moves to each Windows user's own folder.

## C. Release and legal

- [x] **C16. The installer does not carry the AGPL text.** Only `THIRD-PARTY-NOTICES.md` ships; Settings › About › License opens a GitHub URL, which also needs the repository public, with the release's tag, on release day.
  - Where: `ui/windows/src-tauri/tauri.conf.json` (`bundle.resources`); `ui/windows/src-tauri/src/commands.rs` (`open_about`).
  - Fix: ship `LICENSE` as a resource and open it from the disk, as the notices are.
- [x] **C17. The Terms and Privacy links are stand-ins for another product's pages.** The terms dialog is hidden, but `open_terms` still holds the stand-in URLs and is registered; `TermsDialog.svelte` and ADRs 0019 and 0023 name that product.
  - Where: `ui/windows/src-tauri/src/commands.rs` (`open_terms`), `ui/windows/src/lib/shell/TermsDialog.svelte`, `docs/decisions/0019-data-folder.md`, `docs/decisions/0023-licensing.md`.
  - Decided 2026-11-15: Kemable gets a website of its own in `site/` at the repository's root (Astro, React and Tailwind), served by Cloudflare Pages, which holds the Terms and Privacy pages; no payments until there is something to sell, and the domain is bought later (until then the Pages address). SoleBuilt's own site links to it. Until it is live, the stand-in URLs and the product's name leave the app and the ADRs, the terms dialog stays hidden, and About points to GitHub. The site gets an ADR.
- [x] **C18. The README is Vietnamese only, build-from-source only, and out of date.** No download or install section; its "Chạy thử" names buttons the Welcome screen no longer has.
  - Fix: an English README (what Kemable is, a download link, screenshots, the engines, the license), the Vietnamese one beside it.
- [ ] **C19. The installer has no Authenticode signature.** SmartScreen says "Windows protected your PC" and "Unknown publisher" on the first install. Needs a code-signing certificate; the release workflow signs only for the updater (ADR 0028).
  - 2026-11-15: `release.yml` signs the installer when the `WINDOWS_CERTIFICATE` secret is there, and skips it when not (ADR 0028, Authenticode). Open until a certificate is bought.

## D. Small, after the preview

- [x] **D1.** SQL Server: clearing a cell sends `N''`, stored as `0` or `1900-01-01` instead of NULL (`CellEditor.svelte`, `literal.rs`); only the NULL button gives NULL.
- [x] **D2.** Editing a multi-line cell turns its CRLF line ends into LF (the textarea normalises them; `CellEditor.svelte`).
- [x] **D3.** CSV export and import do not round-trip NULL and empty text: both export as an empty field, and "An empty field is NULL" (on by default) reads every empty text back as NULL (`export.rs`, `ImportDialog.svelte`).
- [x] **D4.** Unsaved query text can go: pages are kept for the 40 most recent connections only, and a pages file that cannot be read is treated as empty and written over (`data.rs`); `ui.json` too.
- [x] **D5.** The selected database is one per window, not per page: switching it on one page moves every page (`App.svelte`).
- [x] **D6.** History's Run runs at once; with "This connection" unticked it runs another connection's statement on this one, matched by title only, and changes the window's database without asking (`App.svelte`).
- [x] **D7.** The Drop and Truncate dialogs name neither the database nor the connection, though they run on the tree node's database (`ObjectActionDialog.svelte`).
- [x] **D8.** Opening one database's session holds the server's sessions lock for up to 30 s (the connect timeout): runs, the tree and completion on that connection freeze, and Stop cannot end it (`state.rs`, `run.rs`).
- [x] **D9.** SQL Server and Redis set no TCP keepalive, runs have no deadline, and the HTTP drivers only a connect timeout: a link that dies silently mid-query shows "Running" until Stop (`mssql/src/connection.rs`, `redis/src/link.rs`, `d1/src/api.rs`, `firebase/src/api.rs`). SQL Server maps `WSAETIMEDOUT` to `driver.timeout`, so the dead socket is used once more; a call queued for the editor lock ignores its own Stop and deadline.
- [x] **D10.** PostgreSQL: the rollback of a block a stopped cursor run left open is awaited before Stop is watched (`end_stale_block` in `execute`), so when that run's cancel failed, the next run cannot be stopped.
- [x] **D11.** RabbitMQ: publishing to an exchange that does not exist reports done (no publisher confirms), the broker closes the channel, and every later publish fails until reconnect (`rabbitmq/src/lib.rs`).
- [x] **D12.** Sessions are never closed when idle, though the roadmap says so; each PostgreSQL database touched holds two server connections until disconnect, which can reach a low `max_connections` (`state.rs`, `docs/roadmap.md`).
- [x] **D13.** Nothing stops a second copy of the app; two windows of one user write the same `ui.json` and `pages/*.json`, the last write wins (no single-instance plugin; `data.rs`).
- [x] **D14.** `capabilities/default.json` grants all of `core:default` (menu, tray, image, resources), more than `.claude/rules/ipc.md` allows.
- [x] **D15.** Some failures say nothing: Settings' About buttons and the data folder's Open discard their errors (`SettingsDialog.svelte`); a Credential Manager failure is coded `driver.io`, worded as a file or network error (`library.rs`).
- [x] **D16.** `docs/releases/0.1.0.md` ends "Kemable now updates itself", which on a first release suggests an earlier one.

Seen while recording the landing page's video (2026-11-15, release build, SQLite in memory):

- [x] **D17.** SQLite: a `CREATE TABLE` or `CREATE VIEW` reports the row count of the statement before it ("2,000 rows affected" after an insert of 2,000), since `sqlite3_changes` keeps the last DML's count (`core/crates/drivers/sqlite/src/lib.rs`, `run_statement`). A statement that is not DML reports no count.
- [x] **D18.** The grid's row-number column is too narrow past six digits: at row 2,000,000 the numbers run into the first column (`ui/windows/src/lib/grid/engine.ts`). Its width follows the result's total rows.
- [x] **D19.** A column's width is taken from the first rows, so later, longer values are cut ("row 1" sets the width, "row 600" is clipped) with no way to see them but resizing (`engine.ts`).
- [x] **D20.** An edited number is drawn left-aligned until it is saved, unlike the numbers around it (`engine.ts`, the edit overlay).
- [ ] **D21.** SQLite: a table tab's filter value still goes in by the old typed literal, so `1.10` on a TEXT column is compared as the number 1.1 and may match nothing (A6 changed edits and Import only; `typed_literal` where a filter is written).
- [ ] **D22.** SQL Server: saving edits outside a transaction leaves `XACT_ABORT` on for the session afterwards, as before A3 (`mssql/src/connection.rs`, `apply_edits`).
- [ ] **D23.** A credential profile's password (ADR 0015) is bound to no host: a shared connection that uses the profile, pointed at another host, still gets it (B15 bound connections' own passwords only; `docs/decisions/0033-bound-passwords.md`, undecided).
- [ ] **D24.** SQL Server: a Stop that lands inside a token, in the message where the server also ends a transaction (`XACT_ABORT ON`), misses that change and drops the session, as before A9 (`docs/decisions/0029-tabstream.md`).

## E. Found by the manual run of 2026-11-21

The steps of `docs/testing/` run on the installed build of `task/2026-11-15` (with E1 fixed), against PostgreSQL 17, SQL Server 2025 and SQLite on the machine, through Vitro and a CDP client on the same WebView2. Redis, D1, RabbitMQ, Firebase, Supabase, SSH and the updater were not tried. **E2 to E7** hurt everyday use or lose data; **E8** is the testers' docs; **E9 to E15** are small.

- [x] **E1. After a fast run, the next run stays in "Running" for good.** SQLite in memory: `select 1`, then any statement: no result, Stop and Esc do nothing, every later run is refused. The run ended in Rust (it is in History), but the page dropped all its events.
  - Where: `ui/windows/src/App.svelte` (`startRun`): a fast run's events, `finished` among them, arrive before `run_query` returns; the id it returns was then kept as the run followed, and the next run's events, which carry another id, were all dropped.
  - Fixed 2026-11-21: `ui/windows/src/lib/followed-run.ts` takes a run's events by its channel and keeps no id for an ended run; `followed-run.test.ts`, `pnpm test`. The same order fixed in `lib/live/session.svelte.ts`.
- [ ] **E2. Running again with a result filter empties the result.** PostgreSQL, Limit 1,000: `select * from order_items; select 1;`, filter Result 1 `quantity > 3`, F5: Result 1 shows "0 of 0 rows held" and "Stopped at 0 rows", the grid is empty, and the filter row is back to its defaults (first column, contains, empty) while the WHERE strip still shows the condition.
  - Where: `ui/windows/src/App.svelte` (`onEvent`, `setStarted`): the carried filter is applied by `filterSet` without waiting; the set is added with `filter: null`, so `rows` and `setFinished` count it as unfiltered, then `filterSet` returns the counts of the empty set and writes them over. The `FilterRow` of a result is keyed by `set.result` and mounts before the filter is there, so it shows no condition.
  - Fix: the new set carries the filter's conditions from `setStarted`; `filterSet` reads the counts again once it has run (`refreshCounts`).
- [ ] **E3. JSON and SQL INSERT exports start with a UTF-8 byte order mark that cannot be turned off.** `JSON.parse` fails on the file ("Unexpected token"), and `psql` reads the mark as part of the first statement.
  - Where: `ui/windows/src/lib/data/ExportDialog.svelte` (`bom` starts true and is sent for every format; its checkbox shows only for CSV); `core/crates/core/src/export.rs` (`start` writes the mark whenever `bom` is set).
  - Fix: the mark is for CSV only: the dialog sends `bom` only with CSV, and the core writes it only for CSV; a test per format.
- [ ] **E4. Esc does not stop a run.** The Stop button's tip says "Stop the run  Esc" and opening another connection says "Stop the running query first (Esc)", but nothing binds Esc: `select pg_sleep(60)` ran its full 60 s with Esc pressed. `docs/testing/preview.md` B11, B12, D8 and D10 stop with Esc.
  - Where: `ui/windows/src/App.svelte` (`onWindowKeydown` has no Escape; `stopRun` is reached only from `RunControls.svelte`).
  - Fix: Esc stops the page's run while it runs, unless a dialog, menu, picker, the completion list or a cell editor takes the key first.
- [ ] **E5. Import drops a SQLite `INTEGER PRIMARY KEY` field.** A file `id,ma` / `50,c` into `thu_so (id integer primary key, ...)`: the plan says `id` is "left out: no column of that name", the INSERT has no `id`, and the row gets the next rowid (3) instead of 50.
  - Where: `core/crates/drivers/sqlite/src/lib.rs` (`mark_rowid_alias` marks the rowid alias `generated`); `core/crates/core/src/import.rs` (mapping skips generated columns); `ui/windows/src/lib/data/ImportDialog.svelte` (words every unmapped field as "no column of that name").
  - Fix: a rowid alias takes a value when the file gives one (it is generated only when left out), as an identity column with a value would need `SET IDENTITY_INSERT` on SQL Server; the dialog words a skipped generated column as such.
- [ ] **E6. The SQL files pane's Refresh and Remove from the sidebar do nothing.** The click throws "Cannot read properties of null (reading 'folder')"; the folder stays, also after a restart.
  - Where: `ui/windows/src/lib/sidebar/SqlFolders.svelte`: the items set `menu = null`, then read `target.folder` from `{@const target = menu}`, which is null by then.
  - Fix: read the folder before closing the menu; look for the same order in the other context menus.
- [ ] **E7. A dialog does not take the focus, so keys go to what is behind it.** With a grid cell selected, open Settings or the Close Kemable? question, press Esc: the grid takes it (it collapses its selection and calls `preventDefault`) and the dialog stays.
  - Where: `ui/windows/src/lib/ui/Dialog.svelte` (no focus on open, none given back on close; its Esc handler skips a prevented event); `ui/windows/src/lib/grid/engine.ts` (Escape with a selection).
  - Fix: a dialog moves the focus into itself when it opens and back when it closes; keys outside an open modal dialog do nothing.
- [ ] **E8. The testers' steps name keys and behaviour that changed.** Not a code bug, but the steps mislead whoever runs them:
  - `phase-2.md` §3 pages with Alt+PageDown and Alt+PageUp; the keys are Ctrl+] and Ctrl+[ (as `phase-1.md` 8.3 says).
  - `phase-2.md` §7 expects opening another table or running in the editor to be blocked while a table page has edits; since pages (ADR 0013) they go to other pages, which keep their edits.
  - `phase-2.md` §1 says History's Run switches the database; since D6 it puts the statement in the editor when the database differs.
  - `phase-3.md` 1.6 expects the "Listening in demo_sales, while queries now run in postgres" strip; since D5 each page has its own database.
  - `phase-3.md` §12 times `select g from generate_series(1, 50000000) g` under a second; PostgreSQL computes a set-returning function in `FROM` in full before the first row (12 s here). `select generate_series(1, 50000000)` returns in 1 ms.
  - `phase-3.md` §5 says the first row sorted by `n` is `0`; `x * 7919 % 2000003` is never 0 for `x` up to 2,000,000, so it is `1`.
- [ ] **E9.** A table page's status bar never says "sorted by the server" (`phase-1.md` §5, `phase-2.md` §3), though a query result does (`App.svelte`, the status bar's table-page branch).
- [ ] **E10.** The status bar keeps the previous result's selection after a run ("5 × 6 selected" over a 1-column result); `selected` is not cleared with the results (`App.svelte`).
- [ ] **E11.** Errors coded `config.invalid` are titled "The connection settings are not valid" where no connection is set: the Rename dialog opens already showing it ("the new name is the old one", `object_action.rs`), and so do "Stop the running query first (Esc)" and a duplicate credential profile name (`errors.ts`).
- [ ] **E12.** Wording: "Import 1 rows" and "Imported 1 rows into …" (`ImportDialog.svelte`); "statement 1 changed not one rows" (`error.rs`, `edit_row_gone`); "This file is not UTF-8 or UTF-16 text: not UTF-8 or UTF-16 text: …" (`text_file.rs` repeats the title in the detail).
- [ ] **E13.** PostgreSQL reports "0 rows affected" for `CREATE TABLE`, `DROP`, `DO` and `NOTIFY`, and History shows "0 rows"; SQL Server and SQLite say "Done" (D17). The driver passes `rows_affected()` for every command (`postgres/src/connection.rs`, `command_done`); a count belongs only to INSERT, UPDATE, DELETE, MERGE, COPY and SELECT INTO.
- [ ] **E14.** The object tree (`role="tree"`) has no arrow keys: Right does not open a node, Up and Down do not move (`ObjectTree.svelte`), against the keyboard-first rule.
- [ ] **E15.** A column widens only for a value with more bytes than the widest measured (D19), so a value of the same length with wider digits is cut: at row 2,000,000, "row 2000000" shows as "row 200000(" (`engine.ts`).
