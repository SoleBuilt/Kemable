# Architecture

## Crate layout

```
Cargo.toml                      the workspace, at the root, with rust-toolchain.toml, rustfmt.toml, clippy.toml
core/crates/
├── core/                       paboable-core: domain types, the driver contract, SQL splitting. No I/O.
├── store/                      paboable-store: columnar result sets and the packed windows a grid reads
├── storage/                    paboable-storage: saved connections in connections.json; secrets through SecretVault
└── drivers/
    ├── mssql/                  paboable-driver-mssql: tiberius; SQL Server, Windows or SQL Server login
    ├── postgres/               paboable-driver-postgres: tokio-postgres and rustls; PostgreSQL, Neon, Supabase
    ├── redis/                  paboable-driver-redis: its own RESP2 client on tokio and rustls; Redis, Valkey
    └── sqlite/                 paboable-driver-sqlite: rusqlite, SQLite compiled in
ui/windows/src-tauri/           paboable-windows: the Tauri app, the only crate that names every driver
```

Planned crates, in the order `docs/roadmap.md` gives: `drivers/d1` (HTTP API), `drivers/rabbitmq`, `drivers/firebase`, `session` (health, reconnect), `live` (the Live tab's subscriptions).

## Dependency rules

| Crate | Depends on |
|---|---|
| `core` | no workspace crate |
| `store` | `core` |
| `storage` | `core`. Secrets go through the `SecretVault` trait the app implements, never to the file |
| `drivers/*` | `core` only. Never another driver, never `store`. Their tests may use `store` |
| an app (`ui/windows/src-tauri`) | everything above |

- No core crate depends on Tauri, a UI toolkit or an app crate.
- Only an app names every driver, in its `DriverRegistry`. Everything else reaches drivers through `Driver` and `Connection`.

## The driver contract

`paboable_core::driver::Driver` describes an engine before any connection exists (`EngineInfo`: id, data model, how it is reached, its SQL grammar, whether it has live changes) and opens a `Connection`.

`paboable_core::connection::Connection` is one live link. Every method takes `CallOptions`:

- `deadline`: the call returns by then.
- `cancel`: a `CancellationToken`. The driver stops the statement on the server when it fires (SQLite `sqlite3_interrupt`, PostgreSQL's cancel request, a TDS attention). Dropping the future alone leaves the statement running.

`execute(statement, sink, options)` runs one statement, already split from its script, and streams rows into a `ResultSink`: `begin_set`, then `push_rows` in batches of `PUSH_BATCH_ROWS`, then `end_set`; or `command_done` for a statement without rows. `push_rows` returns `false` at the row limit and the driver stops reading.

An engine whose server holds several databases sets `EngineInfo::has_databases` and answers `current_database` and `list_databases` (what the login may open, as the server's permissions decide). A `Connection` stays bound to one database: PostgreSQL cannot change a session's database, so the app opens one connection per database in use (`ui/windows/src-tauri/src/state.rs`, `OpenServer`). After a run's statement, `paboable_core::sql::database_change` says whether it moved the run (`CREATE DATABASE x`, SQL Server's `USE x`, psql's `\c x`, which the PostgreSQL grammar splits off as a `StatementKind::Directive`).

Methods a data model does not use return `driver.unsupported`. Each data model adds its own methods with that default as its first engine lands (`docs/decisions/0004-engines.md`): key-value has `scan_keys` and `read_key` (`docs/decisions/0008-key-value-engines.md`), and an engine without SQL runs console text split by `paboable_core::command`, one command per line.

## SQL scripts

`paboable_core::sql::split(text, grammar)` returns the byte ranges of the statements a run sends; `statement_at` picks the one under the cursor. The module docs of `crates/core/src/sql/split.rs` have the table of what each grammar keeps whole. Every engine of a family shares its grammar: Neon, Supabase and PGlite read as PostgreSQL, D1 as SQLite.

## Result sets

`docs/decisions/0002-result-windows.md` has the reasons. In short:

- A `ResultStore` keeps each column in chunks of 65,536 rows: a kind byte, a raw `i64`, a null bit and the display text (or a binary value's bytes) per cell. Appending never copies rows already held.
- A grid asks for one window (first row, row count, first column, column count, character cap) and gets one packed buffer. Rows and the row count come from one read lock, so a window is one snapshot.
- The full value of a cell, for copy and the detail view, comes from `ResultStore::cell`; a window carries display text cut at its cap.
- Sorting (`docs/decisions/0005-sorting-a-result.md`): `sql::sorted_statement` writes the statement again with `ORDER BY` when the column can be named or numbered; otherwise `ResultStore::sort` sets a row order over the same chunks, by `paboable_store::order`, and windows, copies and row reads go through it.
- Table tabs (`docs/decisions/0006-table-browsing.md`): `sql::page_statement` and `sql::count_statement` write one page of a table and the count of the rows its filters match, with every name quoted and every value a literal.
- Autocomplete (`docs/decisions/0007-autocomplete.md`): `sql::completion_context` reads the clause, the word typed, its qualifier and the tables in scope; `lookups` names the catalog reads it needs and `completions` builds the items. `sql/lex.rs` is the lexer it shares with the sort, read strictly there and tolerantly here.

## Values

`Value` holds what the driver read, with no formatting. `format::write_display` writes the text every UI shows: exact decimals, the shortest round-trip float, ISO dates with an expanded year past 9999, times with microseconds and trailing zeros trimmed, UUIDs in lowercase. Locale, the NULL placeholder and alignment belong to the UI.

## Errors

`DriverError { code, message, engine_code, position }`. `code` is an `ErrorCode` serialized as a stable string (`driver.timeout`, `driver.query`, ...). `engine_code` is the engine's own (a SQLSTATE, an MSSQL number, a SQLite extended code). `position` is a byte offset into the statement when the engine gives one.
