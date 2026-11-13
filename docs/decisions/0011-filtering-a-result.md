# 0011: A query result is filtered on the rows it holds; the filter is shown as a WHERE to copy, never written into the statement

- **Status**: Accepted
- **Date**: 2026-11-10
- **Amends**: `docs/decisions/0005-sorting-a-result.md` (a sort while a filter is on). **Follows**: `docs/decisions/0006-table-browsing.md` (a table tab's filters) and `docs/decisions/0010-editing-rows.md` (a query result is read-only).

## Context

A table tab already filters on the server (ADR 0006). A query result has no filter: to narrow 1,000 rows down to the 120 that matter, the user edits their SQL and runs it again.

There are two places a query result could be filtered:

| | Write the filter into the statement and run it again | Filter the rows the result holds |
|---|---|---|
| Rows | Every row the server has | Only the rows loaded: a result cut at the row limit is filtered on that part |
| Wrapping as `SELECT * FROM (...) s WHERE ...` | SQL Server refuses it when two columns share a name; a column that is an expression has no name to filter on | Not needed |
| Adding to the statement's own `WHERE` | Needs the statement parsed, and an aggregate or an alias belongs in `HAVING` or an outer query instead | Not needed |
| Statements that cannot run again (`EXEC`, `INSERT ... RETURNING`, scripts) | Impossible | Always possible |
| Cost | The statement's whole cost again, for every change to the filter | In memory, in the core |
| Who owns the SQL | Kemable changes the user's statement | The statement stays exactly as the user wrote it |

The user's statement is theirs. Kemable does not change it.

## Decision

### Filtering runs on the rows held, in the core

- A filter on a query result keeps the rows the result holds that match, in the core: `ResultStore` builds a row order (the matching rows, a list over the same chunks, nothing copied), as `ResultStore::sorted` does for a sort (ADR 0005). Windows read through it; the grid only paints.
- The conditions are the table tab's: the same filter row in the UI, the same operators (`equals` through `isNotNull`, `in`, `between`), match all or any. `raw` is not offered: it is SQL, and only a server can read it.
- A value compares by the column's kind, with the rules of ADR 0005's "Order of the rows held": numbers exactly, dates and timestamps by instant, text by code point. `contains`, `startsWith` and `endsWith` ignore case.
- Each result tab (Result 1, Result 2, ...) has its own filter.

### The WHERE is shown, never run

- The filter row shows the condition as a `WHERE` clause written by the core for the connection's engine, with the quoting the table tab uses (`core/crates/core/src/sql/browse.rs`), and a Copy button. The user adds it to their own statement if they want the server to filter.
- A condition on a column the statement does not name plainly (an expression, or a name two columns share) cannot be written: the clause leaves it out and says so.

### A filter stays until the user clears it

- Running the statement again keeps the filter and applies it to the new rows. Only Clear filter removes it.
- **While a filter is on, a sort sorts the rows held** and does not run the statement again, even where ADR 0005 would run it with `ORDER BY`. The order is: filter, then sort, both on the rows held. With no filter, ADR 0005 is unchanged.

### The status bar says what is shown

- The status bar at the bottom of the window says "120 of 1,000 rows held" while a filter is on.
- When the result stopped at the row limit, it also says that the filter covers only the rows loaded, next to Fetch all.

### A result stays read-only; its table opens to edit

- A query result is never editable (ADR 0010). When the core can name exactly one table behind it (one plain `SELECT` from one table, with no join, no `GROUP BY` and no set operation), the result bar shows "Open <table> to edit", which opens that table's tab as the object tree does: the whole table, not only the rows of the result.
- When the core cannot be sure, the button does not show.
- Amended 2026-11-11, with the user: the core reads the statement (not the driver) to name the table. The statement's `WHERE` comes along as the table tab's filter when every condition is one the filter row can hold (a column, a comparison and a value, joined all by `AND` or all by `OR`); otherwise the whole table opens and the status bar says the `WHERE` was left behind. Asking the driver which table each column comes from (PostgreSQL's table OID, SQLite's column metadata, SQL Server's `sp_describe_first_result_set`) is in the backlog, optional.

## Rationale

- The statement stays the user's: no wrapping and no rewriting means no SQL Server refusals, no wrong `HAVING`, no statement that cannot run again.
- A filter in the core is right for every statement, costs no round trip, and filters the same way in every UI.
- Showing the `WHERE` gives the server-side filter to anyone who wants it, written correctly for the engine, without Kemable running SQL the user did not write.
- A filter that survives a re-run matches how the user works: change the SQL, run, keep looking at the same slice.
- Sorting the rows held while a filter is on keeps one rule for what the grid shows: the filtered rows, in an order the user can read, without a run that would load a different set under the same filter.

## Consequences

- On a result cut at the row limit, the filter shows only the matching rows among those loaded. The status bar says so; Fetch all, or the copied `WHERE` in the statement, gives every matching row.
- A sort with a filter on does not see rows the server has but the result does not hold. Clearing the filter brings back ADR 0005's run with `ORDER BY`.
- Copy and the row detail read through the filter: Ctrl+C copies the filtered rows, and selecting all selects the rows the filter keeps.
- Built in the core: `ResultStore::filter` (`core/crates/store/src/filter.rs`, with how each kind reads the user's value, and `store.rs`) keeps the matching rows as a row order the sort then orders, tests rows that arrive later as they arrive, and windows, copies and row reads go through it; the window's row count is the rows shown. `sql::result_where` (`core/crates/core/src/sql/result_filter.rs`) writes the `WHERE` and names the conditions it left out. The Windows app's `filter_result`, `result_counts` and `result_where` commands carry them (`ui/windows/src-tauri/src/commands.rs`), and `sort_result` sorts the rows held while a filter is on. A SQL condition is refused with `filter.raw`.
- In the Windows app, Filter on the result bar opens the filter row (no SQL condition), the WHERE strip under it copies the clause, and the status bar says "120 of 1,000 rows held" (`ui/windows/src/App.svelte`). A run keeps each result tab's filter for the set in the same place. While a filtered set streams, the app asks `result_counts`, since run events count the rows held.
- "Open <table> to edit", built 2026-11-11: `sql::result_source` (`core/crates/core/src/sql/result_source.rs`) reads a plain `SELECT ... FROM <table>` (an alias, SQL Server's table hints, `TOP`, `ORDER BY` and a three-part name allowed) and turns its `WHERE` into filters: `=`, `<>`, `<`, `<=`, `>`, `>=` with a literal, `LIKE` as contains, starts with or ends with, `IN` and `BETWEEN` with literals, `IS [NOT] NULL`. In a SQL Server batch it reads the statement that made the set (ADR 0013). The Windows app asks `result_source` once a result finishes and shows the button on the result bar; the filters are matched to the table's columns without case, and when one names no column the table opens whole and the status bar says so.

## Alternatives considered

- **Wrap the statement and run it again with the filter** (`SELECT * FROM (...) s WHERE ...`), falling back to the rows held when it cannot be wrapped: right over rows not loaded, but it changes the user's SQL, costs a full run on every filter change, and fails on SQL Server with duplicate column names.
- **Add the conditions to the statement's own `WHERE`**: needs the statement parsed and fails on aggregates and aliases.
- **Filter in the UI**: every UI would filter differently, and the UI would hold rows the core already holds (`AGENTS.md`, principle 2).
- **Edit a query result in place**: see ADR 0010's alternatives.
