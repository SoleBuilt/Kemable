# Roadmap

Windows first. Each phase ends with `node scripts/verify.mjs` passing and the app tried by hand.

## Phase 1: the fast core loop

- [x] Workspace, lints, `node scripts/verify.mjs`, rules and skills.
- [x] Core: values and exact formatting, errors with codes, `CallOptions`, the driver contract, SQL splitting for PostgreSQL, SQL Server and SQLite.
- [x] Store: columnar result sets, packed windows.
- [x] SQLite driver, with interrupt on cancel and deadline.
- [x] Windows app: open a SQLite file, schema tree, SQL editor, run statement or script, streaming canvas grid, Stop, Messages.
- [ ] PostgreSQL driver (also Neon and Supabase), with TLS and the cancel request.
- [ ] SQL Server driver, with `GO` batches and attention.
- [ ] Connection form and saved connections; secrets in Windows Credential Manager.
- [ ] Grid: range selection, copy as TSV, a cell detail pane, column sort by re-running the query.
- [ ] Row cap as a setting (today a fixed 500,000 in `run.rs`), Run Without Limit (Ctrl+Alt+Enter), and Fetch All for a capped result.

## Phase 2: daily use

- [ ] Query history and saved queries.
- [ ] Table browsing with server-side paging and filters.
- [ ] Autocomplete from the catalog.
- [ ] Redis: keys, typed values, TTL.
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
