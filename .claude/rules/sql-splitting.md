---
paths:
  - "core/crates/core/src/sql/**/*"
---

# SQL splitting

- **Split the way the engine's own client does.** psql for PostgreSQL (and Neon, Supabase, PGlite), sqlcmd and SSMS for SQL Server, the sqlite3 shell for SQLite and D1. When unsure, check the client's source or run it, and cite what you checked in the test name or a comment.
- **Ranges are byte offsets** into the script, on char boundaries. Every byte of a multi-byte character counts as an identifier byte, so a scan never stops inside one.
- **The table in `split.rs`'s module docs is the spec.** A grammar change updates the table and adds a test per row it touches.
- **A statement that is only whitespace and comments is dropped.** A comment before a statement is not part of its range.
- **Unclosed quotes, comments and dollar quotes run to the end of the text.** The engine reports the error; the splitter never guesses where it ends.
- **Never add a grammar without a test** for each of: semicolons inside quotes, comments, the grammar's own quoting, and multi-byte text.
