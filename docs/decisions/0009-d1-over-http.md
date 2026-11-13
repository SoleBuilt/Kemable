# 0009: Cloudflare D1 through its REST API, one request per statement

- **Status**: Accepted
- **Date**: 2026-11-10
- **Follows**: `docs/decisions/0004-engines.md` (the D1 rows).

## Context

A remote D1 database has no wire protocol: Cloudflare reaches it from Workers through a binding, and from outside through its REST API with an account ID and an API token. A local D1 database is a SQLite file Wrangler keeps, which the SQLite driver already opens.

## Decision

### The target is an account; each database is a session

- `Target::Http { endpoint: <account ID>, token }`, with the database in the `database` option, by name or UUID. The token lives in Windows Credential Manager like a password.
- The engine has databases (`has_databases`): the tree lists the account's databases (`GET /accounts/{account}/d1/database`, 1,000 a page), and a database is a session, opened by name. A name is looked up once, at connect, to its UUID.
- With no database, the account's session lists databases and runs nothing: a run says to pick one (Ctrl+K).

### A statement is one `POST .../raw`

- The editor splits with the SQLite grammar, and each statement goes alone as `{ "sql": ... }` to `/raw`, which returns `columns` and `rows` as arrays, so column order and repeated names survive (`/query` returns objects keyed by name).
- **No session state**: each request is its own transaction. `BEGIN` in one statement and `COMMIT` in the next do not make a transaction; a script that needs one runs as a single statement through Wrangler, or as a batch, later.
- **Values**: JSON numbers that are integers are `Int` (or `UInt` past `i64`), others `Float`; strings are text; a blob comes as an array of byte numbers and is `Bytes`. D1 sends no column types, so a column takes the kind of its first non-NULL value.
- **Row cap**: D1 sends the whole result in one reply; the driver stops pushing at the cap and reports it, as the other drivers do.
- **Messages** get D1's own accounting for each statement: rows read, rows written, duration. D1 bills by rows read and written.
- **Stop** closes the request. The statement may still finish on Cloudflare's side; there is no cancel to send.

### Errors

- A refused token is HTTP 401 or 403, or Cloudflare's error 10000: `driver.auth`.
- A database that is not there is HTTP 404: `not_found`.
- A SQL error is `driver.query` with Cloudflare's code (7500) as the engine code and SQLite's `at offset N` as the error's position, so the editor marks it.

### HTTP

reqwest 0.13 with `rustls-no-provider`: the driver hands it the same rustls config the other drivers build (ring, the Windows trust store through `rustls-platform-verifier`), HTTP/1.1 only, and reqwest follows the Windows proxy settings (`system-proxy`). The API address can be changed with the `api_base` option, for the tests; plain `http://` is refused except to a loopback address, since the token travels with every request.

## Rationale

- `/raw` is the only endpoint that keeps a result's shape exactly.
- A session per database fits the tree and the database picker the other engines already use, with nothing new in the UI but the form.
- reqwest is the one dependency that brings the system proxy, which a work network may need; building the TLS config here keeps one crypto provider in the app.

## Consequences

- No transactions across statements, as the API offers none.
- Every statement is a round trip to Cloudflare: a script of many small statements is slow. Running a script as one batch request is a later change.
- Large results cost a full download before the first row shows.
- No D1 account was at hand when this was written: the driver's tests run against an HTTP server in the test that answers as the API documents, and one test against the real API is ignored until `$KEMABLE_D1_ACCOUNT`, `$KEMABLE_D1_TOKEN` and `$KEMABLE_D1_DATABASE` are set.

## Alternatives considered

- **`/query`.** Same request, rows as objects: a result with two `id` columns loses one.
- **Wrangler as a sidecar** (`wrangler d1 execute --remote`). Needs Node and a login on the machine, and starts a process per statement.
- **The SQLite file for remote databases too**, by exporting. A copy, not the database.
