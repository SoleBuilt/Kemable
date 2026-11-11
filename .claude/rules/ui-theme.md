---
paths:
  - "ui/windows/src/app.css"
  - "ui/windows/src/**/*.svelte"
  - "ui/windows/src/lib/grid/engine.ts"
  - "ui/windows/src/lib/editor/**/*"
---

# Theme

- **Colours are tokens** on `:root` in `src/app.css`, redefined for dark under `prefers-color-scheme: dark` and `[data-theme="dark"]`. Components use the design system's `pb-*` classes in `src/components.css` (`docs/decisions/0012-design-system.md`), else `var(--token)` or the Tailwind names mapped to the tokens. No hex or rgb outside `app.css`.
- **A screen uses the parts in `src/lib/ui/`**, never the `pb-*` markup they draw (`docs/decisions/0014-ui-components.md`). A new part goes in `lib/ui/`, with its class in `components.css` under the design system's name and no `<style>` block of its own.
- **A new token** is added to both the light and the dark block, and to `@theme inline` when Tailwind needs it.
- **The canvas grid and CodeMirror read the same tokens** (`readTheme()` in the grid, `var(--…)` in the editor theme), so a token change reaches them too.
- **Density:** 13 px body text. Grid rows are 28 px, or 24 px with Compact rows. Buttons are 32 px, or 28 px for the small size. Bars are 40 px and the toolbar is 48 px. The fonts are Geist for text and Geist Mono for code, bundled from npm.
- **Icons** are `lib/ui/Icon.svelte` (Lucide paths). A new icon adds its path there.
- **Tooltips** are a part's `tip` and `tipAt` props (`lib/ui/tip.ts`, which writes `data-tip`), never `title`. The tip names the shortcut.
- **A text box has no focus ring**; its caret shows where typing goes. Buttons and other controls keep the ring for the keyboard.
- **Pickers** are `lib/ui/Combobox.svelte`, never `<select>`.
- **Light and dark both checked** before a UI change is done.
