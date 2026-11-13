# 0027: Firebase: Firestore and the Realtime Database over REST, signed in with a service account's key file

- **Status**: Accepted
- **Date**: 2026-11-14
- **Follows**: `docs/decisions/0004-engines.md` (Firebase through REST with a service account's token), `docs/decisions/0008-key-value-engines.md` (a console of commands, one a line).

## Context

Firebase is the first document engine (`DataModel::Document`): Firestore's collections of documents, with fields of many types, and the Realtime Database, one JSON tree. Both have REST APIs. A desktop tool reads them as an administrator would, with a service account: Google's tools sign in with its JSON key file, through `GOOGLE_APPLICATION_CREDENTIALS`.

Two things shape it:

- **The key is a file of about 2.4 KB.** Windows Credential Manager keeps at most 2,560 bytes a credential, and a password kept as UTF-16 takes twice its length: the key does not fit.
- **Documents have no schema.** Two documents of one collection may have different fields, and a field may change type.

## Decision

- **A crate of its own**, `kemable-driver-firebase`, on reqwest with our rustls config (ring, the Windows trust store), as D1's.
- **Signing in**: the key file's path is kept with the connection (`key_file`); the file is read at connect, never copied. Its RSA key signs a JWT (RS256, with ring, already in the app for rustls), which its `token_uri` trades for an access token, kept until a minute before it ends. A key can also be given as the config's token, for a caller that has its text. The project is the endpoint, or the key file's `project_id`.
- **Where**: Firestore at Google's API (`database (default)`); the Realtime Database at `https://<project>-default-rtdb.firebaseio.com` unless `database_url` names another (other regions use `*.firebasedatabase.app`). `emulator` (`host:port`) reaches the Firestore emulator with no key.
- **The tree**: two schemas, Firestore (its top-level collections, folder Collections) and Realtime Database (the root's children, folder Children). A double-click reads one on a query page, through the console.
- **The console**, one command a line:
  - `get <collection>` reads its documents page after page (300 a request) until the row cap; `get <collection>/<document>` reads one; `where <field> <op> <value> [and …]`, `order by <field> [desc]` and `limit <n>` make it a query (`runQuery`). A value is JSON (`12`, `true`, `"text"`, `'["a","b"]'`) or a word read as a string.
  - `rtdb <path> [limit <n>]` reads the Realtime Database: one row per child, its fields as columns when the children are objects, else its value.
- **A document as a row**: `id`, one column per field the first page has (by name, as Firestore sends them), `other_fields` (JSON of the fields only later documents have, so nothing is lost), `update_time`. Types map exactly: integers as integers (past a float's 2^53 too), doubles, timestamps in UTC with their microseconds, bytes, references as their path, maps and arrays as JSON with their typed values unwrapped.
- **Errors** keep Google's words and status (`NOT_FOUND`, `FAILED_PRECONDITION` with its link to create an index); a refused key or token is `driver.auth`.

## Rationale

- **The file, not its text, kept**: it does not fit Credential Manager, and it is how Google's own tools take a key; the user already keeps the file somewhere safe.
- **Columns from the first page, the rest as JSON**: the grid needs its columns before the rows; reading the whole collection first to learn them would make the first rows wait for the last.
- **REST, not the Firestore gRPC API**: gRPC would bring a protocol stack for what REST reads as well (`docs/decisions/0004-engines.md`).

## Consequences

- Built 2026-11-14 with its tests against a small HTTP server inside them standing in for Google's token endpoint, Firestore and the Realtime Database: the JWT verified with the key's public half, a collection over two pages with every type, a document, a query and its request, a query that needs an index, the Realtime Database, the tree, and keys that do not read. `test-only-key.pem` was made for these tests and is used nowhere else. Not tried against a real project here: `docs/testing/phase-3.md` has the steps.
- Writing (set, update, delete) and subcollections in the tree are not built.
- A connection holds no session: each command is a request, as D1's are.
