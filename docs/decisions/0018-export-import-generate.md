# 0018: Export, Import and Generate Rows: jobs that stream, run in one transaction, and stop clean

- **Status**: Accepted
- **Date**: 2026-11-12
- **Follows**: `docs/decisions/0017-tree-actions.md` (the tree menu's items left "Later"). Decided with the user on 2026-11-12. Transfer To and Copy To stay in the backlog (`docs/roadmap.md`).

## Context

The tree menu offered Export, Import and Generate Rows, disabled. A table can hold millions of rows, so none of them may hold a table in memory, and a stop half way must leave nothing behind: no half file, no half import.

## Decision

- **Export** writes every row of a table, or the rows its open page's filter keeps (`sql::export_statement`: the table tab's WHERE, in key order, no page), to CSV, JSON or INSERT statements (`paboable_core::export::ExportWriter`). The rows stream: each batch the driver reads is written to the file and dropped, so memory stays flat whatever the table's size. The file is written as `<name>.part` beside the target and takes its name only when the export finishes; a stop or a failure deletes it. CSV is RFC 4180 (quotes only where needed, NULL as an empty field), with the column names first and a UTF-8 byte order mark by default, so Excel reads accents; JSON is an array of objects, numbers and booleans as JSON has them, a decimal's exact digits kept; INSERT is one statement a row for the engine the rows came from.
- **Import** reads a CSV file (UTF-8 or UTF-16, its separator found, `paboable_core::import::CsvReader`), into the table (fields matched to columns by name, case aside; a field with no column, and the columns the server fills, are left out) or into a new table, its column types taken from its values (`infer_shape`, `column_type`: a code with a leading zero stays text) and editable before it runs. The `CREATE TABLE` and the first `INSERT` show before anything runs. The rows go in as `INSERT`s of 500 rows (SQL Server takes at most 1,000 in a VALUES list), each field written for its column's type (a number bare only in a numeric column), all in one transaction: a stop or a failure rolls back every row, and a new table with them. D1 has no transaction, so there each batch stands alone. Files up to 512 MB.
- **Generate Rows** makes rows that fit each column's type and what its name says it holds (email, phone, name, city, address, code, …, cut to the column's length), leaves out the columns the server fills (`ColumnInfo::generated`: identity, serial, computed, rowversion, SQLite's rowid), and counts a single integer key up from one past its largest value. A seed makes the rows repeatable, so the five shown are the first five inserted. It runs like Import: batches of 500, one transaction.
- **Jobs.** The three run in the background with an id (`cancel_run` stops them) and report on a channel (`JobEvent`: progress at most ten times a second, then finished, cancelled or failed). Export is opened from the tree menu and from a table page's tools; with the page's filter on, it offers the filtered rows.

## Consequences

- Export reads the table in its key order; a table without a key comes in the server's order.
- An import into an existing table cannot remap fields by hand yet: a field whose name matches no column is left out, and the dialog says which.
- Generated values look real but are not: emails use example domains.

## Alternatives considered

- **Reading the whole result, then writing the file.** Simple, but a large table would sit in memory and the UI would wait for all of it.
- **A server's own bulk tools** (`COPY`, `BULK INSERT`, `bcp`). Faster, but they read files on the server's machine, not the user's, and differ per engine.
