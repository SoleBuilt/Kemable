# tabstream

A TDS (SQL Server) client in Rust: Kemable's fork of [tiberius](https://github.com/prisma/tiberius) 0.13.0, kept and changed by Kemable for its SQL Server driver (`core/crates/drivers/mssql`). Why it is a fork, and how it is kept: `docs/decisions/0029-tabstream.md`.

## Where it came from

- tiberius 0.13.0 as published on crates.io (its `.cargo_vcs_info.json` names commit `7fd75bb7070fe6ea41374698abf78928bb983bda`), by Steffen Butzer, Julius de Bruijn, Eric Sheppard, Yoshua Wuyts and the Tiberius contributors.
- Copied: `src/` and the two license files. Left out: the Docker setup, the examples, the tests that need a server, the Nix files, the docs and the changelog.
- Upstream fixes are taken one by one when they matter (security, protocol), never merged whole.

## License

MIT OR Apache-2.0, as tiberius: `LICENSE-MIT.txt` and `LICENSE-APACHE.txt`, with the original copyright. Kemable's changes to these files are under the same terms. Each file Kemable changed says so at its top (Apache-2.0, section 4b).

## What Kemable changed

- `Cargo.toml`: the package is `tabstream` 0.1.0, with no workspace of its own, no tests or examples that need a server, only the dev-dependencies the unit tests use, and `tiberius-macros` from crates.io. The library's name is `tabstream`; the workspace's `Cargo.toml` names the dependency `tiberius`, so the driver's code still says `tiberius::`.
- `rustfmt.toml`: the upstream `.rustfmt.toml`, renamed, so `cargo fmt --all` formats this code as upstream does and not with the workspace's settings.
- `src/client/connection.rs`, `src/tds/stream/token.rs`: an attention's acknowledgement is read on into the messages after the response, until it comes; the caller bounds the wait. tiberius stopped at the end of the response and reported the acknowledgement missing when the server had already sent the whole response. The token reader marks the connection while it is inside a token; when a dropped result stream left it there, the attention reads the rest of that message as packets and looks at its last token. Errors of the cancelled request are dropped. `cancel_request` returns nothing instead of the DONE token.
- `src/client.rs`: `Client::in_transaction()`, whether a transaction is open, from the descriptor the server's ENVCHANGE tokens set.

## Tests

The crate is not a workspace member, so `node scripts/verify.mjs` builds it through the driver but does not run its unit tests. When it changes, run them on their own:

```bash
cargo test --manifest-path core/crates/tabstream/Cargo.toml --lib --no-default-features --features tds73,rustls,sql-browser-tokio,winauth -- --skip client::tls_stream
```

That run makes a `Cargo.lock` beside this file, which `.gitignore` keeps out. It skips the TLS loader's tests: they read certificates from upstream's `docker/certs`, which is not copied. On 2026-11-16 the rest passed: 957 tests, the attention's among them (`src/tds/stream/token.rs`).
