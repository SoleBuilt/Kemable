# Roadmap

Windows first. Each phase ends with `node scripts/verify.mjs` passing and the app tried by hand.

## Phase 1: the fast core loop

- [x] Workspace, lints, `node scripts/verify.mjs`, rules and skills.
- [x] Core: values and exact formatting, errors with codes, `CallOptions`, the driver contract, SQL splitting for PostgreSQL, SQL Server and SQLite.
- [x] Store: columnar result sets, packed windows.
- [x] SQLite driver, with interrupt on cancel and deadline.
- [x] Windows app: open a SQLite file, schema tree, SQL editor, run statement or script, streaming canvas grid, Stop, Messages.
- [x] PostgreSQL driver (also Neon and Supabase), with TLS and the cancel request; a connect dialog that reads a pasted URL.
- [x] SQL Server driver, with `GO` batches and attention; Windows or SQL Server login; a connect dialog shared with PostgreSQL.
- [x] Connection form and saved connections; secrets in Windows Credential Manager.
- [x] Grid: range selection, copy as TSV, a row detail pane (Ctrl+Alt+I).
- [x] Sorting, as `docs/decisions/0005-sorting-a-result.md` decides: a table tab sorts on the server with paging; a query result runs again with `ORDER BY` when the column can be named, else the core sorts the rows held. Header click cycles ascending, descending, unsorted.
- [x] Row cap as a setting (the toolbar's Limit, remembered), Run Without Limit (Ctrl+Alt+Enter), and Fetch All for a capped result.

## UX backlog, from using the app

Found while trying the app on a local PostgreSQL (2026-11-09). To do before Phase 2's new features.

- [x] **A connection is to the server, and the sidebar shows every database the login may see.** As SQL Server Management Studio's Object Explorer does: Database in the connection form is optional; once connected, the tree's top level is the server's databases, each expanding to its schemas and objects. The list is what the server lets this login see (PostgreSQL `pg_database` where `has_database_privilege(datname, 'CONNECT')`, SQL Server `sys.databases`, which already filters by permission). PostgreSQL needs a session per database, so a database's session opens when it is first expanded or used, and closes after it has been idle a while.
- [x] **Connection form: Database is a combobox.** Once host, port, user and password are filled, list the databases the login may see (the same query as above) and let the user pick the one to open first. Typing a name stays possible. Built 2026-11-11: the list is read when it opens (`list_form_databases`, signing in as Test does), again only when the login changes; Database now sits after the login and Encryption; Default is the login's own database; a name typed in the search becomes "Use <name>".
- [x] **A script that creates a database runs the rest in it.** Decided (2026-11-09): after `CREATE DATABASE x` succeeds, the run continues in `x`: on PostgreSQL the editor reopens its session on `x`, on SQL Server and MySQL the run sends `USE x`. The Messages tab says "Switched to x". psql's `\c x` / `\connect x` is also read as a directive that switches. This differs from psql, pgAdmin, DBeaver, DataGrip and TablePlus, which all stay on the database they were on, so the switch is announced. Settings turns it off (After CREATE DATABASE, run the rest of the script in the new database).
- [x] **Switch database without the connect dialog.** A database picker in the toolbar and Ctrl+K, reusing the saved credentials. Agreed 2026-11-09.
- [x] **Run and Run all are easy to confuse.** Ctrl+Enter / ▶ Run runs only the statement under the cursor. Label it "Run statement", flash the range that ran, and say "Ran 1 of 4 statements" in the status bar, so running one statement of a script is never mistaken for running all of it.
- [x] **Sidebar refresh.** A refresh button and Ctrl+R on the object tree, besides the refresh after a run that changed the schema.
- [x] **Filter a query result.** The table tab's filter row on the result bar, filtering the rows the result holds in the core, with the `WHERE` shown to copy and "120 of 1,000 rows held" in the status bar. Decided 2026-11-10: `docs/decisions/0011-filtering-a-result.md`.
- [x] **Open <table> to edit, from a query result.** Shown when the core can name exactly one table behind a result (ADR 0011). Decided and built 2026-11-11: the core reads the statement; its `WHERE` becomes the table tab's filter when the filter row can hold it (ADR 0011, amended).
- [x] **The approved design system and screens.** Pages of query and table tabs, the command palette, the toolbar that says where you are, sidebar panes, the pager in the status bar, Settings. Decided 2026-11-11: `docs/decisions/0012-design-system.md`, `docs/decisions/0013-pages-and-tabs.md`.
- [x] **A Mica theme.** A theme of its own, picked in Settings: Windows 11 Mica behind the toolbar and sidebar. It needs a transparent WebView2 window, so measure the grid's frame times before it ships. Decided 2026-11-10 (design system decision D10, backlog B1). Built 2026-11-11 as Settings › Mica background. Frame times measured 2026-11-11: no difference with the transparent window or with Mica on (ADR 0012, Consequences).
- [x] **Connection colours.** A colour kept with each saved connection, picked in the connect form, shown as the toolbar's 3 px stripe and as a dot in the palette (design system D8). Built 2026-11-11: the dot in Ctrl+K and the Saved connection list; no stripe, as asked (ADR 0012, Consequences).
- [x] **The tree menu's other items.** DDL, structure, export and import, transfer, generate rows, rename, maintenance, truncate and drop (with a confirmation). Built 2026-11-11 and 2026-11-12: Show and Copy DDL, Show Structure (columns, indexes, DDL), Edit View Definition, Rename, Edit Comment, Maintenance, Truncate, Drop (`docs/decisions/0017-tree-actions.md`); Export, Import, Generate Rows (`docs/decisions/0018-export-import-generate.md`).
- [ ] **Open with Paboable, from Explorer.** As VS Code does: right-click a `.sql` file (or several) in Explorer and pick Open with Paboable to open it as a query page; right-click a folder, or the empty space inside one, to open the folder in Paboable, its `.sql` files listed in a sidebar pane to open with a click. The installer adds the menu items (and a running app takes the file instead of a second window); with it, File › Open Folder inside the app. Asked 2026-11-12, later.
- [ ] **Transfer To and Copy To.** Copy a table's rows into a table of another connection (Transfer To), or the table itself, its DDL and its rows, to another database or connection (Copy To). First between servers of one engine; across engines the types must be mapped. Asked 2026-11-12, later.
- [x] **Test connection** in the connect form, and a **Profile** in the palette. Test connection is built (2026-11-11): connect, read the version and the databases, close. Profile means credential profiles, built 2026-11-11: `docs/decisions/0015-credential-profiles.md`.
- [x] **Ctrl+click picks rows one by one** in the grid (design system D21). Shift picks a range today; the core's copy takes one rectangle, so it needs a selection of several ranges. Built 2026-11-11: a selection is row spans of the same columns, copied by `ResultStore::copy_tsv_rows`.
- [x] **Open and save `.sql` files**, beside saved queries. Decided and built 2026-11-11: `docs/decisions/0016-sql-files.md`.
- [ ] **A file changed outside the app** shows a notice: Reload from disk, or keep my changes. Later, after `.sql` files (ADR 0016).
- [ ] **Link a folder of `.sql` files** into the sidebar. Later, after `.sql` files (ADR 0016).
- [ ] **The driver names a result's source table** (optional, not required): PostgreSQL's table OID per column, SQLite's column metadata, SQL Server's `sp_describe_first_result_set`. More exact than reading the statement, for "Open <table> to edit" and any later editing in place (ADR 0011, amended 2026-11-11).
- [x] **Pages across launches.** Query tabs and their text come back when the app opens again. Built 2026-11-11: per connection, when it opens again (ADR 0013, Consequences).
- [x] **An orange style.** A second accent style like ERP Tinh gọn's orange `primary`, picked in Settings beside the default black and white. Only the accent tokens change; shapes stay. Decided 2026-11-10 (backlog B2). Built 2026-11-11: `data-style="orange"` on `<html>`, the tokens in `ui/windows/src/app.css`.

## Phase 2: daily use

- [x] Query history and saved queries. The history is `history.db` (SQLite) in the app data folder, at most 20,000 entries, and keeps only what a person ran: not a sort, not opening a table, not a statement that holds a password. Saved queries are `queries.json`, each for one saved connection or for all.
- [x] Table browsing with server-side paging and filters; a table tab's sort pages with it (ADR 0005, ADR 0006).
- [x] Autocomplete from the catalog (ADR 0007).
- [x] Redis: keys, typed values, TTL (ADR 0008). Tested against a RESP2 server in the tests; the live test waits for a Redis at `$PABOABLE_REDIS_URL`.
- [x] D1 over HTTP (ADR 0009). Tested against an HTTP server in the tests; the live test waits for `$PABOABLE_D1_ACCOUNT`, `$PABOABLE_D1_TOKEN` and `$PABOABLE_D1_DATABASE`.
- [x] Editing rows, with a preview of the statements before they run (ADR 0010).

## Phase 3: everything else

- [ ] The Live tab: PostgreSQL `LISTEN`, Supabase Realtime, Redis pub/sub, RabbitMQ consumers, Firebase.
- [ ] RabbitMQ: queues, exchanges, peek, publish.
- [ ] Firebase: Firestore collections and documents, Realtime Database.
- [ ] PGlite through a pglite-socket sidecar.
- [ ] SSH tunnels.
- [ ] Export and import.

## Later

- `ui/apple` and `ui/linux`, each with its own decision on toolkit.
