# 0022: The app is Kemable, and starts with no data from Paboable

- **Status**: Accepted
- **Date**: 2026-11-13
- **Amends**: `docs/decisions/0019-data-folder.md` (the identifiers it names, and its move of the WebView's old storage). Decided with the user on 2026-11-13.

## Context

The project was called Paboable and lived in a GitLab repository. It moved to GitHub, under the SoleBuilt organization, with a new name, Kemable, and its history kept. The name reaches past the code (`ui/windows/AGENTS.md`, Renaming the app): the identifiers `paboable.app` and `dev.paboable.app` named the `%APPDATA%` folders, and Credential Manager kept each password under a target ending `.Paboable`. Only the user had ever run the app.

## Decision

- **Everything is renamed**: crates (`kemable-core`, `kemable-driver-<engine>`, ...), the app's name, window title and installer, the identifiers (`kemable.app`, `dev.kemable.app`), the Credential Manager suffix (`.Kemable`), the environment variables of the server tests (`KEMABLE_POSTGRES_URL`, ...), the docs.
- **Nothing is carried over from Paboable.** Kemable starts with an empty data folder and no saved passwords. The user deleted Paboable's folders and passwords by hand and sets up the connections again.
- **The WebView's old storage is no longer read.** ADR 0019 moved the keys a version before it kept in `%LOCALAPPDATA%\<identifier>\EBWebView` into files. Under the new identifier that folder is never there, so the move is removed: `import_local_storage`, `webview_for_launch`, and the old keys in `ui/windows/src/lib/store.ts`. The WebView always uses `data\webview`.

## Rationale

With one user, code that carries old data over costs more than setting up a few connections again, and it would stay in the app long after it is needed.

## Consequences

- A machine that ran Paboable keeps its old folders and passwords until someone deletes them; Kemable never reads them.
- The server tests' environment variables changed names: set `KEMABLE_*` instead of `PABOABLE_*`.
- `DataFolder::carry_over` (ADR 0019) stays: it moves a Kemable install's `%APPDATA%` files into a new `data` folder.

## Alternatives considered

- **Copy the old `%APPDATA%` folder and move each password under the new name, once.** Written, then dropped: no other user has Paboable's data.
- **Keep the identifiers and the Credential Manager suffix as they were.** Nothing to move, but the old name would stay in every folder and secret a user sees.
