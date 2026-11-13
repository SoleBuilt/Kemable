---
paths:
  - "core/**/tests.rs"
  - "core/**/tests/**/*"
  - "scripts/verify.mjs"
---

# Tests

- **A test names the behavior it pins**, as a sentence: `a_cancel_interrupts_a_running_statement`, not `test_cancel`.
- **Tests sit beside the code**: a `#[cfg(test)] mod tests` at the end of the file, or `tests.rs` next to `lib.rs` when the tests are long.
- **No sleeps to wait for something.** Await the event. A sleep is allowed only to make time pass before a cancel or a deadline, and then it is short.
- **No network, no fixed paths.** SQLite tests use `:memory:` or a folder under `std::env::temp_dir()` named with the process id, removed at the end.
- **Live-server tests are `#[ignore = "why"]`** and read their URL from `KEMABLE_<ENGINE>_URL`.
- **A failing test is fixed in the code.** Changing a test is allowed only when the behavior it pins changes on purpose, and the change says so.
