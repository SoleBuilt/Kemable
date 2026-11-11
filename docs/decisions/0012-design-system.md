# 0012: The Windows app is drawn from one design system: black and white, Geist, Lucide, tokens and pb-* classes

- **Status**: Accepted
- **Date**: 2026-11-11
- **Follows**: the design language of ERP Tinh gọn. The design system lives as the claude.ai artifact "Paboable Design System" (https://claude.ai/artifact/95LQbDNV7YrTatbBwjPy8Z); the approved screens are the "Paboable Prototype" canvas (https://claude.ai/artifact/MGBUywSGe5pUwerFj3W8xz).

## Context

The first screens used Segoe UI, Tailwind classes written in each component and a few colour tokens. Every new screen invented its own buttons, menus and spacing, and the toolbar grew one button per way to connect. A prototype of every screen was built and tried before any code, and its decisions (D1 to D26) were approved on 2026-11-11.

## Decision

### Look

- **Black and white.** `accent` is near black in light and near white in dark. State colours (danger, warning, success), edit colours (changed, deleted, added), connection colours and syntax colours keep their own hues.
- **Geist and Geist Mono**, bundled from `@fontsource-variable/geist` and `@fontsource-variable/geist-mono`. The WebView loads only the subsets a page uses: Latin text takes about 82 KB.
- **Lucide icons**, as SVG paths inside `ui/windows/src/lib/ui/Icon.svelte`. There is no icon package at runtime.
- **Density:** body text 13 px. Grid rows are 28 px, or 24 px with Compact rows in Settings. Buttons are 32 px, or 28 px for the small size. Bars are 40 px and the toolbar is 48 px. Motion lasts 120 ms and is turned off under `prefers-reduced-motion`.
- **The grid** has vertical and horizontal lines in `grid-line` and no stripes. NULL is drawn in italics in `fg-muted`.
- **Light and dark** follow the system, or the choice in Settings (`data-theme` on `<html>`). The accent is black and white, or orange (`data-style` on `<html>`).

### Where it lives

- **Tokens:** `ui/windows/src/app.css`. Colours, radii, shadows and sizes are CSS variables, mapped for Tailwind in `@theme inline`. The old names (`bg`, `surface`, `surface-2`, `surface-3`) stay as aliases.
- **Components:** `ui/windows/src/components.css` holds the `pb-*` classes, the same ones the design system's bundle uses: buttons, fields, combobox, menu, palette, tabs, panes, dialog, info bar, badge, status bar, pager and filter rows. A component in the app uses those classes; it writes no colour of its own.
- **Tooltips:** `data-tip`, with `data-tip-start`, `data-tip-end` or `data-tip-up` to place the tip. Never `title`, which WebView2 draws late and in the system's style.
- **Pickers:** every choice from a list is `lib/ui/Combobox.svelte`. No `<select>`.
- **Shared parts:** `lib/ui/`: `Icon`, `Combobox`, `CommandPalette` (Ctrl+K) and `SettingsDialog`.

### A change to the look

- It changes the design system artifact first, then `app.css` or `components.css`. The two must keep the same class names.
- A new colour is a token in both the light and the dark block (`.claude/rules/ui-theme.md`).

## Rationale

- One set of classes makes a new screen a matter of layout. It matches the prototype the user approved, and keeps the look the same from screen to screen.
- Black and white puts the colour where it carries meaning: an error, an edit, the connection.
- Fonts and icons are bundled, so the app looks the same on every Windows machine and never waits on the network.

## Consequences

- The Mica theme (D10, backlog B1) is planned in `docs/roadmap.md`.
- The orange accent style (B2) is Settings › Style. It sets `data-style="orange"` on `<html>` and swaps only `accent`, `accent-hover`, `on-accent`, `focus` and `focus-halo`: `#c2410c` with white text in light (5.2:1), `#fb923c` with `#171717` text in dark (7.9:1).
- Connection colours (D8) have their tokens (`conn-*`), but a saved connection does not store a colour yet. Until it does, the toolbar shows no stripe: a grey one for every connection told the user nothing.
- The canvas grid and CodeMirror read the same tokens: `readTheme()` in `engine.ts`, and `var(--…)` in the editor's theme.

## Alternatives considered

- **A component library** (Bits UI, shadcn-svelte): more JavaScript than the few parts the app needs, and its look would still need the same tokens.
- **Segoe UI Variable and Fluent icons:** native, but they do not match the ERP Tinh gọn language the user chose, and Segoe is not on every machine.
- **Tailwind classes in each component:** this was how the first screens were written, and each screen drifted from the others.
