# 0013: The work area holds pages: query tabs and table tabs, each with its own tools on one bar

- **Status**: Accepted
- **Date**: 2026-11-11
- **Follows**: `docs/decisions/0012-design-system.md` (decisions D13 to D25 of the design system). **Changes**: how `docs/decisions/0006-table-browsing.md` shows a table. It now opens on a page of its own, not as a tab in the results.

## Context

The first window had one editor and one row of result tabs. A table opened from the tree became a result tab under the editor that did not belong to it. The run bar, the table bar and the toolbar each held part of the tools, and the toolbar had a button for each way to connect. In the prototype, the user asked for several queries open at once, for a table on a page of its own, and for a toolbar that only says where you are.

## Decision

### The toolbar says where you are

- **Left:** the connection, as a combobox. Its menu shows the connection and offers Edit this connection… and Disconnect. Next to it is the database picker, for an engine with databases.
- **Middle:** Search, which opens the command palette (Ctrl+K). The palette holds the ways to start: New query tab, New connection…, Open file…, in-memory, every saved connection, and Settings.
- **The Welcome screen** (nothing open) has no sidebar and shows only Search in the toolbar. Its empty state offers New connection…, Open file… and an in-memory database.

### Pages

- **Query pages** (Query 1, Query 2, …). Each keeps its own editor state (text, cursor, undo), its results, its messages and its run counts while another page is shown. There is one CodeMirror view; a page's state is swapped in with `saveState` / `restoreState`.
- **Table pages.** Opening a table or view from the tree opens it on a page of its own, with no editor: its filter row, its grid and the pager in the status bar (ADR 0006). Opening a table that is already open shows its page. Open in New Tab, in the tree menu, opens another page.
- **The tabs.** Tabs sit on the left of a 40 px bar and scroll sideways. Each has a ×; a table tab shows its row count. `+` and Ctrl+T open a query page, Ctrl+W closes the page shown, and Ctrl+Tab and Ctrl+Shift+Tab move between pages. Closing the last query page opens a new empty one, so there is always an editor.
- **While a run is going,** its page stays shown: switching or closing a page asks to wait or Stop first. A table page with unsaved edits does not close (ADR 0010).

### The tools of the page shown sit on the right of the same bar

- **A query page:** Run ▾ (Run statement, Run all), which turns into Stop while it runs. Then the row limit, as a combobox, and Save query. "Run without limit" is gone, since the limit offers No limit; Ctrl+Alt+Enter still runs without a limit.
- **A table page:** Filter, Row, Delete rows and Copy SQL, and Read-only when the table has no key. While edits are pending: their summary, Discard, and Preview & save.

### A query page shows results only after a run

- A new query page is all editor. After the first run, the results appear under a splitter. Drag it, use the arrow keys, or double-click it to go back to 188 px.
- The result bar holds the result tabs (each with its row count, or a cap badge), Messages, Filter (ADR 0011) and Row detail.
- With two results or more, the editor frames the statement behind the result shown.

### The sidebar is panes

- Objects (Keys for a key-value engine) fills the sidebar. Its header holds Refresh (Ctrl+R).
- Queries and History fold under it, keep their order, and show their counts. A key-value engine has Keys and History.
- A right click on the tree opens its menu, with the items a desktop database client has. The items not built yet show disabled, marked "Later".

## Rationale

- Pages let the user keep several queries and tables open without losing one to see another. A table's page has the room its pager and filters need.
- The tools next to the tabs change with the page, so no bar shows tools that do nothing on the page shown.
- With one CodeMirror view and swapped states, a hidden page costs no DOM and no editor of its own.

## Consequences

- A page shown keeps its state in the App's live variables; a hidden page keeps it in its `PageState`. A new field of a page's state goes into `stash` and `unstash` in `App.svelte`.
- Pages are not saved across launches yet.
- Still to build, and shown disabled or left out for now (`docs/roadmap.md`):
  - in the tree menu: DDL, structure, export and import, truncate and drop;
  - Test connection;
  - Profile;
  - Ctrl+click to pick rows one by one in the grid;
  - connection colours;
  - opening and saving `.sql` files;
  - a tab bar for Redis.

## Alternatives considered

- **One editor with result tabs, as before:** a table under an editor that does not belong to it, and no second query.
- **A CodeMirror view per page:** simpler swapping, but a hidden editor would keep its DOM and its listeners.
- **Tools in a bar of their own over the editor:** a third bar that changes with every page, and the user asked for one.
