---
paths:
  - "ui/windows/src/**/*"
---

# Windows UI (Svelte 5, TypeScript)

- **Svelte 5 runes only:** `$state`, `$state.raw`, `$derived`, `$derived.by`, `$effect`, `$props`. No Svelte 4 stores or `export let`.
- **`$state.raw` for data from Rust** (result sets, catalogs): replace it, never mutate it, so no proxy wraps large data.
- **An `$effect` never writes what it reads.** Use `untrack` for reads that must not re-run it, and prefer `$derived` over an effect that sets state.
- **Imperative widgets stay outside Svelte's reactivity:** the grid engine and CodeMirror are created in `onMount` and told about changes from an effect.
- **All calls to Rust go through `src/lib/ipc.ts`.** No `invoke` anywhere else.
- **Errors show `errorTitle(error)`** from `src/lib/errors.ts`, with the engine's message as detail. Never show a raw code alone.
- **Keyboard first.** Every action has a key or is reachable by Tab. Shortcuts follow Windows (Ctrl, F5), and a button's `data-tip` names its shortcut.
- **Strict TypeScript.** `pnpm check` passes with no warnings; no `any`, no `@ts-ignore`.
- **Dependencies are few.** Add a package only when it saves real work, and say what it adds to the bundle.
