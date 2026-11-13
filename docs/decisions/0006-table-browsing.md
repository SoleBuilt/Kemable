# 0006: A table tab reads one page at a time, filtered, sorted and counted on the server

- **Status**: Accepted; amended by `docs/decisions/0013-pages-and-tabs.md` (a table opens on a page of its own, its pager in the status bar)
- **Date**: 2026-11-09
- **Follows**: `docs/decisions/0005-sorting-a-result.md` (a table tab: the server sorts).

## Context

Opening a table from the tree ran `select * from t` up to the row cap. On a large table that reads up to 500,000 rows to show the first screen. A filter or a sort then applies only to the rows loaded, which ADR 0005 already ruled wrong for a table.

## Decision

### The statements are the core's

`kemable_core::sql::page_statement(grammar, query)` and `count_statement(grammar, query)` write them from a `BrowseQuery`: the table, its filters, how they combine (all or any), the sort, the primary key, the offset and the page size.

- **Names** (table, schema, columns) come from the catalog and are quoted for the grammar.
- **Values** are literals with their quotes doubled (`N'...'` on SQL Server), so no value can end its literal. On SQLite, a value that is a plain number (no leading zero, no exponent) is written bare, because SQLite compares by the literal's type; PostgreSQL and SQL Server convert a quoted literal to the column's type and refuse one that does not fit.
- **Contains, starts with, ends with** escape `%`, `_` and `\` (and `[` on SQL Server) and say `ESCAPE '\'`. They ignore case: `ILIKE` on PostgreSQL with `::text`, so a number or a date can be searched too; SQLite's `LIKE` ignores ASCII case; SQL Server's follows the column's collation, which ignores case by default.
- **A SQL condition** is the user's own text, written as typed in parentheses and on its own line, so a trailing `--` comment cannot swallow the rest. It must be one expression: one that splits into two statements or holds a `;` is refused with `config.invalid`.
- Operators: `=`, `≠`, contains, does not contain, starts with, ends with, `>`, `≥`, `<`, `≤`, in (a comma-separated list), between, is NULL, is not NULL.

### Paging

- `LIMIT n OFFSET m` on PostgreSQL and SQLite; `OFFSET m ROWS FETCH NEXT n ROWS ONLY` on SQL Server, which needs an `ORDER BY`, so a table with no key and no sort orders by `(SELECT NULL)`.
- **The primary key orders the page**, after the user's sort keys. Without it, PostgreSQL may return rows in a different order between two pages (synchronized sequential scans start mid-table), and ties in a sort spread across pages, so a row can show twice or never. With it, each row is on one page.
- Page sizes: 100, 500, 1,000 (the default), 5,000 and 10,000, remembered across launches. No row cap applies to a page.
- A header click sorts the whole table and goes back to the first page. A filter change goes back to the first page and counts again.

### Counting

- After the first page of a new filter, the app counts the rows it matches with `count_statement`, on the same session, with a 30-second deadline. A first page that is not full needs no count: the table holds what it shows.
- The pager says "1–1,000 of …" while counting and "of 12,345" once done. A count that fails or runs out of time leaves the total unknown, with a Count button to try again; the pager still goes forward while a page is full.
- A count started for older filters is dropped when it returns.

### What the tab does not do

- It is not recorded in the query history: the app wrote the statement (ADR 0005, interaction). Copy SQL puts the page's statement on the clipboard for the editor.
- A run from the editor closes the table tab, as before.

## Rationale

- One page is what the screen shows; the server is the only place a filter or sort over the whole table is right.
- The core writes the statements so every UI pages and filters the same way, with the quoting tested once (`core/crates/core/src/sql/browse.rs`), and checked against PostgreSQL, SQL Server and SQLite in the app's live tests (`a_table_tab_pages_and_filters_on_*`).
- Ordering by the key costs little: with `LIMIT`, an index scan on the key reads only the page.

## Consequences

- A deep page reads and skips every row before it (`OFFSET 1,000,000` reads a million rows). Paging by key (`WHERE id > last`) would avoid it, but cannot jump to a page number or the last page; it can come later for "next page" alone.
- A count reads every matching row. PostgreSQL's `COUNT(*)` on a large table can take seconds; the deadline bounds it, and SQL Server waits for it before the next statement on that session.
- A view has no primary key, so its pages are ordered only by the sort, or not at all.
- When filters cannot be read (a bad value for the column's type), the page fails like any statement and the error shows in Messages; the filters stay in the bar to be fixed.

## Alternatives considered

- **Load every row and filter in the core.** Fast once loaded, but the load is the cost this avoids, and a filter over rows not loaded is wrong.
- **Bind parameters instead of literals.** Safer in general, but the driver contract runs text; the literals here are escaped by one tested function and the user can copy the statement and run it as is.
- **An estimated count** (`pg_class.reltuples`, `sys.partitions`). Instant, but wrong after bulk changes and useless with a filter. It may be shown while the exact count runs, later.
