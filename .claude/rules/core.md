---
paths:
  - "core/crates/core/**/*"
  - "core/crates/store/**/*"
---

# Core and store

- **`paboable-core` has no I/O and depends on no workspace crate.** It holds types, traits and pure functions. A tokio runtime appears only in its tests.
- **`Value` is never formatted by a driver or a UI.** `format::write_display` is the one formatter. A new variant gets its display text and a test of its edge cases (negative, zero, the type's limits, multi-byte text) in `format.rs`.
- **`ValueKind` numbers are part of the window format.** Never renumber one. A new kind takes the next number and is added to `kind_from_u8` in the store and to `Kind` in `ui/windows/src/lib/grid/window.ts`.
- **Every public `Connection` method takes `CallOptions`.** A blocking or long call checks `options.check()` between stages and stops server-side work when the token fires.
- **`DriverError` carries a code.** Build it with the constructors (`DriverError::cancelled`, `::timeout`, `::new(code, engine_message)`). Never put user-facing words in `message`; it holds the engine's own text.
- **Locks in the store are short.** A window is packed under one read guard; no I/O, no callback and no allocation proportional to the whole set happens while a lock is held.
- **No `unwrap` or `expect` outside tests.** Use `?`, `ok_or_else`, or `unwrap_or_else(PoisonError::into_inner)` for a lock whose data stays consistent.
- **A change to the window layout** updates `store/src/window.rs`, `ui/windows/src/lib/grid/window.ts` and `docs/decisions/0002-result-windows.md` together, and the store's window tests.
