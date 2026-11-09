---
paths:
  - "ui/windows/src-tauri/**/*"
  - "ui/windows/src/lib/ipc.ts"
  - "ui/windows/src/lib/types.ts"
---

# Commands between the UI and Rust

- **Every command is `async`** and returns `Result<T, DriverError>`. A sync Tauri command runs on the main thread and freezes the window.
- **Three files change together:** the command in `src-tauri/src/commands.rs`, its line in `generate_handler!` in `src-tauri/src/lib.rs`, and its typed wrapper in `src/lib/ipc.ts` (with any new type in `src/lib/types.ts`). Rust parameter `snake_case` is `camelCase` on the TypeScript side.
- **Rows travel as bytes.** A command that returns rows returns `tauri::ipc::Response` with a packed window. JSON carries ids, metadata and events only.
- **Long work returns at once** with an id, and reports on a `Channel`. Events that can come fast (row counts) are coalesced to at most one per 50 ms.
- **Ids, not handles.** The UI holds `u64` ids from `AppState`; a stale id returns `not_found`. Whatever the UI opens (a connection, a result set) it closes or releases.
- **Capabilities stay minimal.** `capabilities/default.json` lists only the permissions in use. A plugin is added with the one permission it needs, never `*:default` when a narrower one exists.
- **The CSP stays strict** in `tauri.conf.json`. No remote script, no `unsafe-eval`.
- **No secret in an event or a log.** A password reaches Rust inside `ConnectionConfig` and stays there.
