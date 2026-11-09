---
paths:
  - "ui/windows/src/lib/grid/**/*"
---

# Data grid

The grid is `GridEngine` (`engine.ts`): one canvas laid over a native scroll container. `DataGrid.svelte` only mounts it and passes changes in.

## Speed

- **Draw, never mount.** No DOM element per row or per cell. A frame paints only the visible rows and columns.
- **The browser scrolls.** The scroller holds an empty spacer as big as the data; the canvas has `pointer-events: none` and redraws on `scroll` inside one `requestAnimationFrame`. Never implement scrolling or momentum by hand.
- **Rows past 16,000,000 px are scaled** onto the scroll range (`rowsTop`, `setRowsTop`). Any code that turns a scroll position into a row goes through them.
- **Data arrives in blocks of `BLOCK_ROWS`** through `fetch_window`, with one block of read-ahead each way, an LRU of `MAX_BLOCKS`, and a re-read of a block that was read while rows were still arriving. A response from an older source is dropped by `generation`.
- **Text is decoded lazily**, per cell, the first time it is drawn (`ResultWindow.text`).
- **One `save`/`clip`/`restore` per column**, not per cell. Fonts and colours are set once per pass.
- **`measureText` only** when sizing columns from the first block, never in the draw loop.

## Behaviour

- Numbers align right, everything else left, by each cell's own kind (SQLite mixes kinds in a column).
- NULL is drawn as `NULL` in the muted colour. The core never sends that word.
- **Sorting is the core's.** A header click asks the owner to sort (ascending, descending, unsorted; Shift adds a key); the grid only draws the arrow. The owner runs the statement again or asks the core for a sorted view (`docs/decisions/0005-sorting-a-result.md`). The grid never reorders rows itself.
- Colours come from the CSS tokens through `readTheme()`, re-read when the system scheme changes. No colour is written in this folder.
- **A selection is an anchor, a focus (the cell cursor) and a mode**: cells, rows, columns or all. Click a cell, drag or Shift+click for cells; the row numbers for rows; Ctrl+click a header for columns (a plain header click is the sort's); the corner or Ctrl+A for all. The grid reports every change through `selectionChanged`.
- Keys: arrows, Ctrl+arrows, Tab, Page Up/Down, Home/End, Ctrl+Home/End move the cursor; with Shift they extend the selection; Escape collapses it.
- **Copy is the core's.** Ctrl+C copies the selection as tab-separated text built by `ResultStore::copy_tsv` (CRLF rows, NULL as an empty field, quoting as spreadsheets read it); Ctrl+Shift+C adds the column names. Never assemble copied text from windows: they are cut at the character cap.
- **Edits are drawn, not stored.** The owner passes an `EditOverlay` (new cell values, deleted rows, new rows after `baseRows`); the grid tints and draws them over the rows and never fetches a block past `baseRows`. Keys and double-click only ask the owner (`editRequested`, `deleteRequested`); the editor input is the owner's (`docs/decisions/0010-editing-rows.md`).
- Svelte state never reaches the draw loop: `DataGrid.svelte` passes plain objects (`$state.raw` in the owner), so no proxy is read per cell.

## Checking a change

Open a large result (`with recursive c(x) as (select 1 union all select x + 1 from c limit 2000000) select x, x * 2, 'row ' || x from c` on an in-memory SQLite) and scroll with the wheel, the scrollbar and the keyboard. Scrolling must stay smooth while rows still arrive, and Chrome DevTools' Performance panel (right-click, Inspect, in a dev build) must show frames under 8 ms.
