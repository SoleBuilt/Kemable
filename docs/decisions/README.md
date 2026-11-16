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
| 0017 | [The object tree's menu: DDL, structure and actions shown before they run](0017-tree-actions.md) |
| 0018 | [Export, Import and Generate Rows: jobs that stream, run in one transaction, and stop clean](0018-export-import-generate.md) |
| 0019 | [One data folder, beside the app where it was installed; an installer that asks where](0019-data-folder.md) |
| 0020 | [A Live tab listens on a link of its own, and what it hears is a result set](0020-live-tab.md) |
| 0021 | [A blank line ends a statement when the next line starts a new one](0021-blank-line-ends-a-statement.md) |
| 0022 | [The app is Kemable, and starts with no data from Paboable](0022-rename-to-kemable.md) |
| 0023 | [AGPL-3.0-only and a commercial license; permissive dependencies only, their notices shipped with the app](0023-licensing.md) |
| 0024 | [An SSH tunnel is a local port the app opens; drivers only dial it](0024-ssh-tunnels.md) |
| 0025 | [A Supabase project's Live tab also listens through Realtime](0025-supabase-realtime.md) |
| 0026 | [RabbitMQ: an AMQP connection, a console that publishes, and Live tabs that watch or take](0026-rabbitmq.md) |
| 0027 | [Firebase: Firestore and the Realtime Database over REST, signed in with a service account's key file](0027-firebase.md) |
| 0028 | [The app updates itself from GitHub Releases, asked once at launch, with installers only CI signs](0028-updates.md) |
| 0029 | [SQL Server's TDS client is tabstream, Kemable's own fork of tiberius 0.13.0](0029-tabstream.md) |
| 0030 | [Kemable's website lives in `site/`: static Astro pages on Cloudflare Pages, with the Terms and the Privacy Policy](0030-website.md) |
| 0033 | [A kept password goes only where it was kept for; SSH host keys are each Windows user's own](0033-bound-passwords.md) |
