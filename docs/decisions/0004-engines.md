# 0004: How each engine is reached

- **Status**: Accepted
- **Date**: 2026-11-09

## Context

The engines the author uses: SQL Server, PostgreSQL, SQLite, PGlite, Redis, RabbitMQ, Firebase, Supabase, Neon and Cloudflare D1, each with its live changes where it has them. They differ in data model and in how they are reached.

## Decision

| Engine | Data model | Reached through | Crate | Notes |
|---|---|---|---|---|
| SQLite | SQL | the file | `rusqlite`, bundled | Done. Interrupt on cancel |
| Cloudflare D1, local | SQL | the file | the SQLite driver | Wrangler keeps local D1 databases as SQLite files under `.wrangler/state/v3/d1/` |
| PostgreSQL | SQL | TCP, TLS | `tokio-postgres`, rustls with ring and the Windows trust store | Done. Two sessions per connection, editor and catalog. Binary values decoded exactly; arrays, ranges, intervals and network types as psql writes them; `timestamptz` shown in UTC. Cancel through the cancel request, also when the row cap stops a result |
| Neon | SQL | the PostgreSQL driver | | TLS with SNI is required: the endpoint id travels in the server name. A suspended compute wakes in seconds, so the connect timeout is generous |
| Supabase | SQL | the PostgreSQL driver | | The transaction pooler (`*.pooler.supabase.com:6543`) does not keep prepared statements: the driver sends unnamed statements there (`StatementMode::Unnamed`, or `statement_mode=unnamed` in the URL), so a result with no rows shows no columns. Direct (5432) and session pooler work as plain PostgreSQL |
| PGlite | SQL | the PostgreSQL driver, through `@electric-sql/pglite-socket` | | PGlite runs inside Node and has no server. The app starts `pglite-server` on the database's directory as a sidecar on a loopback port, then connects as PostgreSQL. PGlite serves one connection at a time, and the directory must not be open in the user's own app at the same time |
| SQL Server | SQL | TCP, TLS | `tiberius` | Done. Batches split at `GO` lines; every result set of a batch streams. Stop and the row cap send a TDS attention on the session; a session that does not come back clean is reopened. A user-less login is Windows integrated authentication; `hostINSTANCE` goes through the SQL Server Browser. Encryption on with the server's certificate unchecked unless Verify full. tiberius does not report a batch's row counts, so a command says Done without a count. `datetime2` keeps its seventh digit as text |
| Cloudflare D1, remote | SQL | HTTPS | `reqwest` with rustls and the system proxy | Done (`docs/decisions/0009-d1-over-http.md`). `POST /accounts/{account}/d1/database/{id}/raw` with an API token, one statement a request. No session: each request is its own transaction. The account's databases are the tree's databases |
| Redis | key-value | TCP, TLS | its own RESP2 client on tokio, rustls with ring and the Windows trust store | Done (`docs/decisions/0008-key-value-engines.md`). `SCAN` for keys, never `KEYS`; a key read in steps of 1,000 into a result set; one command per line in the console, `SELECT n` as a directive. Not the `redis` crate: its TLS and its cancelled replies are explained in ADR 0008 |
| Firebase | document | HTTPS | `reqwest` with a service-account token | Firestore through its REST API; the Realtime Database through REST |
| RabbitMQ | queue | HTTPS and AMQP | management HTTP API, `lapin` | Lists and peeks through the management API (peek requeues); publish and consume through AMQP |

The Live tab, one kind of tab for every engine that pushes changes (`docs/decisions/0020-live-tab.md`):

| Engine | Live source |
|---|---|
| PostgreSQL, Neon, Supabase | Done: `LISTEN` / `NOTIFY` on a session of its own, not through a pooler in transaction mode. To build: Supabase Realtime over its WebSocket |
| Redis | Done: `SUBSCRIBE`, `PSUBSCRIBE`, keyspace notifications as the channels they are, with a `PING` every 30 s |
| RabbitMQ | a consumer on a queue, or on a temporary queue bound to an exchange |
| Firebase | the Realtime Database's server-sent events; Firestore listens over gRPC, which is added only if its size is worth it |
| D1, SQLite | none; a Live tab is not offered |

## Rationale

- Engines of one family share a driver and a grammar, so a fix reaches all of them.
- Each engine is reached the way its own tools reach it, so its limits (Supabase's pooler, PGlite's single connection) behave as users expect.

## Consequences

- Key-value, document and queue engines add their own `Connection` methods, defaulting to `driver.unsupported`, when the first engine of each model lands.
- PGlite needs Node on the machine. The app says so when it is missing.
- The Live tab counts its events to the UI at most every 50 ms, as a run counts its rows; the events are rows of a result set (ADR 0020).
