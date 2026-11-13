# 0026: RabbitMQ: an AMQP connection, a console that publishes, and Live tabs that watch or take

- **Status**: Accepted
- **Date**: 2026-11-14
- **Follows**: `docs/decisions/0004-engines.md` (RabbitMQ is reached through AMQP and the management API), `docs/decisions/0020-live-tab.md` (the Live tab).

## Context

RabbitMQ is the first queue engine (`DataModel::Queue`). What its users watch is messages as they flow: what an exchange routes, what lands in a queue. The roadmap asked first for the Live tab's consumers; listing queues and exchanges, peeking and publishing from forms come after.

Two ways to hear messages differ in what they leave behind:

- **A copy**: a queue of the listener's own, bound to an exchange, gets a copy of every message the exchange routes to it. Nobody else's messages are touched, and the queue goes when the listener stops.
- **A take**: consuming an existing queue removes what is acknowledged. That is what a worker does, and what a user means by "read the dead-letter queue".

## Decision

- **The client**: lapin 4 on tokio, TLS through rustls with ring and the platform's trust store (`rustls--ring`, `rustls-platform-verifier`), as every driver here. Not a client of our own: AMQP 0-9-1 has many more frames than RESP2, and lapin cancels and closes cleanly on drop. MIT, its protocol crates BSD-2-Clause.
- **The connection**: one AMQP connection and one channel, opened from the config as an AMQP URI (`amqp://` or `amqps://`, the virtual host as the database, `/` when none; `guest` when no user is given). A heartbeat of 30 s is asked for. An SSH tunnel works without TLS only: TLS would check the certificate against `127.0.0.1`.
- **The console**: one command per line (`kemable_core::command`). `publish <exchange> <routing key> <body>` sends a message (`""` is the default exchange; the body is the rest of the line, or one quoted word) and waits for the broker's confirmation where it gives one. Nothing else yet.
- **The Live tab** listens to two kinds of topic, added to `kemable_core::live`:
  - `Binding`, typed `exchange` or `exchange:key` (the key is `#` when none is given: everything, on a topic exchange): one exclusive, auto-delete queue the server names is bound to each, and consumed without acknowledgement. Closing the connection deletes it.
  - `Queue`, by name, consumed with acknowledgement: each message is acknowledged once the result holds it, so a message is never lost between the two; the page says that the queue no longer holds what was taken. The queue is checked with a passive declare first: one that is not there is an error, never made.
  - Columns: `received`, `exchange`, `routing_key`, `body` (text, or bytes when it is not UTF-8), `properties` (JSON of the ones the publisher set: content type, message id, headers, …), `redelivered`.
  - At most 200 messages are taken ahead of the ones stored (`basic.qos`).
- **The window**: a queue engine has no tree. The sidebar shows how to open a Live tab and publish; the editor is a plain console. `amqp://` and `amqps://` URLs fill the connect form; its Database is the Virtual host.
- lapin keeps the broker's version to itself, so the status bar shows none.

## Rationale

- **A copy by default**: watching should not change what a system does. Taking from a queue is offered by name, in its own box, with what it does said beside it.
- **Acknowledge after storing**: the result is where the user reads a message; acknowledging first would lose one if the store refused it.

## Consequences

- Built 2026-11-14. Tests without a broker: the URI (encoding, TLS port, guest, an SSH tunnel), bindings, a message as a row (text and bytes, every kind of header), a closed port. The live test (`a_live_binding_hears_what_the_console_publishes`) waits for `$KEMABLE_RABBITMQ_URL` and has not run: there was no broker on this machine. `docs/testing/phase-3.md` has the steps by hand.
- Still to build (`docs/roadmap.md`): queues and exchanges in the sidebar and peeking through the management HTTP API, publish from a form.

## Alternatives considered

- **Consuming a queue with requeue (nack)**: the message comes straight back to the same consumer, in a loop.
- **`basic.get` polling**: a round trip per message, and still a take.
