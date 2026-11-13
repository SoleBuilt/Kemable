# 0001: One Rust core, one UI per operating system

- **Status**: Accepted
- **Date**: 2026-11-09

## Context

Kemable must start fast, stay light and scroll smoothly through large results. It starts on Windows; macOS and Linux follow. A UI-free Rust core can keep every driver, session, result set and error code, and give each platform its own UI, which calls the crates directly or through FFI.

## Decision

- All work that is not drawing lives in Rust crates under `core/`, with no UI or platform dependency.
- Each operating system has its own UI under `ui/<os>/`: `ui/windows` now, `ui/apple` and `ui/linux` later.
- A UI crate is the only place that names every driver and passes platform services in.
- The core returns error codes; each UI words them.

## Rationale

- One implementation of drivers, SQL splitting and value formatting means one place to test them, and every UI shows the same exact values.
- A UI that only lays out and paints can be replaced (a native toolkit later, if WebView2's floor ever matters) without touching the core.

## Consequences

- A feature the UI needs and the core lacks is added to the core first, with tests.
- The core API grows from what the Windows UI asks; it is shaped by need, not designed ahead.
- `ui/apple` and `ui/linux` may reuse the Windows web UI through Tauri, or go native. That choice gets its own decision when it comes.

## Alternatives considered

- **Logic in TypeScript, Rust only for drivers.** Formatting and splitting would be written again for each UI, and big results would cross into JavaScript.
- **Reusing another project's database crates as a dependency.** Those read while planning were unpublished or changed often, and pulled engines and a crypto stack this app does not need yet. Kemable writes its own, smaller core.
