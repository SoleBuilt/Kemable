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
| PostgreSQL | SQL | TCP, TLS | `tokio-postgres` with rustls | Cancel through the cancel request |
| Neon | SQL | the PostgreSQL driver | | TLS with SNI is required: the endpoint id travels in the server name. A suspended compute wakes in seconds, so the connect timeout is generous |
| Supabase | SQL | the PostgreSQL driver | | The transaction pooler (port 6543) does not keep prepared statements: the driver uses the simple or unnamed-statement protocol there. Direct (5432) and session pooler work as plain PostgreSQL |
| PGlite | SQL | the PostgreSQL driver, through `@electric-sql/pglite-socket` | | PGlite runs inside Node and has no server. The app starts `pglite-server` on the database's directory as a sidecar on a loopback port, then connects as PostgreSQL. PGlite serves one connection at a time, and the directory must not be open in the user's own app at the same time |
| SQL Server | SQL | TCP, TLS | `tiberius` | Batches split at `GO` lines. Cancel through a TDS attention |
| Cloudflare D1, remote | SQL | HTTPS | `reqwest` | `POST /accounts/{account}/d1/database/{id}/query` with an API token. No session: each request is its own transaction |
| Redis | key-value | TCP, TLS | `redis` | `SCAN` for keys, never `KEYS` |
| Firebase | document | HTTPS | `reqwest` with a service-account token | Firestore through its REST API; the Realtime Database through REST |
| RabbitMQ | queue | HTTPS and AMQP | management HTTP API, `lapin` | Lists and peeks through the management API (peek requeues); publish and consume through AMQP |

The Live tab, one kind of tab for every engine that pushes changes:

| Engine | Live source |
|---|---|
| PostgreSQL, Neon, Supabase | `LISTEN` / `NOTIFY`; Supabase Realtime over its WebSocket |
| Redis | `SUBSCRIBE`, `PSUBSCRIBE`, keyspace notifications |
| RabbitMQ | a consumer on a queue, or on a temporary queue bound to an exchange |
| Firebase | the Realtime Database's server-sent events; Firestore listens over gRPC, which is added only if its size is worth it |
| D1, SQLite | none; a Live tab is not offered |

## Rationale

- Engines of one family share a driver and a grammar, so a fix reaches all of them.
- Each engine is reached the way its own tools reach it, so its limits (Supabase's pooler, PGlite's single connection) behave as users expect.

## Consequences

- Key-value, document and queue engines add their own `Connection` methods, defaulting to `driver.unsupported`, when the first engine of each model lands.
- PGlite needs Node on the machine. The app says so when it is missing.
- The Live tab streams events through the same coalesced channel as runs.
