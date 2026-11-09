# 0008: Key-value engines: a console of commands, keys by SCAN, a key read in steps into a result set

- **Status**: Accepted
- **Date**: 2026-11-09
- **Follows**: `docs/decisions/0004-engines.md` (Redis row), `docs/decisions/0002-result-windows.md` (result sets the grid pulls).

## Context

Redis is the first engine without SQL. It needs a place to type commands, a way to find keys in a database that may hold millions, and a view of one key's value whatever its type. The UI, the store and the grid already handle result sets; the question is how much of that a key-value engine can reuse.

## Decision

### The driver contract grows two methods

`Connection::scan_keys(scan)` and `Connection::read_key(key, sink)`, each defaulting to `driver.unsupported`, so the SQL drivers are unchanged (`core/crates/core/src/connection.rs`, types in `core/src/keys.rs`).

- **`scan_keys`** is one `SCAN cursor MATCH pattern COUNT n [TYPE t]` step, then one pipelined round trip for each key's `TYPE` and `PTTL`. It never sends `KEYS`, which blocks the server. A key that expires between the scan and the lookup is left out. The UI gathers steps until it holds about 200 keys or the scan ends, at most 50 steps a click, and offers Load more.
- **`read_key`** streams the value into a sink as a result set, in steps of 1,000: a hash as `field, value` (`HSCAN`), a list as `index, value` (`LRANGE`), a set as `member` (`SSCAN`), a sorted set as `member, score` (`ZRANGE ... WITHSCORES`), a stream as `id, fields` (`XRANGE`, the fields as JSON), a string as one `value`. It stops when the sink takes no more, so the row cap holds, and returns the kind, the time to live and the length (`HLEN`, `LLEN`, ...). A module's type (RedisJSON) is named in a notice and read through its own commands.

Since a key is a result set, the grid, copy, the row detail, the core's sort and Fetch all work on it unchanged.

### The console runs one command per line

`paboable_core::command` splits console text: one command per line; empty lines and lines starting with `#` or `//` are skipped; words split as `redis-cli` splits them (`"..."` with `\n`, `\xHH` and the like, `'...'` with `\'`). The run sends each line as it sends a SQL statement, through the same events, Stop, history and row cap.

- **`SELECT n` is a directive**, as psql's `\c` is: the run switches to database `n` (the session for it), and the toolbar follows. Sent to the server, it would change the database of a session the app keeps for another one.
- **Refused**: `SUBSCRIBE`, `PSUBSCRIBE`, `SSUBSCRIBE`, `MONITOR` (they turn the connection into a stream; the Live tab will take them), `SYNC`, `PSYNC`, `HELLO`, `RESET`, `QUIT`, `AUTH` (the connection logs in from its saved settings).
- **Replies take a shape** by command: `OK` is Done; `HGETALL` and `CONFIG GET` are `field, value`; `WITHSCORES` replies are `member, score`; a `SCAN` family reply shows its items and puts the next cursor in Messages; `XRANGE` is `id, fields`; any other array is one row per element. The table is in `core/crates/drivers/redis/src/reply.rs`.
- The history keeps no password: a line with `AUTH`, `requirepass`, `masterauth` or `ACL SETUSER` is stored as the withheld placeholder.

### The client is Paboable's own RESP2

`paboable-driver-redis` speaks RESP2 over tokio, with TLS through rustls, ring and the Windows trust store, as the PostgreSQL driver does. A link is one connection; commands on it go one exchange at a time. A cancel or a deadline drops the link, since its reply may still be on the way and closing the socket is what stops `BLPOP` and the like on the server; the next call opens a new link, logged in and on its database again.

- **TLS** is off or on with the certificate checked (`rediss://`): Redis has no negotiation, so "prefer" means off.
- **Login**: `AUTH password` or `AUTH user password` (ACL). A refused login is `driver.auth` with the server's word (`WRONGPASS`) as the engine code; any other error reply is `driver.query` with its first word (`WRONGTYPE`, `ERR`).
- **Databases**: 0 up to the server's `databases` setting (16 when the login may not read it); each is a session, as PostgreSQL's databases are.

## Rationale

- One result path for every engine: a key is rows, so nothing in the grid or the store learns about Redis.
- Steps of 1,000 keep each round trip small and let the row cap stop a list of millions after the first page.
- The `redis` crate (1.8) was the plan in ADR 0004. Its TLS reads `rustls-native-certs` and the process-wide crypto provider, where this repo uses `rustls-platform-verifier` with ring passed in; its multiplexed connection hides when a cancelled reply is still pending. RESP2 is a small, stable protocol: the client here is about 300 lines with its tests, and owns cancel and deadlines exactly.

## Consequences

- No Redis Cluster or Sentinel: a cluster node answers `MOVED` for keys it does not hold, shown as the error it is.
- RESP3-only features (client-side caching, push messages) are not used; every Redis since 2.6 and Valkey speak RESP2.
- Key names that are not UTF-8 show with replacement characters, and such a key cannot be opened from the list; the console can still reach it with `\xHH`.
- No Redis server was at hand when this was written: the driver's tests run against a small RESP2 server in the test that answers what the driver sends (login, `SCAN`, each kind of key, a blocking command), and one test against a real Redis is ignored until `$PABOABLE_REDIS_URL` is set.
- Editing a value is not here: the console's commands do it (`HSET`, `EXPIRE`, `DEL`), and Phase 2's row editing decides the rest.

## Alternatives considered

- **Keys as `list_objects`** in the object tree. A tree loads a level whole; a database of millions of keys needs a scan that pages.
- **`KEYS pattern`**. Simple, and it blocks the server for every client while it walks the keyspace.
- **A view per key type** instead of a result set. Nicer for a hash of three fields, and it would rebuild the grid's scroll, copy and sort for each type.
