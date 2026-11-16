# 0004: How each engine is reached

- **Status**: Accepted
- **Date**: 2026-11-09
- **Amended by**: `docs/decisions/0029-tabstream.md` (SQL Server's client, Stop keeps the session)

## Context

The engines the author uses: SQL Server, PostgreSQL, SQLite, PGlite, Redis, RabbitMQ, Firebase, Supabase, Neon and Cloudflare D1, each with its live changes where it has them. They differ in data model and in how they are reached.

## Decision

| Engine | Data model | Reached through | Crate | Notes |
|---|---|---|---|---|
| SQLite | SQL | the file | `rusqlite`, bundled | Done. Interrupt on cancel. The connection has one interrupt, which stops whatever statement holds it: a call interrupts only while its own work holds the connection, again until that work ends (an interrupt before the statement starts is dropped), and a call still waiting for the connection gives up at its deadline without one, so completion or the tree never stops the user's query (amended 2026-11-15, `docs/preview-audit.md` B11) |
| Cloudflare D1, local | SQL | the file | the SQLite driver | Wrangler keeps local D1 databases as SQLite files under `.wrangler/state/v3/d1/` |
| PostgreSQL | SQL | TCP, TLS | `tokio-postgres`, rustls with ring and the Windows trust store | Done. Two sessions per connection, editor and catalog. Binary values decoded exactly; arrays, ranges, intervals and network types as psql writes them; `timestamptz` shown in UTC. Cancel through the cancel request on Stop and at the deadline; the row cap stops a plain query through a cursor and never cancels (The row cap, below) |
| Neon | SQL | the PostgreSQL driver | | TLS with SNI is required: the endpoint id travels in the server name. A suspended compute wakes in seconds, so the connect timeout is generous |
| Supabase | SQL | the PostgreSQL driver | | The transaction pooler (`*.pooler.supabase.com:6543`) does not keep prepared statements: the driver sends unnamed statements there (`StatementMode::Unnamed`, or `statement_mode=unnamed` in the URL), so a result with no rows shows no columns. Direct (5432) and session pooler work as plain PostgreSQL |
| PGlite | SQL | the PostgreSQL driver, through `@electric-sql/pglite-socket` | | PGlite runs inside Node and has no server. The app starts `pglite-server` on the database's directory as a sidecar on a loopback port, then connects as PostgreSQL. PGlite serves one connection at a time, and the directory must not be open in the user's own app at the same time |
| SQL Server | SQL | TCP, TLS | `tabstream`, Kemable's fork of tiberius 0.13.0 (`docs/decisions/0029-tabstream.md`) | Done. Batches split at `GO` lines; every result set of a batch streams. Stop sends a TDS attention on the session, never the row cap (The row cap, below), and the session stays as it was; one the server does not answer in time is reopened. A user-less login is Windows integrated authentication; `hostINSTANCE` goes through the SQL Server Browser. Encryption on with the server's certificate unchecked unless Verify full. A session stays in the database it opened on: after a batch with a `USE` anywhere in it (`sql::may_use_database`), the driver reads `db_name()`, puts the session back with `USE <its own>` and tells the sink where the batch went (`ResultSink::database_changed`), and the run goes on in that database's own session (amended 2026-11-15, `docs/preview-audit.md` A1). A run has no deadline (the client's own 30 s command bound is off): Stop ends it, and TCP keepalive (probes after 30 s idle, every 10 s) makes a link that died silently fail; a socket that times out once connected is `driver.lost`, so its session is dropped. A call queued for the editor session keeps its own Stop and deadline (amended 2026-11-15, D9). The client does not report a batch's row counts, so a command says Done without a count. `datetime2` keeps its seventh digit as text |
| Cloudflare D1, remote | SQL | HTTPS | `reqwest` with rustls and the system proxy | Done (`docs/decisions/0009-d1-over-http.md`). `POST /accounts/{account}/d1/database/{id}/raw` with an API token, one statement a request. A reply silent for 120 s fails, as Firebase's does (D9). No session: each request is its own transaction. The account's databases are the tree's databases |
| Redis | key-value | TCP, TLS | its own RESP2 client on tokio, rustls with ring and the Windows trust store | Done (`docs/decisions/0008-key-value-engines.md`). `SCAN` for keys, never `KEYS`; TCP keepalive on every link, so a dead one fails as lost (D9); a key read in steps of 1,000 into a result set; one command per line in the console, `SELECT n` as a directive. Not the `redis` crate: its TLS and its cancelled replies are explained in ADR 0008 |
| Firebase | document | HTTPS | `reqwest`, a service account's JWT signed with ring | Done for reading (`docs/decisions/0027-firebase.md`): Firestore through its REST API (list, get, `runQuery`), the Realtime Database through REST; the key file's path is kept, never its text |
| RabbitMQ | queue | AMQP (and HTTPS later) | `lapin` on tokio, rustls with ring | Done for AMQP (`docs/decisions/0026-rabbitmq.md`): the console publishes, a Live tab watches exchanges or takes from queues. To build: lists and peeks through the management HTTP API (peek requeues) |

The Live tab, one kind of tab for every engine that pushes changes (`docs/decisions/0020-live-tab.md`):

