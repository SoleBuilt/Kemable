# 0016: A query page can be a `.sql` file

- **Status**: Accepted
- **Date**: 2026-11-11
- **Follows**: `docs/decisions/0013-pages-and-tabs.md` (pages), the UX backlog's "Open and save `.sql` files" (`docs/roadmap.md`), decided with the user on 2026-11-11.

## Context

Scripts live in files: in git, shared by a team, opened from Explorer. Paboable kept a query page's text only in the app (pages across launches) or as a saved query in `queries.json`. SSMS and other desktop clients open a `.sql` file into a tab and save it back with Ctrl+S.

## Decision

- **A query page may be bound to a file.** Its tab shows the file's name, with a `●` while its text differs from what was last read or written. A page that is not bound stays "Query n".
- **Open.** Ctrl+O and Open file… show one dialog that takes both kinds: a `.sql` (or any text) file opens on a new query page; a SQLite file (`.db`, `.sqlite`, …) opens as a connection, as before. Recent files are in Ctrl+K.
- **Opening without a connection.** The file opens on a query page with no connection: it can be read and edited, and Run asks for a connection first. When one opens, the file's pages move into that connection's pages.
- **Save.** Ctrl+S on a query page writes its file; on a page not bound to one it asks where (Save As) and binds the page. Ctrl+Shift+S is Save As. On a table page Ctrl+S still saves the table's edits (ADR 0010). Saved queries (Ctrl+Shift+D) are unchanged.
- **Close.** Closing a page whose text is not saved asks Save, Don't save or Cancel. Closing the window does not ask: the page's text is kept with the pages (ADR 0013), so it comes back unsaved, with its `●`.
- **Text as the file has it.** The core reads the bytes: UTF-8 with or without a BOM, and UTF-16 (SSMS's older default), and the line endings. A save writes the same encoding and line endings back. A new file is UTF-8 without a BOM, with CRLF.
- **Across launches.** A bound page keeps its path. Opening the connection again reads the file again; when the kept text differs from the file, the kept text wins and the page shows `●`.

## Consequences

- Built 2026-11-11: `paboable_core::text_file` decodes and encodes (tested with BOMs, UTF-16 and mixed line endings; bytes that are not text are refused with `file.not_text`, never guessed); the Windows app's `read_text_file` and `write_text_file` read and write (64 MB at most, a save through a temporary file and a rename), and the dialog plugin's `save` asks for a path (`dialog:allow-save`). In `App.svelte` a page's `file` holds the path, the encoding, the line endings and the text last saved; the tab's `●` compares the editor's text with it.
- Backlog (not decided further): a notice when a file changes outside the app (Reload from disk, or keep my changes), and linking a folder of `.sql` files into the sidebar.

## Alternatives considered

- **Ctrl+S saves into Saved queries.** Saved queries already have Ctrl+Shift+D; Ctrl+S meaning "the file" matches every editor.
- **Refusing to open a `.sql` without a connection.** Simpler, but the user chose to open it and pick the connection after (2026-11-11).
