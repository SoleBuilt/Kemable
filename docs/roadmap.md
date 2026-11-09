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
- [ ] **Connection form: Database is a combobox.** Once host, port, user and password are filled, list the databases the login may see (the same query as above) and let the user pick the one to open first. Typing a name stays possible.
- [x] **A script that creates a database runs the rest in it.** Decided (2026-11-09): after `CREATE DATABASE x` succeeds, the run continues in `x`: on PostgreSQL the editor reopens its session on `x`, on SQL Server and MySQL the run sends `USE x`. The Messages tab says "Switched to x". psql's `\c x` / `\connect x` is also read as a directive that switches. This differs from psql, pgAdmin, DBeaver, DataGrip and TablePlus, which all stay on the database they were on, so the switch is announced. Done, except the setting to turn it off, which waits for a settings screen (`FOLLOW_CREATED_DATABASE` in `ui/windows/src/App.svelte`).
- [x] **Switch database without the connect dialog.** A database picker in the toolbar and Ctrl+K, reusing the saved credentials. Agreed 2026-11-09.
- [x] **Run and Run all are easy to confuse.** Ctrl+Enter / ▶ Run runs only the statement under the cursor. Label it "Run statement", flash the range that ran, and say "Ran 1 of 4 statements" in the status bar, so running one statement of a script is never mistaken for running all of it.
- [x] **Sidebar refresh.** A refresh button and Ctrl+R on the object tree, besides the refresh after a run that changed the schema.

## Phase 2: daily use

- [x] Query history and saved queries. The history is `history.db` (SQLite) in the app data folder, at most 20,000 entries, and keeps only what a person ran: not a sort, not opening a table, not a statement that holds a password. Saved queries are `queries.json`, each for one saved connection or for all.
- [x] Table browsing with server-side paging and filters; a table tab's sort pages with it (ADR 0005, ADR 0006).
- [x] Autocomplete from the catalog (ADR 0007).
- [x] Redis: keys, typed values, TTL (ADR 0008). Tested against a RESP2 server in the tests; the live test waits for a Redis at `$PABOABLE_REDIS_URL`.
- [ ] D1 over HTTP.
- [ ] Editing rows, with a preview of the statements before they run.

## Phase 3: everything else

- [ ] The Live tab: PostgreSQL `LISTEN`, Supabase Realtime, Redis pub/sub, RabbitMQ consumers, Firebase.
- [ ] RabbitMQ: queues, exchanges, peek, publish.
- [ ] Firebase: Firestore collections and documents, Realtime Database.
- [ ] PGlite through a pglite-socket sidecar.
- [ ] SSH tunnels.
- [ ] Export and import.

## Later

- `ui/apple` and `ui/linux`, each with its own decision on toolkit.
