# 0010: Editing rows: edits on a table tab, statements shown before they run, saved all or none

- **Status**: Accepted
- **Date**: 2026-11-10
- **Follows**: `docs/decisions/0006-table-browsing.md` (a table tab and its primary key).

## Context

A table tab shows one page of a table. Changing a cell, adding a row or deleting rows should not touch the database until the user has seen what will run, and a save should never leave half of it done.

## Decision

### Where edits happen

- **Only on a table tab whose table has a primary key.** The key names each row in the `WHERE`; a table without one (or a view) is read-only, and the edit bar says why. A query result is never editable: it may come from several tables, or none.
- **Edits wait in the UI** until Save: new cell values (text, or NULL), rows marked for delete, new rows after the page's. The grid draws them over the page (`EditOverlay` in `ui/windows/src/lib/grid/engine.ts`): an edited cell tinted, a deleted row struck through, a new row marked `*` with `DEFAULT` in the cells not set yet.
- **Keys**: double-click, F2, Enter or typing on a cell opens an editor over it (Enter saves and goes down, Tab right, Shift+Tab left, Escape cancels, Shift+Enter a new line, Ctrl+Shift+N NULL). Delete marks the selected rows, or unmarks them when all are marked; on new rows it removes them. Alt+Insert adds a row. Ctrl+S opens the preview.
- **Nothing replaces the page while edits wait**: paging, sorting, filtering, opening another table, a run from the editor and Disconnect say to save or discard first.

### The statements are the core's

`paboable_core::sql::edit_statement(grammar, table, edit)` writes one statement per row: `UPDATE ... SET <changed columns> WHERE <key>`, `DELETE ... WHERE <key>`, `INSERT ... (<set columns>) VALUES (...)` (`DEFAULT VALUES` when none is set). They run deletes first, then updates, then inserts, so a key a delete frees can be taken by the rest.

- **The key's values are the page's own**, read from the result set and written exactly (`sql::value_literal`): numbers bare, bytes in each engine's form (`'\x..'::bytea`, `0x..`, `X'..'`), booleans as `TRUE` or `1`, everything else as quoted text the engine reads back to the same value. `CellValue::into_value` turns a stored cell back into the value its literal needs.
- **A typed value is text the server converts** to the column's type, as a filter's is (`sql::typed_literal`): quoted with its quotes doubled, `N'...'` on SQL Server, a plain number bare on SQLite. A value that does not fit the column fails the save with the server's error.

### Preview, then save all or none

The preview shows the statements as they will run, with Copy SQL. Save calls `Connection::apply_edits(statements)`: one transaction, and **every statement must change exactly one row**. Another count means the row is gone (deleted, or its key changed, since the page was read) or the key is not unique: `edit.row_gone`, and nothing is saved.

| Engine | How |
|---|---|
| SQLite | `BEGIN IMMEDIATE`, each statement's change count checked, `COMMIT`; `ROLLBACK` on anything else |
| PostgreSQL | `BEGIN` and simple queries, whose command tags carry the counts; `ROLLBACK` after any failure, a cancel or a deadline, so the session never stays in a transaction |
| SQL Server | one batch: `SET XACT_ABORT ON`, `BEGIN TRY ... BEGIN TRANSACTION`, each statement followed by `IF @@ROWCOUNT <> 1 THROW 50001`, `COMMIT`, and a `CATCH` that rolls back and throws again. tiberius does not report counts, so the batch checks them itself |
| D1 | one `batch` request, which D1 runs as one transaction; after each statement a guard `SELECT CASE WHEN changes() = 1 THEN 1 ELSE json('...') END` fails the batch (SQLite's `malformed JSON`) when the count is not one |
| Redis | not here: the console's commands edit keys |

After a save the page is read again, and counted again.

## Rationale

- The preview is the safety: the user reads exactly what runs, in the form they could run by hand.
- One row per statement, checked, is what makes a stale page safe: a row someone else deleted fails the save instead of being silently skipped.
- The statements are written once in the core, so every UI saves the same SQL, tested per engine against real servers (`edits_save_on_*`).

## Consequences

- The check is by key only: a row whose other columns changed since the page was read is overwritten. Comparing every column (optimistic concurrency) can come later, with a "the row changed" message.
- Copy and the row detail show the values as read, not the edits.
- A key of a type kept only as text and compared loosely (a `datetime` key on SQL Server with more digits than the type keeps, a float key) may not match its row; the save then fails with `edit.row_gone` and changes nothing.
- Saved edits are not in the query history yet.
- D1's guard relies on D1 running a REST batch as one transaction, as its documentation says for batches; no D1 account was at hand to try it.

## Alternatives considered

- **Run each change at once**, as a cell is left. No preview and no way back from a wrong edit.
- **Edit any query result.** The tables and keys behind an arbitrary `SELECT` are guesswork.
- **Name rows by every column's value** when there is no key. Floats, large text and NULLs make that unreliable, and duplicates change two rows.
