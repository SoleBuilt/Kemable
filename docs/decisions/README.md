# Decisions

One file per decision that shapes more than one file, numbered in order: `NNNN-short-name.md`.

```markdown
# NNNN: The decision, as one sentence

- **Status**: Proposed | Accepted | Superseded by NNNN
- **Date**: YYYY-MM-DD

## Context
What forces the choice. Facts and measurements, with how they were measured.

## Decision
What we do.

## Rationale
Why this over the alternatives.

## Consequences
What follows, good and bad, and what stays open.

## Alternatives considered
Each, and why not.
```

A decision changes by a new file that supersedes it; the old one gets its status updated and stays.

| # | Decision |
|---|---|
| 0001 | [One Rust core, one UI per operating system](0001-shared-core-and-ui-per-os.md) |
| 0002 | [The core owns result sets; a grid pulls packed windows](0002-result-windows.md) |
| 0003 | [Tauri 2 and Svelte 5 for the Windows app](0003-tauri-svelte-for-windows.md) |
| 0004 | [How each engine is reached](0004-engines.md) |
| 0005 | [Sorting a result: run again with ORDER BY, else sort the rows held](0005-sorting-a-result.md) |
| 0006 | [A table tab reads one page at a time, filtered, sorted and counted on the server](0006-table-browsing.md) |
| 0007 | [Autocomplete reads its context in the core and its names from a cached catalog](0007-autocomplete.md) |
| 0008 | [Key-value engines: a console of commands, keys by SCAN, a key read in steps](0008-key-value-engines.md) |
| 0009 | [Cloudflare D1 through its REST API, one request per statement](0009-d1-over-http.md) |
| 0010 | [Editing rows: statements shown before they run, saved all or none](0010-editing-rows.md) |
| 0011 | [A query result is filtered on the rows it holds; its WHERE is shown to copy](0011-filtering-a-result.md) |
| 0012 | [One design system: black and white, Geist, Lucide, tokens and pb-* classes](0012-design-system.md) |
| 0013 | [The work area holds pages: query tabs and table tabs, each with its tools on one bar](0013-pages-and-tabs.md) |
| 0014 | [Small components in lib/ui; pages only put them together; the window draws its own title bar](0014-ui-components.md) |
| 0015 | [Credential profiles: a user and a password under a name, shared by saved connections](0015-credential-profiles.md) |
| 0016 | [A query page can be a `.sql` file](0016-sql-files.md) |
