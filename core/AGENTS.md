# Paboable core

The Rust crates every UI builds on: the domain types and the driver contract, one crate per engine, and the columnar result store. None has a GUI or a platform dependency. The root `AGENTS.md` holds the rules every folder shares; `ARCHITECTURE.md` beside this file holds the design.

## Principles

1. **No GUI, no platform service.** A core crate depends on no UI toolkit and no Tauri. What a platform does its own way (the keyring, file dialogs, the browser) comes in as a value or a trait the app passes in.
2. **Every target builds.** The crates build for Windows, macOS and Linux. Platform code goes behind `cfg` with the other platforms' answer beside it.
3. **One-directional crates.** `ARCHITECTURE.md`, Dependency rules. Drivers never depend on each other.
4. **Every call has a deadline and a cancel.** A connection method takes `CallOptions` and stops the work on the server when it fires, not only the future.
5. **Exact values.** Drivers map engine types to `Value` without formatting; `paboable_core::format` writes the display text. Nothing rounds.
6. **Errors carry a code.** `DriverError::code` is stable; `message` is the engine's text for the detail view.

## Map

- `crates/core`: `value`, `format`, `catalog`, `config`, `engine`, `error`, `call`, `sink`, `connection`, `driver`, `registry`, `sql` (script splitting per grammar, and `batch`: the statements inside a SQL Server batch).
- `crates/store`: `ResultStore` (columnar chunks, with its sort and filter as row orders over them: `order.rs`, `filter.rs`) and `window` (the packed window format and its reader).
- `crates/storage`: `ConnectionLibrary` (saved connections, `connections.json`, atomic writes, never over a file it cannot read), `profiles` (credential profiles, `profiles.json`, ADR 0015), `queries`, `history`, and the `SecretVault` trait.
- `crates/drivers/<engine>`: one crate per engine, each exporting a `Driver` and its `EngineInfo`. Today: `sqlite`, `postgres`, `mssql`.

## Commands

```bash
cargo test -p paboable-core -p paboable-store -p paboable-driver-sqlite
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

`node scripts/verify.mjs` at the root runs these with the UI checks.

## Every change

- A new behavior comes with a test in the same crate. Driver tests that need a server are `#[ignore]` with the reason, and run through the `add-driver` skill's steps.
- A change to the window format changes `crates/store/src/window.rs`, `ui/windows/src/lib/grid/window.ts` and `docs/decisions/0002-result-windows.md` together.
- A new error code goes in `ErrorCode`, and its words in `ui/windows/src/lib/errors.ts`.
