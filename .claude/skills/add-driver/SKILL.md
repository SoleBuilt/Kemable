---
name: add-driver
description: >-
  Add a database or service engine to Paboable (PostgreSQL, SQL Server, Redis, D1, RabbitMQ,
  Firebase, PGlite, or a family member such as Neon or Supabase): a driver crate in core, its
  tests, its registration in the Windows app and its docs. Use for "add a driver", "support <engine>",
  "thêm driver", "hỗ trợ <engine>", or when an engine in docs/roadmap.md is next.
---

# Add a driver

One run ends with the engine listed in the app, its tests passing, `node scripts/verify.mjs` at `PASS`, and the docs saying how it is reached.

## 1. Decide how it is reached

Read `docs/decisions/0004-engines.md`. It names the crate and the limits for each planned engine.

- **A family member** (Neon, Supabase, PGlite, local D1) is not a new crate: it is the family's driver with options, or a new `EngineInfo` in the same crate. Add what differs (TLS server name, the pooler's statement mode, a sidecar) and its tests.
- **A new data model** (the first key-value, document or queue engine) adds its methods to `Connection` with a `driver.unsupported` default first, in its own change to `paboable-core`, with the reason in `core/ARCHITECTURE.md`.
- **Check the library at the version you add.** Read its docs and source for: streaming rows, cancelling a running call, TLS options, error types. `cargo search <crate>` gives the latest version.

## 2. Write the crate

`core/crates/drivers/<engine>/`, named `paboable-driver-<engine>`, added to the root `Cargo.toml` members and `[workspace.dependencies]`. `.claude/rules/drivers.md` holds the rules; in short:

- `pub static INFO: EngineInfo` with the id saved in connection files, the data model, the targets, the default port, the SQL grammar, `live`, `has_databases`, and `tree_folders`: the folders a schema's objects fall in, named as the engine's own tool names them (`paboable_core::engine::SQL_TREE_FOLDERS` when they are the usual ones, SQL Server's SSMS folders as an example of its own). Add a test that every `ObjectKind` the driver's `list_objects` gives sits in one of its folders, as `paboable-driver-mssql` and `paboable-driver-postgres` do; an object in no folder never shows in the tree.
- A unit struct implementing `Driver`; a connection struct implementing `Connection`.
- `connect` honours `CallOptions` between stages and validates the `Target` kind first (`DriverError::config`).
- `execute` streams through the sink in `PUSH_BATCH_ROWS` batches, stops when `push_rows` returns `false`, and cancels on the server.
- Types map exactly to `Value` (the table in `.claude/rules/drivers.md`).
- Errors map to `ErrorCode`, with the engine's own code in `engine_code`.

## 3. Test it

In `src/tests.rs`:

- Without a server: config validation, type mapping helpers, error mapping, anything pure.
- With a server, `#[ignore = "needs <engine> at $PABOABLE_<ENGINE>_URL"]`: connect, a query across every mapped type with their edge values, a command with rows affected, the row limit, cancel during a long statement, a deadline, a syntax error's code and position, the catalog.
- Run the ignored ones by hand against a real server: `PABOABLE_<ENGINE>_URL=... cargo test -p paboable-driver-<engine> -- --ignored`. Say in the report whether they ran.

## 4. Wire it into the app

- `ui/windows/src-tauri/Cargo.toml`: the dependency. `src-tauri/src/lib.rs`: `.with(Arc::new(<Engine>Driver))` in `registry()`.
- The connection form (once it exists) shows the fields the engine's `targets` need.
- A grammar the editor does not know yet maps in `DIALECTS` in `ui/windows/src/lib/editor/SqlEditor.svelte`.

## 5. Document it

- `docs/decisions/0004-engines.md`: correct the engine's row with what you learned.
- `docs/roadmap.md`: tick the item.
- `core/ARCHITECTURE.md`: the crate in the layout.

## 6. Verify

`node scripts/verify.mjs` (the `verify` skill). Then `pnpm tauri dev`, connect to a real server, run a query, scroll the result, press Stop on a long one.
