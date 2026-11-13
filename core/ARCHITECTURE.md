# Architecture

## Crate layout

```
Cargo.toml                      the workspace, at the root, with rust-toolchain.toml, rustfmt.toml, clippy.toml
core/crates/
├── core/                       kemable-core: domain types, the driver contract, SQL splitting. No I/O.
├── store/                      kemable-store: columnar result sets and the packed windows a grid reads
├── storage/                    kemable-storage: saved connections in connections.json, credential profiles in profiles.json; secrets through SecretVault
└── drivers/
    ├── d1/                     kemable-driver-d1: Cloudflare D1 through its REST API, reqwest with rustls
    ├── mssql/                  kemable-driver-mssql: tiberius; SQL Server, Windows or SQL Server login
    ├── postgres/               kemable-driver-postgres: tokio-postgres and rustls; PostgreSQL, Neon, Supabase
    ├── redis/                  kemable-driver-redis: its own RESP2 client on tokio and rustls; Redis, Valkey
    └── sqlite/                 kemable-driver-sqlite: rusqlite, SQLite compiled in
ui/windows/src-tauri/           kemable-windows: the Tauri app, the only crate that names every driver
```

Planned crates, in the order `docs/roadmap.md` gives: `drivers/rabbitmq`, `drivers/firebase`, `session` (health, reconnect). The Live tab needed no crate of its own: a driver's `listen` and a result set hold it (`docs/decisions/0020-live-tab.md`).

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

`kemable_core::driver::Driver` describes an engine before any connection exists (`EngineInfo`: id, data model, how it is reached, its SQL grammar, the topics a Live tab can listen to, `live_topics`, and the object tree's folders under a schema, `tree_folders`) and opens a `Connection`.

`Driver::listen(config, topics, sink, options)` is the Live tab's (`docs/decisions/0020-live-tab.md`): on a link of its own, never one of the connection's sessions, it streams each event into the sink as a row of one set, pushed as it arrives, and returns only when it stops: at the sink's row limit, after a cancel, or with `driver.lost`. `begin_set` comes once the server has taken every topic. `kemable_core::live` holds the topic types and reads the names a user typed. It defaults to `driver.unsupported`; PostgreSQL (`LISTEN`) and Redis (`SUBSCRIBE`, `PSUBSCRIBE`) implement it.

`kemable_core::connection::Connection` is one live link. Every method takes `CallOptions`:

- `deadline`: the call returns by then.
- `cancel`: a `CancellationToken`. The driver stops the statement on the server when it fires (SQLite `sqlite3_interrupt`, PostgreSQL's cancel request, a TDS attention). Dropping the future alone leaves the statement running.

`execute(statement, sink, options)` runs one statement, already split from its script, and streams rows into a `ResultSink`: `begin_set`, then `push_rows` in batches of `PUSH_BATCH_ROWS`, then `end_set`; or `command_done` for a statement without rows. `push_rows` returns `false` at the row limit and the driver stops reading.

An engine whose server holds several databases sets `EngineInfo::has_databases` and answers `current_database` and `list_databases` (what the login may open, as the server's permissions decide). A `Connection` stays bound to one database: PostgreSQL cannot change a session's database, so the app opens one connection per database in use (`ui/windows/src-tauri/src/state.rs`, `OpenServer`). After a run's statement, `kemable_core::sql::database_change` says whether it moved the run (`CREATE DATABASE x`, SQL Server's `USE x`, psql's `\c x`, which the PostgreSQL grammar splits off as a `StatementKind::Directive`).

Methods a data model does not use return `driver.unsupported`. Each data model adds its own methods with that default as its first engine lands (`docs/decisions/0004-engines.md`): key-value has `scan_keys` and `read_key` (`docs/decisions/0008-key-value-engines.md`), and an engine without SQL runs console text split by `kemable_core::command`, one command per line.

## SQL scripts

`kemable_core::sql::split(text, grammar)` returns the byte ranges of the statements a run sends; `statement_at` picks the one under the cursor. The module docs of `crates/core/src/sql/split.rs` have the table of what each grammar keeps whole. Every engine of a family shares its grammar: Neon, Supabase and PGlite read as PostgreSQL, D1 as SQLite.

## Result sets

`docs/decisions/0002-result-windows.md` has the reasons. In short:

- A `ResultStore` keeps each column in chunks of 65,536 rows: a kind byte, a raw `i64`, a null bit and the display text (or a binary value's bytes) per cell. Appending never copies rows already held.
- A grid asks for one window (first row, row count, first column, column count, character cap) and gets one packed buffer. Rows and the row count come from one read lock, so a window is one snapshot.
- The full value of a cell, for copy and the detail view, comes from `ResultStore::cell`; a window carries display text cut at its cap.
- Sorting (`docs/decisions/0005-sorting-a-result.md`): `sql::sorted_statement` writes the statement again with `ORDER BY` when the column can be named or numbered; otherwise `ResultStore::sort` sets a row order over the same chunks, by `kemable_store::order`, and windows, copies and row reads go through it. It sorts one column at a time: each row's cell is read once into a key of bytes that order as the cells do, the rows are sorted by key alone (a radix sort for numbers and times, a comparison of keys for text), and each run of rows with the same key is settled by itself, by the next sort column or, for texts alike past a key's width, by keys of what follows the start they share.
- Filtering a query result (`docs/decisions/0011-filtering-a-result.md`): `ResultStore::filter` keeps the rows held that match `sql::ResultFilter` conditions (by column index, all or any) as a row order, filtered first and then sorted; windows, copies and row reads go through it, and a window's row count is the rows shown. `sql::result_where` writes the same conditions as a `WHERE` to copy, leaving out a column whose name is empty or shared.
- Table tabs (`docs/decisions/0006-table-browsing.md`): `sql::page_statement` and `sql::count_statement` write one page of a table and the count of the rows its filters match, with every name quoted and every value a literal.
- Editing rows (`docs/decisions/0010-editing-rows.md`): `sql::edit_statement` writes one statement per row, naming it by its key's values (`sql::value_literal`); `Connection::apply_edits` runs them in one transaction, each changing exactly one row, else `edit.row_gone` and nothing saved.
- Autocomplete (`docs/decisions/0007-autocomplete.md`): `sql::completion_context` reads the clause, the word typed, its qualifier and the tables in scope; `lookups` names the catalog reads it needs and `completions` builds the items. `sql/lex.rs` is the lexer it shares with the sort, read strictly there and tolerantly here.

## Values

`Value` holds what the driver read, with no formatting. `format::write_display` writes the text every UI shows: exact decimals, the shortest round-trip float, ISO dates with an expanded year past 9999, times with microseconds and trailing zeros trimmed, UUIDs in lowercase. Locale, the NULL placeholder and alignment belong to the UI.

## Errors

`DriverError { code, message, engine_code, position }`. `code` is an `ErrorCode` serialized as a stable string (`driver.timeout`, `driver.query`, ...). `engine_code` is the engine's own (a SQLSTATE, an MSSQL number, a SQLite extended code). `position` is a byte offset into the statement when the engine gives one.