| Engine | Live source |
|---|---|
| PostgreSQL, Neon, Supabase | Done: `LISTEN` / `NOTIFY` on a session of its own, not through a pooler in transaction mode. Supabase also through Realtime over its WebSocket: tables' changes and broadcast channels (`docs/decisions/0025-supabase-realtime.md`) |
| Redis | Done: `SUBSCRIBE`, `PSUBSCRIBE`, keyspace notifications as the channels they are, with a `PING` every 30 s |
| RabbitMQ | Done: a consumer on a queue (it takes), or on a queue of its own bound to exchanges (a copy) |
| Firebase | Done: the Realtime Database's paths, as server-sent events (`docs/decisions/0027-firebase.md`). Firestore listens over gRPC, which is added only if its size is worth it |
| D1, SQLite | none; a Live tab is not offered |

Any network engine can go through an SSH server (`docs/decisions/0024-ssh-tunnels.md`): the app opens a tunnel and the driver dials its local end, keeping the host for TLS. SQL Server's named instances need a port there.

### The row cap

Amended 2026-11-14. The row cap (the toolbar's Limit) decides how many rows a result keeps; it never decides how much of a statement runs. Until then the PostgreSQL driver sent the cancel request and SQL Server an attention once a result was full. A statement still running when the cancel landed was rolled back whole: `select pg_notify('orders', g::text) from generate_series(1, 300000) g` delivered no notification, an `INSERT ... RETURNING` that had not finished inserted nothing, and inside the user's own transaction the cancel ended the transaction. On SQL Server the attention also ended the batch, so the statements after a full result never ran.

Now a statement is stopped early only where nothing it did is lost, and is otherwise read to its end with the rows past the cap dropped unread:

- **A plain query.** `sql::is_plain_query` reads it: `SELECT`, `VALUES` or `TABLE` (after `WITH` and parentheses), one statement, and no word that writes (`INSERT`, `UPDATE` but `FOR UPDATE`, `DELETE`, `MERGE`, `TRUNCATE`, `INTO`). It errs towards no; a wrong no costs only the reading of rows nobody keeps.
- **PostgreSQL** runs a plain query under a sink's limit (`ResultSink::row_limit`) through a cursor: `DECLARE kemable_cursor_<n> NO SCROLL CURSOR FOR <statement>` and `FETCH <limit + 1>`, so the server computes the rows kept and one more, keeps what computing them did (the notifications, `nextval`, a function's writes), and computes nothing after. Outside a transaction block the cursor gets a block of its own, committed at the end; inside the user's it lives there and only the cursor is closed, so Kemable never commits or ends the user's transaction. tokio-postgres keeps the transaction status to itself, so `select statement_timestamp() <> transaction_timestamp()` asks for it, in the same round trip as the `BEGIN`, the `DECLARE` and the first `FETCH`. A guess of the status, kept from the last look and from the statements run, decides whether that `BEGIN` is sent; inside the user's block its warning is dropped, and a wrong guess costs one more round trip. A run stopped before its block ended leaves the next call to roll it back, inside the select that watches Stop and the deadline: when that run's cancel did not land, the rollback waits behind the statement still running, and Stop cancels it instead of waiting (amended 2026-11-15, `docs/preview-audit.md` D10). A failed `DECLARE` in Kemable's block is rolled back and the statement runs whole, so its error reads as usual; in the user's block the failure ends the block, as the statement's own would, and its position is moved into the statement. Every other statement, a plain query with no limit (Fetch All, Run Without Limit) included, runs as before, never cancelled at the cap.
- **SQL Server** gets no attention at the cap: an attention ends the batch, so the statements after a full result would never run. Until 2026-11-15 it also cost the session: tiberius 0.13 read the attention's acknowledgement only from the response in flight, and when the server had already sent the whole response (the usual case), the acknowledgement came in the next message, tiberius reported it missing, and the session was dropped and reopened, losing its temp tables, `SET` options, `USE` and open transaction (measured 2026-11-14). tabstream, Kemable's fork, reads on to it (`docs/decisions/0029-tabstream.md`). A batch that is one plain query under a limit is sent as `SET ROWCOUNT <limit + 1>; <batch>` on its first line, so the server stops after the rows kept and its error lines stay the user's, and `SET ROWCOUNT 0` follows it in every case, a session that cannot reset it being dropped. In any other batch, a full set's other rows are read and dropped, and the batch runs to its end. Stop still sends an attention, and keeps the session (ADR 0029).
- **SQLite** makes a statement's changes on its first step and stops reading where the cap is, so it needed no change. D1, Redis and Firebase read a whole answer per request.

## Rationale

- Engines of one family share a driver and a grammar, so a fix reaches all of them.
- Each engine is reached the way its own tools reach it, so its limits (Supabase's pooler, PGlite's single connection) behave as users expect.

## Consequences

- Key-value, document and queue engines add their own `Connection` methods, defaulting to `driver.unsupported`, when the first engine of each model lands.
- PGlite needs Node on the machine. The app says so when it is missing.
- The Live tab counts its events to the UI at most every 50 ms, as a run counts its rows; the events are rows of a result set (ADR 0020).
