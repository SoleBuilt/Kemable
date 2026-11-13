# 0020: A Live tab listens on a link of its own, and what it hears is a result set

- **Status**: Accepted
- **Date**: 2026-11-12
- **Follows**: `docs/decisions/0004-engines.md` (the Live sources per engine), `docs/decisions/0002-result-windows.md` (result sets the grid pulls), `docs/decisions/0013-pages-and-tabs.md` (pages; this adds a third kind).

## Context

Phase 3 starts with the Live tab: a place to watch what an engine pushes, as it happens. PostgreSQL `LISTEN` and Redis pub/sub come first, since both engines are already in the app; Supabase Realtime, RabbitMQ consumers and Firebase follow with their drivers.

Three things shape it:

- A listener is not a request with a reply. PostgreSQL delivers a `NOTIFY` only to the session that sent `LISTEN`; a Redis connection that sends `SUBSCRIBE` takes no other command. Neither can share the sessions the editor and the tree use.
- Events can come fast and for hours. The UI must not get one message per event, and a tab left open must not fill memory.
- The usual way to try it is to listen in one tab and send from another (`NOTIFY`, `PUBLISH`). Until now a run kept its page shown until it ended (ADR 0013).

## Decision

### The driver contract grows one method, on the driver

`Driver::listen(config, topics, sink, options)`, defaulting to `driver.unsupported`. It opens a link of its own from the connection's config, asks for the topics, and then streams each event into the `ResultSink` as one row of one set, pushed as it arrives. `begin_set` is called once the server has taken every topic, so a caller that sees the set begin knows nothing after that is missed. It returns only when it stops: at the sink's row limit (`limited`), with `options.stop_reason()` after a cancel, or with `driver.lost` when the link drops. The link closes as it returns, which is also what ends the listening on the server; no `UNLISTEN` or `UNSUBSCRIBE` is sent.

`EngineInfo::live` (a flag) becomes `EngineInfo::live_topics`: the kinds of topic the engine listens to, empty for an engine that pushes nothing. Today a `Channel` (a name) and a `Pattern` (a glob).

`kemable_core::live::topics` reads what the user typed: names split at spaces, commas and new lines; a name in double quotes keeps its spaces and commas and is marked `exact`.

### PostgreSQL

- One session, not the connection's two, sends `LISTEN "name"` for each channel and then only waits.
- A bare name folds to lower case, as it would typed after `LISTEN` in psql, so `Orders` hears `NOTIFY Orders`. A quoted name is kept as written, to hear `pg_notify('Orders', ...)`.
- Columns: `received` (when it arrived here, in UTC; the server sends no time), `channel`, `payload`, `pid` (the backend that sent it).
- A pooler in transaction mode (Supabase on port 6543) does not keep `LISTEN`; the listener says so in a notice.

### Redis

- One connection sends `SUBSCRIBE` for the channels and `PSUBSCRIBE` for the patterns, then only reads.
- Columns: `received`, `channel`, `pattern` (NULL for a channel subscribed by name), `message` (bytes when it is not UTF-8).
- **It pings.** A subscriber that hears nothing cannot tell a quiet channel from a dead link, and a cloud load balancer drops a link left idle for a few minutes. Every 30 s the listener sends `PING`, which Redis answers in this mode too; a `PING` left unanswered until the next one ends the listening as `driver.lost`. PostgreSQL's session has TCP keepalive from its connect settings.
- **Key events** are channels (`__keyspace@0__:*`, `__keyevent@0__:expired`), so they need no code of their own. When a topic names them and the server's `notify-keyspace-events` is empty, the listener says in a notice that nothing will come and how to turn it on.
- An error reply to a subscription (`NOPERM`) fails the listening with the server's word as the engine code.

### The app: events are counted, not sent

`start_live` returns at once with an id that `cancel_run` stops, and reports on a channel:

- `listening`: the server took the topics; the events are rows of result `n`.
- `events`: how many the result holds and how many its filter shows. At most one every 50 ms, and the count after the last event always goes out, however long the channel then stays quiet (a reporter task wakes on each event, sends, then waits out the interval).
- `notice`, and `finished` with whether it stopped at the cap or failed.

