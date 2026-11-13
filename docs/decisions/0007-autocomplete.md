# 0007: Autocomplete reads its context in the core and its names from a cached catalog

- **Status**: Accepted
- **Date**: 2026-11-09
- **Follows**: the usual parts of an autocomplete (a context analyzer, a catalog protocol, a provider); here they sit in the Rust core so every UI shares them.

## Context

The editor offered lang-sql's keywords and nothing from the database. Names (schemas, tables, views, columns, aliases) need two things: knowing where the cursor is in the statement, and the catalog. Both must stay fast while typing, and the first is the same work for every UI.

## Decision

### The core reads the context

`kemable_core::sql::completion_context(text, cursor, grammar)` returns, for the statement under the cursor (in SQL Server, the statement inside its batch, since T-SQL needs no `;` between statements: `kemable_core::sql::batch`, amended 2026-11-11, so the columns offered are those of the tables that statement names, not of every table in the batch):

- **The word being typed** and where it starts, so an item replaces it. Inside an open quoted name (`"Ord`, `[Ord`) the word starts after the quote and the item closes it.
- **The qualifier**: the names before the dots (`o.` → `o`, `sales.` → `sales`).
- **The clause**: `Start` (first word), `Table` (after `FROM`, `JOIN`, `UPDATE`, `INTO`, `TABLE`, or a comma in a `FROM` list), `AfterTable` (past a table: an alias or a keyword comes next), `Expression` (select list, `WHERE`, `ON`, `GROUP BY`, `ORDER BY`, `SET`, `HAVING`, function arguments), `InsertColumns(t)` (`INSERT INTO t (`), `Other`. It is the clause word that rules the cursor's parenthesis level; a level with none (a function call, an `IN (` list) takes the clause outside it.
- **The tables in scope**, at any depth of the statement, with their aliases. A name still being typed is not one of them. Derived tables are skipped: their columns are not in the catalog.
- `None` inside a string or a comment.

It lexes text that is still being written: the tolerant reading in `sql/lex.rs`, which the sort's strict reading shares, so both follow the splitter's quoting rules.

### The lookups are what the context needs, no more

`CompletionContext::lookups(schemas)` names what to read: the tables of one schema (the default one, or the qualifier), or the columns of the tables in scope (an alias resolves to its table; a name without a schema is in the default one). `completions(context, catalog, grammar)` builds the items from what was found.

### The app caches the catalog

`ui/windows/src-tauri/src/catalog.rs` keeps, per open server and database, the schemas, each schema's objects and each table's columns, read through the session with a 1.5-second deadline. A lookup that fails or is late gives nothing and is not kept. The UI forgets a server's catalog after a run that changed the schema (the same signal that refreshes the tree) and on Ctrl+R; closing the server forgets it too.

### The editor asks as a word starts

- The completion source sits beside lang-sql's keywords. It asks Rust only when a word starts: `validFor` keeps the list while the word goes on, and CodeMirror filters it.
- It opens by itself after a letter, a dot or an opening quote; Ctrl+Space opens it anywhere.
- No names in `Start`, `AfterTable` and `Other`: keywords only. Columns rank before tables, tables before schemas and aliases, all before keywords.
- A name is written as typed when it reads the same bare, else quoted for the grammar: PostgreSQL quotes any upper-case letter (it folds bare names to lower case), SQL Server uses brackets; reserved words are always quoted.
- A text longer than 2,000,000 characters gets keywords only, so a large script never sends megabytes per word.

## Rationale

- One context reader in the core for every UI, tested on its own (`core/crates/core/src/sql/completion.rs`), and checked against the real catalogs of SQLite, PostgreSQL and SQL Server (`completion_reads_the_*_catalog` in the app's tests).
- Asking per word, not per key, keeps the calls to a handful per line; the cache makes most of them memory reads.
- Lazy lookups scale to a database with thousands of tables: nothing reads every column up front.

## Consequences

- A table reached through PostgreSQL's `search_path` but not in the default schema gets no columns unless its schema is written.
- A column of a CTE or a derived table is not offered.
- While a long run holds a SQL Server session, a lookup waits for it and gives up after 1.5 seconds; the names come once the cache holds them.
- A comma after `JOIN ... ON` that continues the `FROM` list is not read as one.

## Alternatives considered

- **lang-sql's own schema completion**, fed with every table and column up front. Needs the whole catalog before the first word and cannot follow aliases across a statement.
- **The context in TypeScript.** It would be written again for the Apple and Linux UIs.
- **Every column of the database read at connect.** Slow on large databases and stale after the first `ALTER`.
