# 0021: A blank line ends a statement when the next line starts a new one

- **Status**: Accepted
- **Date**: 2026-11-12
- **Changes**: the first rule of `.claude/rules/sql-splitting.md` (split the way the engine's own client does), for PostgreSQL and SQLite. Decided with the user on 2026-11-12.

## Context

A script was split only at `;`, as psql and the sqlite3 shell do. Written without semicolons,

```sql
select 1

select * from public.bang_thu
```

is one statement to psql, and a syntax error. In Kemable, Run statement (Ctrl+Enter) with the cursor on the second line sent both lines and failed, and the user read it as the wrong statement being picked. DBeaver's default treats a blank line as the end of a statement, and people who come from it write scripts this way.

A blank line cannot always end a statement: people also put blank lines inside one, between a `WITH` and its `SELECT`, between the clauses of a long query.

## Decision

In the PostgreSQL and SQLite grammars, a blank line (a line holding only spaces) ends the statement before it when all of these hold. The table in `core/crates/core/src/sql/split.rs` is the spec.

1. **It is outside anything kept whole**: quotes, comments, dollar quotes, parentheses and `BEGIN ... END` bodies.
2. **The next line starts with a word that begins a statement**: `SELECT`, `WITH`, `INSERT`, `UPDATE`, `DELETE`, `MERGE`, `VALUES`, `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `GRANT`, `REVOKE`, `COMMENT`, `BEGIN`, `COMMIT`, `ROLLBACK`, `ABORT`, `SAVEPOINT`, `RELEASE`, `SHOW`, `EXPLAIN`, `ANALYZE`, `VACUUM`, `REINDEX`, `COPY`, `LISTEN`, `UNLISTEN`, `NOTIFY`, `CALL`, `LOCK`, `REFRESH`, `DECLARE`, `MOVE`, `CLOSE`, `PREPARE`, `DEALLOCATE`, `DISCARD`, `CHECKPOINT`, `IMPORT`, `REASSIGN`, and for SQLite `PRAGMA`, `ATTACH`, `DETACH`, `REPLACE`. Words that also continue a statement are left out, so they never split: `SET` (`UPDATE t SET`), `START` (`START WITH`), `RESET`, `FETCH` (`FETCH FIRST`), `EXECUTE` (`EXECUTE FUNCTION`), `DO` (`ON CONFLICT DO`), `TABLE`, `END`, `CLUSTER`, `SECURITY`.
3. **The statement before it is not waiting for more**:
   - its last token is not an operator, a comma or an opening parenthesis, and not a word that needs something after it (`AS`, `UNION`, `ALL`, `EXCEPT`, `INTERSECT`, `AND`, `OR`, `NOT`, `ON`, `IN`, `FROM`, `WHERE`, `BY`, `THEN`, `ELSE`, `DO`, `INSTEAD`, `FOR`, `TO`, `EXPLAIN`, `WITH`, ...);
   - a `WITH`, `INSERT`, `REPLACE` or `EXPLAIN` that has not reached its own query yet (no query word outside parentheses after its first word) keeps the `SELECT`, `VALUES`, `WITH`, `INSERT`, `UPDATE`, `DELETE` or `MERGE` after the blank line as that query. A `CREATE VIEW ... AS`, `DECLARE ... FOR` or `PREPARE ... AS` is covered by its last word;
   - an `ALTER` keeps an `ALTER COLUMN`, `ALTER CONSTRAINT`, `DROP COLUMN` or `DROP CONSTRAINT` after the blank line as its next action;
   - a `CREATE FUNCTION`, `PROCEDURE` or `TRIGGER` keeps a `BEGIN` after the blank line as its body.

A psql directive (`\c demo`) after a blank line is its own statement by the same rule.

Both Run statement and Run all use the split, so they agree on what a statement is.

SQL Server is unchanged: a batch between `GO` lines is sent whole, and T-SQL needs no `;` between its statements.

## Rationale

- When the rule is unsure it does not split, and the script behaves as it did before and as psql would. It never cuts a statement in two that psql would have run whole, except where the words above say a new statement began, which psql would have refused anyway.
- A list of words is easy to read in a test and to correct; a real parser of each engine's grammar is not something this app has or needs.

## Consequences

- A script that relied on psql's reading, a new statement after a blank line with no `;` and expecting an error, now runs both. Nobody writes that on purpose.
- A statement after a blank line that starts with a word left out of the list (`SET search_path = x`, `DO $$ ... $$`, `FETCH ...`) still needs a `;` before it.
- A blank line inside a statement, before a word of the list, splits it when the text before looks complete: `SELECT 1` blank line `UNION ...` is safe (it does not start with a listed word), but `CREATE SCHEMA s` blank line `CREATE TABLE ...` as one schema-with-elements statement becomes two. Both still run.

## Alternatives considered

- **Always split at a blank line** (DBeaver's "Always"). Breaks every query written with blank lines between its clauses or after its `WITH`.
- **Only for Run statement, leaving Run all at `;`**. The same text would be one statement or two depending on the key pressed.
- **Stay with psql**. Correct by the old rule, and a daily annoyance for a script without semicolons.
