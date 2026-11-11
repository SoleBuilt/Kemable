# 0017: The object tree's menu: DDL, structure and actions shown before they run

- **Status**: Accepted
- **Date**: 2026-11-11
- **Follows**: `docs/decisions/0013-pages-and-tabs.md` (the tree menu, its items marked "Later"), `docs/decisions/0010-editing-rows.md` (statements shown before they run). Asked by the user on 2026-11-11.

## Context

The tree's right-click menu listed the items a desktop database client has, most of them disabled. The user asked for them. Some only read (DDL, structure); some change the database (rename, truncate, drop, maintenance, comments); some move data and need their own design (export, import, transfer, copy to, generate rows).

## Decision

- **Reading.** A driver scripts an object (`Connection::object_ddl`) and reads its comment (`Connection::object_comment`). SQLite and D1 give the `CREATE` statements they keep, with a table's indexes and triggers. PostgreSQL builds a table's DDL from the catalog with the server's own functions (`format_type`, `pg_get_constraintdef`, `pg_get_indexdef`, …) and gives views and routines as the server writes them. SQL Server scripts a table from `sys.*` and gives views, procedures and functions with `OBJECT_DEFINITION`, its statements separated by `GO` lines, as SSMS scripts them.
- **Show DDL** opens the DDL on a new query page; **Copy DDL** copies it; **Show Structure** shows the columns and the DDL in a dialog; **Edit View Definition** opens the statement that replaces the view (`CREATE OR REPLACE VIEW`, `CREATE OR ALTER VIEW`, or SQLite's `DROP VIEW` then `CREATE VIEW`) on a query page, to change and run.
- **Changing.** Rename (F2), Edit Comment, Maintenance, Truncate (Alt+Del) and Drop (Del) open a dialog that shows the statements the core writes for the engine (`sql::action_statements`) and runs them only when asked. The core refuses what an engine has no statement for (SQLite comments, renaming a PostgreSQL routine, which is named with its argument types), and the menu leaves those items out. Maintenance offers each engine's own tasks (`sql::maintenance_tasks`): PostgreSQL's VACUUM, ANALYZE, REINDEX and VACUUM FULL; SQL Server's UPDATE STATISTICS, REORGANIZE, REBUILD and DBCC CHECKTABLE; SQLite's ANALYZE, REINDEX and VACUUM. The statements run one by one, outside a transaction (VACUUM cannot run inside one), with a 30-minute deadline, and the tree refreshes after.
- **Later, to decide with the user:** Export, Import, Transfer To, Copy To and Generate Rows stay in the menu, disabled.

## Consequences

- Truncate on SQLite is `DELETE FROM`, since SQLite has no TRUNCATE; the dialog shows it.
- A drop is not undone and asks nothing more than the dialog: it names the object, says it cannot be undone, and shows the statement. PostgreSQL offers CASCADE as a checkbox.
- A page showing a table that was renamed or dropped stays open on its old name until closed.

## Alternatives considered

- **Running the action at once, with a yes/no question.** Faster, but the user could not see what runs; ADR 0010 already shows statements before they run.
- **Scripting tables in the UI from the columns it has.** It would miss keys, constraints and indexes, and every UI would repeat it; the driver knows its catalog.