The result is an ordinary `ResultStore`, so the grid, copy, the row detail, the core's sort and the filter on rows held (ADR 0011) work on it unchanged; a filter set while it listens is applied to each event as it arrives.

- **The cap is 100,000 events.** One past it the listening stops and the page says so. A ring that drops the oldest would need a store that removes rows, which every reader of row numbers would then have to follow.
- **Closing a connection stops its Live tabs**: each takes a child of the server's `closing` token (`OpenServer`), so nothing listens on for a connection the user closed.
- No reconnect yet: a lost link ends the listening with its error and the events it had, and Listen starts again. The planned `session` crate (health, reconnect) will decide the rest.

### The UI: a third kind of page, which listens while hidden

- **A Live page** (Ctrl+Alt+L, or New Live tab in Ctrl+K, for an engine with `live_topics`). No editor: a row for the channels (and the patterns, for Redis), then the events in the grid. Listen is F5 or Enter in a box; Stop is Shift+F5.
- **It goes on listening while another page is shown.** Its state is a `LiveSession` object the page holds (`ui/windows/src/lib/live/session.svelte.ts`), not the App's live variables, so events arriving for a hidden page cost one count update and no DOM. A query run still keeps its own page shown (ADR 0013); a listening does not.
- **Tools on the page bar**: Listen and Stop side by side, Follow (keep the newest event in view; on at first), Filter, Clear, Row detail. The tab shows the event count.
- A new Listen starts a new list once the server has taken the topics; the page's filter carries over to it.
- **It says when the toolbar has moved to another database** (added 2026-11-13). An event reaches only listeners in the database it was sent in, so a `NOTIFY` from a query page that now runs elsewhere succeeds and never arrives. While that is so, the page shows it in a warning strip, with what to do.
- Live pages are not kept across launches.

## Rationale

- **On `Driver`, not `Connection`**: a listener needs a link that does nothing else, and the engines to come (an AMQP consumer, Firebase's server-sent events, Supabase's WebSocket) each open theirs their own way. A method on `Connection` would suggest it shares the connection's sessions, which it cannot.
- **Rows, not an event type of its own**: as a Redis key did (ADR 0008), an event stream reuses the store, the window format, the grid and the filter. Nothing in them learns about Live.
- **Counts, not events, to the UI**: the same rule as a run's rows (`.claude/rules/ipc.md`). A channel with thousands of events a second sends twenty small messages a second.

## Consequences

- A listening holds one more connection to the server for as long as it runs.
- The time shown is when the event reached Kemable, in UTC like every `timestamptz` in the app, not when it was sent.
- At the cap the listening stops; a busy channel needs Clear and Listen again, or a narrower topic.
- Sorting while it listens sorts the events held. The store puts later ones after them in arrival order (its rule for late rows), so a Live page sorts again at most every 300 ms while events arrive (amended 2026-11-12, after a sort by `received` descending left new events at the bottom).
- A Tauri channel hands on its next message only once the handler of this one returned, so a handler that throws would hold every later event back with nothing said. `LiveSession` logs a failure and goes on (amended 2026-11-12).
- The drivers' tests run against a small PostgreSQL backend and a small pub/sub server inside the tests; the tests against a real server are ignored until `$KEMABLE_POSTGRES_URL` and `$KEMABLE_REDIS_URL` are set. On 2026-11-12 the PostgreSQL ones ran against PostgreSQL 17.4, and the Live page was driven in the dev app over WebView2's DevTools protocol (listen, 5,000 events, filter, sort, close); Redis had not run (`docs/testing/phase-3.md` has the steps).
- Still to build (`docs/roadmap.md`): Supabase Realtime, RabbitMQ consumers, Firebase; reconnect; remembering a connection's channels.

## Alternatives considered

- **`LISTEN` on the editor's session, notifications shown in Messages.** What psql does. Notifications would arrive only between statements, mixed with every run's messages, and nothing would show while the user is on another page.
- **The Redis console taking `SUBSCRIBE`.** It turns the connection into a stream, which is why the console refuses it (ADR 0008).
- **One event to the UI per message**, appended to a list in JavaScript. Simple, and it is the DOM-per-row list the grid was built to avoid.
- **A ring buffer of the newest N events.** The right shape for a monitor, at the cost of a store whose rows move under an open grid, selection and row detail.
