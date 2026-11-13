---
paths:
  - "core/crates/drivers/**/*"
---

# Drivers

The `add-driver` skill has the steps for a new engine. These rules hold for every driver.

- **Depend on `kemable-core` only.** Never on another driver, the store or the app. A family (PostgreSQL, Neon, Supabase) is one driver with options, not several crates.
- **Map types exactly.** Integers to `Int`/`UInt`, exact numerics to `Decimal` with the engine's text, floats to `Float`, dates and times to day numbers and microseconds, `uuid` to `Uuid`, JSON to `Json`, binary to `Bytes`. A type with no variant goes to `Other` in the engine's own text form. Never format, never round, never go through `f64` for a decimal.
- **Stream.** Push rows in batches of `PUSH_BATCH_ROWS` as they are read. Stop reading when `push_rows` returns `false`, and report `limited`.
- **Cancel on the server.** When `options.cancel` fires or the deadline passes, stop the statement the engine's way (SQLite interrupt, PostgreSQL cancel request, TDS attention, closing an HTTP request), then report `options.stop_reason()`.
- **Blocking libraries run on `spawn_blocking`.** Never block a tokio worker.
- **Map errors to codes.** Login refused is `driver.auth`, unreachable is `driver.connect`, a bad certificate `driver.tls`, a statement the server refused `driver.query` with the engine's code in `engine_code` and its position when it gives one.
- **Never log a secret.** `Secret` hides itself in `Debug`; never call `expose()` outside the line that hands it to the engine.
- **Tests:** what runs without a server runs in `cargo test`. Tests against a live server are `#[ignore = "needs <engine> at $KEMABLE_<ENGINE>_URL"]` and read the URL from the environment.
