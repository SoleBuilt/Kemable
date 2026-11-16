# 0029: SQL Server's TDS client is tabstream, Kemable's own fork of tiberius 0.13.0

- **Status**: Accepted
- **Date**: 2026-11-15
- **Amends**: `docs/decisions/0004-engines.md` (the SQL Server row, and The row cap). Decided with the user on 2026-11-15 (`docs/preview-audit.md`, A9).

## Context

- **Stop reset the session.** Stop on SQL Server sends a TDS attention; the server answers with a DONE token that has the `DONE_ATTN` bit, the last token of the message that carries it. tiberius 0.13.0 looked for it only up to the end of the message in flight. When the server had already sent the whole response, which is the usual case for a result that is read fast, the acknowledgement comes as a message of its own, tiberius reported "Never got a DONE token acknowledging the Attention signal", and the driver dropped the session and opened another. Temp tables, `SET` options, `USE` and the open transaction were gone, and nothing said so. Measured 2026-11-14 with `select top 200000 a.object_id from sys.all_objects a cross join sys.all_objects b`, stopped after its first rows.
- **A stop can land inside a token.** A row often spans two packets. When Stop comes while the reader waits for the second packet, the stream is dropped with half the row read, and the bytes that follow are not a token's start. Reading tokens from there reads garbage.
- **The transaction state was kept but not told.** tiberius keeps the transaction descriptor the server sends in its ENVCHANGE tokens and sends it with each request, but does not say whether a transaction is open. Kemable needs to know (`docs/preview-audit.md`, A3).
- The fix is small but sits in the protocol's read loop. A fix upstream would still mean waiting for a release, and the next fix with it.

## Decision

- **A crate of our own.** `core/crates/tabstream` holds tiberius 0.13.0's `src/` and its license files: package `tabstream`, version 0.1.0, `MIT OR Apache-2.0`, with the original copyright. Kemable maintains it. Each file Kemable changes says so in a comment at its top (Apache-2.0, section 4b), and its `README.md` lists what changed. The Docker setup, the examples, the tests that need a server and the Nix files are not copied.
- **Outside the workspace's members.** The root `Cargo.toml` lists it in `workspace.exclude` and names the dependency `tiberius` (`package = "tabstream"`), so the driver's code keeps `tiberius::`. The workspace lints and `cargo clippy --workspace` do not reach it, and the lock file holds only the optional dependencies Kemable turns on. `cargo fmt --all` still formats it, as every path dependency, so it keeps upstream's `rustfmt.toml` beside it. Its features stay as upstream has them; Kemable uses `tds73`, `rustls`, `sql-browser-tokio`, and `winauth` on Windows.
- **The attention reads on.** After an attention, the reader goes on into the following messages until the acknowledgement comes; the caller bounds the wait (the driver's `CANCEL_GRACE`). The cancelled request's errors are dropped on the way; its ENVCHANGE tokens still update the session (the transaction, the packet size).
- **A token half read is skipped by packets.** The token reader marks the connection while a token is half read. An attention then reads the rest of that message as packets and looks only at its last token, then reads tokens again from the next message.
- **`Client::in_transaction()`** says whether a transaction is open: the descriptor from the last ENVCHANGE is not zero.
- **Upstream fixes one by one.** A fix from tiberius is taken when it matters (security, the protocol), as its own commit that names the upstream change. Upstream is never merged whole.
- **Licenses.** `cargo deny` checks it as any crate, by its license field. cargo-about lists it in `ui/windows/THIRD-PARTY-NOTICES.md` under the MIT License, quoting its own `LICENSE-MIT.txt` with "Copyright (c) 2021 Tiberius Contributors" (`docs/decisions/0023-licensing.md`). It never gets `publish = false`: both tools skip such crates as Kemable's own, and the notice would go.

## Rationale

- Keeping the session is what a user expects from Stop: a temp table, a `SET`, a `USE` and an open transaction are the user's work. Dropping them on every Stop, even with a message, loses that work.
- A fork in the repository changes in the same commit as the driver and its tests. A `[patch]` with a git dependency would need `deny.toml` to allow an unknown git source, and still a home for the change.
- Outside the members, the fork stays close to upstream, so an upstream fix still applies; rewriting it to the workspace lints would make every one a merge by hand.
- Reading tokens after the attention, rather than packets only, keeps the ENVCHANGE tokens, and the transaction state with them. Packets are only for the one case tokens cannot be read.

## Consequences

- Stop leaves the session as it was: the same `@@SPID`, its temp tables, `SET` options, database and open transaction (the driver's live tests `a_stop_keeps_the_session_its_temp_tables_and_its_transaction` and `an_attention_after_the_whole_response_is_acknowledged_in_the_next_message`, which both fail with upstream's read; the fork's unit tests in `src/tds/stream/token.rs` cover each path without a server). A session the server does not answer in `CANCEL_GRACE` is still dropped and opened again.
- When Stop lands inside a token and the server ends a transaction in that same message (`XACT_ABORT ON` rolls back on an attention), the skipped ENVCHANGE is not read, the descriptor stays, and the next request may be refused, which drops the session as before. Rare: it needs all three at once.
- Kemable watches tiberius' releases and advisories itself: `cargo deny` knows nothing about a path crate.
- The fork's unit tests are not run by `node scripts/verify.mjs`, which builds it only through the driver; its `README.md` gives the command to run them when it changes.
- The row cap still never sends an attention (`docs/decisions/0004-engines.md`, The row cap): an attention ends the batch's other statements, which this does not change.

## Alternatives considered

- **Wait for a fix upstream**: Stop resets the session until a release that may not come, and the transaction state stays untold.
- **A `[patch.crates-io]` to a git fork**: `deny.toml` denies unknown git sources, and the fork would live outside the repository and its review.
- **A workspace member**: the workspace lints and clippy on upstream's code mean rewriting it, and the lock file and `cargo deny` would take in every optional dependency (native TLS, OpenSSL, GSSAPI).
- **Read only packets after an attention**: robust in every case, but it never reads the ENVCHANGE tokens, so a transaction ended by the cancelled batch would stay open in the client's eyes.
- **Keep dropping the session, and say so**: honest, but the user's state is still lost on every Stop.
