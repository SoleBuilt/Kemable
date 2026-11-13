# 0025: A Supabase project's Live tab also listens through Realtime

- **Status**: Accepted
- **Date**: 2026-11-14
- **Follows**: `docs/decisions/0020-live-tab.md` (the Live tab, its sink and its counts), `docs/decisions/0004-engines.md` (Supabase is the PostgreSQL driver).

## Context

A Supabase project pushes its changes through Realtime, a service beside the database: a WebSocket at `https://<project>.supabase.co/realtime/v1/websocket` speaking Phoenix channels in JSON. It hears inserts, updates and deletes on the tables in the `supabase_realtime` publication (`postgres_changes`), and messages sent to a channel by name (`broadcast`). `LISTEN` still works on a direct connection to the database, but it hears only `NOTIFY`, and not through the transaction pooler.

Realtime needs the project's URL and an API key (anon, or service_role to pass row level security); the database login does not reach it.

## Decision

- **Where it lives**: `kemable_driver_postgres::realtime`, the PostgreSQL family's crate, since Supabase is that family. It is a function the app calls, `realtime::listen(url, key, topics, sink, options)`, not `Driver::listen`: it is reached with a URL and a key, not with the connection's config, and the database's driver contract stays one link of the database's own protocol.
- **Which connections**: `realtime::project_url` reads the project from the host (`db.<ref>.supabase.co`) or from a pooler's user (`postgres.<ref>` at `*.pooler.supabase.com`). An opened connection carries that URL (`OpenedConnection::realtime_url`), and only then does its Live page show **LISTEN | Realtime**.
- **Topics**: two new kinds in `kemable_core::live`, read as channels are (`topics_of`): `Table` (`schema.table`, `schema.*`, `*`; a bare name is in `public`) and `Broadcast` (a channel's name). Every table goes on one Phoenix channel, `realtime:kemable`; each broadcast channel is its own.
- **When it starts**: the set begins once every join is answered `ok` and, for tables, the server's `system` message says the subscription to the database is in place, so nothing sent after Listen is missed. A refused join or subscription fails with the server's reason (`driver.query`): a table not in the publication, a key without the rights.
- **Rows**: `received` (here, in UTC), `channel` (`schema.table`, or the broadcast channel), `event` (`INSERT`, `UPDATE`, `DELETE`, or the broadcast's event name), `committed` (the commit time the server gives, as its text), `record` and `old_record` (JSON; `old_record` is NULL when the table has no `REPLICA IDENTITY FULL` and the server sends nothing of it).
- **A dead socket**: a heartbeat every 25 s, as Supabase's own clients send; one unanswered when the next is due ends the listening as `driver.lost`, as Redis's `PING` does.
- **The key**: typed on the Live page once, then kept in Windows Credential Manager as `realtime/<project>.supabase.co/key` and used when the box is blank. It travels in the socket's URL and in each join, as Supabase's clients send it, never to a log or an event.
- TLS through rustls with ring and the Windows trust store, as every driver here (`tokio-tungstenite`, MIT or Apache-2.0).

## Rationale

- **A function, not a second `Driver`**: Realtime is not an engine of its own; it belongs to a PostgreSQL connection the user already opened, and its Live page is that connection's.
- **One channel for every table**: Supabase's clients put many `postgres_changes` filters on one channel; a channel per table would be as many joins and replies for nothing.
- **Waiting for `system`**: the join's reply comes before the database subscription exists; starting the set then could miss the first change.

## Consequences

- Built 2026-11-14 with its tests against a Phoenix server inside them (changes and broadcasts as rows, the joins sent, a refused join and a refused subscription, a stop, a server not there), and the project URL from a host or a pooler user. Not tried against a real project here: `docs/testing/phase-3.md` has the steps.
- Presence (who is in a channel) is not shown; a self-hosted Realtime at another address is not reached yet (the URL comes only from a Supabase host).
- Realtime through an SSH tunnel is not offered: it goes to Supabase's public address.

## Alternatives considered

- **Polling the tables**: heavy on the database, and misses rows changed twice between polls.
- **Reading the publication with logical replication from the app**: needs a replication slot and the database's rights Realtime already has.
