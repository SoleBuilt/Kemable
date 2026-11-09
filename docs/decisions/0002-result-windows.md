# 0002: The core owns result sets; a grid pulls packed windows

- **Status**: Accepted
- **Date**: 2026-11-09

## Context

A query can return millions of rows while more are still arriving. The Windows UI runs in WebView2: every byte that crosses from Rust is copied into the webview, and JSON serialization of rows costs more than the rows. A columnar store holds a million rows in far less memory than one object per row, packing the one window of display text a grid shows is cheap, and letting each UI format typed values changes them (rounded decimals, lost microseconds).

## Decision

- The core keeps each result set in a `ResultStore`: columnar chunks of 65,536 rows, each cell a kind byte, a raw `i64`, a null bit and its exact display text (or a binary value's bytes).
- The core writes the display text (`paboable_core::format`). The UI adds only what depends on words or locale: the NULL placeholder, alignment, colours.
- A grid asks for one window: first row, row count, first column, column count, character cap. It gets one buffer in the layout `core/crates/store/src/window.rs` documents, sent as raw bytes through `tauri::ipc::Response`, read in TypeScript by `ui/windows/src/lib/grid/window.ts` with typed-array views and no copy.
- Drivers append at wire speed. The UI learns the row count from events sent at most every 50 ms, and fetches the windows it shows.
- The full value of a cell, for copy and the detail view, comes from a separate call.

## Rationale

- Exactness: one formatter, in Rust, for every UI.
- Speed: a window of 128 rows by 20 columns is tens of kilobytes; nothing per row is serialized as JSON, and only visible cells are decoded (lazily, per cell).
- Memory: columnar chunks grow without copying, and a set is freed by id when the UI releases it.

## Consequences

- The window format is a contract between `window.rs` and `window.ts`. A change updates both and this file.
- Sorting, filtering and editing a result are not designed yet. Server-side sort re-runs the query; a client-side sort will be an index over the store, kept in the core.
- Each result set stops at `ROW_LIMIT` (`ui/windows/src-tauri/src/run.rs`) and says so.

## Alternatives considered

- **Rows as JSON events.** Simple, but serializes every row, blocks the webview while parsing, and formats values in JavaScript.
- **Arrow IPC to the UI.** Fast to move, but JavaScript would format values, and formatting outside the core is where values change.
- **A call per cell.** Thousands of IPC round trips per frame.
