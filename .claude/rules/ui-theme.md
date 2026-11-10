---
paths:
  - "ui/windows/src/app.css"
  - "ui/windows/src/**/*.svelte"
  - "ui/windows/src/lib/grid/engine.ts"
  - "ui/windows/src/lib/editor/**/*"
---

# Theme

- **Colours are tokens** on `:root` in `src/app.css`, redefined for dark under `prefers-color-scheme: dark` and `[data-theme="dark"]`. Components use the design system's `pb-*` classes in `src/components.css` (`docs/decisions/0012-design-system.md`), else `var(--token)` or the Tailwind names mapped to the tokens. No hex or rgb outside `app.css`.
- **A class the design system has is used, not rewritten.** A new part is added to the design system artifact and to `components.css` under the same name.
- **A new token** is added to both the light and the dark block, and to `@theme inline` when Tailwind needs it.
- **The canvas grid and CodeMirror read the same tokens** (`readTheme()` in the grid, `var(--…)` in the editor theme), so a token change reaches them too.
- **Density:** 13 px body text. Grid rows are 28 px, or 24 px with Compact rows. Buttons are 32 px, or 28 px for the small size. Bars are 40 px and the toolbar is 48 px. The fonts are Geist for text and Geist Mono for code, bundled from npm.
- **Icons** are `lib/ui/Icon.svelte` (Lucide paths). A new icon adds its path there.
- **Tooltips** use `data-tip` (with `data-tip-start`, `-end` or `-up` to place it), never `title`. The tip names the shortcut.
- **Pickers** are `lib/ui/Combobox.svelte`, never `<select>`.
- **Light and dark both checked** before a UI change is done.
