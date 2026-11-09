---
paths:
  - "ui/windows/src/app.css"
  - "ui/windows/src/**/*.svelte"
  - "ui/windows/src/lib/grid/engine.ts"
  - "ui/windows/src/lib/editor/**/*"
---

# Theme

- **Colours are tokens** on `:root` in `src/app.css`, redefined for dark under `prefers-color-scheme: dark`. Components use the Tailwind names mapped to them (`bg-surface`, `text-muted`, `border-line`, `text-accent`) or `var(--token)`. No hex or rgb outside `app.css`.
- **A new token** is added to both the light and the dark block, and to `@theme inline` when Tailwind needs it.
- **The canvas grid and CodeMirror read the same tokens** (`readTheme()` in the grid, `var(--…)` in the editor theme), so a token change reaches them too.
- **Density:** 13 px body text, 12 px in the grid and toolbars, 24 px rows, 26 px buttons. Fonts are Segoe UI Variable for text and Cascadia Mono for code.
- **Light and dark both checked** before a UI change is done.
