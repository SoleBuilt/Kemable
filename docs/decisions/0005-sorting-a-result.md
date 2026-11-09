# 0005: A sort runs the statement again with ORDER BY when it can name the column, else the core sorts the rows held

- **Status**: Accepted
- **Date**: 2026-11-09
- **Follows**: `docs/decisions/0002-result-windows.md` (result sets held in the core).

## Context

A grid sorts in two places:

- **A table tab**, opened from the tree, browses a table that may be far bigger than the rows loaded. Sorting the rows loaded would show the wrong "first" rows.
- **A query result**, from a statement the user wrote. It may hold every row, or stop at the row cap the run named (the toolbar's Limit).

The two ways to sort, compared (2026-11-09):

| | Run the statement again with `ORDER BY` | Sort the rows the result holds |
|---|---|---|
| Rows | The latest: a row inserted since shows up | The ones on screen |
| A result cut at the cap | Correct over every row | Only the rows loaded |
| Text order | The database's collation, as a hand-written `ORDER BY` gives | Paboable's rules (below) |
| Cost | The statement's whole cost again, on the server | In memory, in the core |
| Statements that cannot run again | Impossible: `EXEC`, `INSERT ... RETURNING`, scripts, `SHOW`, `EXPLAIN`, temp tables gone | Always possible |

Naming the column in an `ORDER BY` is not always safe:

- A join with two `id` columns makes `ORDER BY "id"` ambiguous.
- SQLite reads a double-quoted word that names no column as a string literal, so `ORDER BY "count(*)"` sorts by a constant with no error.
- Wrapping the statement, `SELECT * FROM (...) s ORDER BY ...`, is refused by SQL Server (and MySQL) when two columns share a name.
- A position, `ORDER BY 2`, is read by PostgreSQL, SQLite and SQL Server (SQL Server advises against it but supports it). MySQL deprecates it and ClickHouse may ignore it; neither is an engine here yet.

## Decision

### A table tab: the server sorts

Opening a table runs `SELECT * FROM <schema>.<table>` written by the core, and a header click writes it again with `ORDER BY <quoted column>` and the page (`LIMIT`/`OFFSET` on PostgreSQL and SQLite, `OFFSET ... FETCH` on SQL Server). The column names come from the catalog, so they are always safe. A sort shows the table as it is now.

Paging, filters and the count are in `docs/decisions/0006-table-browsing.md`: the page statement carries the sort, with the primary key after it to break ties, and a sort goes back to the first page.

### A query result: run again when the column can be named, else sort the rows held

`paboable_core::sql::sorted_statement(statement, grammar, columns, key)` returns the statement to run again, or `None`:

1. **Only a statement that can run again**: one statement, a plain `SELECT` or `WITH ... SELECT`. Not `SELECT ... INTO`, not a directive, not `EXEC`, `SHOW`, `EXPLAIN`, DML with `RETURNING`/`OUTPUT`, nor anything in a script of several statements.
2. **Name the column** when its name is unique among the result's columns (case folded) and its select item is an unqualified column, an alias, or part of a bare `*`.
3. **Else give its position**, on PostgreSQL, SQLite (and D1, PGlite) and SQL Server.
4. **Else `None`.**

The `ORDER BY` replaces the statement's own top-level one, and goes before `LIMIT`/`OFFSET`/`FETCH`, so `... LIMIT 100` sorted by price returns the 100 cheapest rows, not the first 100 reordered.

When `sorted_statement` gives `None`, the core sorts the rows the result holds: `ResultStore::sorted(key)` builds a row order (a permutation over the same chunks; nothing is copied) and windows read through it. When the result stopped at the row cap, the status bar says the sort covers only the rows loaded.

### Order of the rows held

The one ordering for every UI, in the core (`paboable_store::order`, which reads the store's cells directly):

- NULL first ascending, last descending (descending is the comparison reversed). Equal values keep their arrival order: the sort is stable.
- Each value orders by its own kind. Numbers compare exactly across integer, float and decimal kinds, NaN after every number. Dates and timestamps compare by instant. UUIDs and bytes compare by content.
- A column that holds several kinds (SQLite) orders by kind first: NULL, numbers, text, bytes.
- Text compares by code point with runs of digits by their value (`item2` before `item10`), case counting. No locale collation: it would not match the database's anyway, and the server-side path covers the cases where the database's order matters.
- Kinds kept only as text (arrays, ranges, network addresses, `Other`) order by that text.

### Interaction

- A header click cycles ascending, descending, unsorted; the header shows the arrow, and the key's number when there are several. Shift+click adds a key. Ctrl+click selects the column instead.
- Unsorted shows the result as it arrived: for a re-run, the original statement runs again.
- A sort that runs the statement again goes through the same run as Ctrl+Enter, with its events, its Stop and its row cap. It is not added to the query history as a new query.
- While a table tab has unsaved edits, a sort that would read the page again is refused until they are saved or discarded (`docs/decisions/0010-editing-rows.md`).
- Fresh data is never a side effect of a client sort: to see rows inserted since, run the statement again (Ctrl+Enter, F5).

## Rationale

- Each path is right where the other is wrong: the server is right over rows not loaded and in the database's collation; the core is right for statements that cannot run again and costs nothing.
- Naming first, position second, never a guess: a wrong `ORDER BY` either fails or, worse, silently sorts by a constant.
- One ordering in the core, so every UI (Windows now, Apple and Linux later) sorts the same, exactly past `i64` and `f64`.

## Consequences

- The core grows `sql::sorted_statement` (reading the select list and the top-level clauses per grammar, with tests for each rule above), and the store `order` and a row order that windows, copies and the detail pane read through (`ResultStore::sort`; `docs/decisions/0002-result-windows.md`). The window format does not change: a sorted window is still rows in order.
- Each result set records where it came from (`ResultOrigin` in the Windows app: statement, database, row cap, and whether the run held that statement alone), which `sort_result` reads to choose the path.
- A query result sorted by re-running may show different rows than before: new ones, gone ones, the top N by the new key. That is the point of the server path, and the reason a client sort never re-runs.
- Text sorted in the core and text sorted by the server may differ for accented or mixed-case words. The status bar names which path sorted.
- Rows that arrive after a client sort (a result still streaming) follow in arrival order until the sort is applied again.

## Alternatives considered

- **Always run the statement again.** Fails for every statement that cannot run again, reruns expensive reports on each click, and changes the data under the user.
- **Always sort the rows held.** Wrong for a table tab and for any result cut at the cap.
- **Wrap the statement in a subquery.** SQL Server refuses a derived table with two columns of one name, the case that most needs care.
- **Always a position.** Unreadable in the history, and wrong on engines that ignore it (none here yet).
- **A locale collation in the core.** It would still not be the database's, and it adds a large dependency.
