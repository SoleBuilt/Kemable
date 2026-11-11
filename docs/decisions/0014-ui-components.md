# 0014: The Windows UI is built from small components in lib/ui; a page only puts them together, and the window draws its own title bar

- **Status**: Accepted
- **Date**: 2026-11-11
- **Follows**: `docs/decisions/0012-design-system.md` (the tokens and the `pb-*` classes) and `docs/decisions/0013-pages-and-tabs.md` (the pages).

## Context

After the screens were rebuilt on the design system, each screen still wrote its own markup with the `pb-*` classes: `<button class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label=… data-tip-end="" data-tip=…>`. `App.svelte` alone had more than 2,200 lines. A button, a field, a checkbox or a menu item was written again in every file. When one copy changed, the others did not.

The window also kept the system's title bar above the toolbar. That is two bars at the top, and the system bar holds only the title and the window's buttons.

## Decision

### Three layers

1. **Small parts in `ui/windows/src/lib/ui/`.** Each knows nothing about databases. It renders one design system class with typed props, as shadcn's components do.
   - Controls: `Button`, `Input`, `Textarea`, `SearchField`, `Checkbox`, `Segmented`, `Combobox`.
   - Labels and marks: `Field`, `Keys`, `Badge`, `Count`, `Dot`, `Icon`.
   - Menus: `Menu`, `MenuItem`, `MenuLabel`, `MenuSeparator`, `Popover`, `ContextMenu`.
   - Tabs, lists and trees: `TabList`, `Tab`, `ListItem`, `TreeRow`.
   - Surfaces: `Dialog`, `InfoBar`, `EmptyState`, `Setting`, `ErrorNote`, `Note`, `Confirm`.
   - Layout: `Splitter`, `Spacer`, `Separator`.
   - Built from the parts above: `CommandPalette` and `SettingsDialog`.
2. **Larger parts that know the app.**
   - In `lib/shell/`: `TitleBar`, `WindowControls`, `PaletteTrigger`, `PageTabs`, `SidePane`, `StatusBar`, `Welcome`.
   - Beside the feature they serve: `connect/ConnectionMenu`, `connect/PasswordDialog`, `editor/RunControls`, `grid/ResultBar`, `grid/WhereStrip`, `grid/MessageList`, `grid/KeyBar`, `grid/TableTools`, `grid/FilterRow`, `grid/Pager`.
   - They are built only from layer 1.
3. **Pages** (`App.svelte`, the dialogs, the sidebar lists) hold state and call the core. Their markup puts layers 1 and 2 together.

### Rules

- **Outside `lib/ui/`, no markup copies what a part draws.** That means no `<button class="pb-btn…">`, `<input class="pb-input">`, `<kbd>`, `pb-badge`, `pb-info`, `pb-menu-item` or `pb-dialog`. A page uses the part.
- **A tooltip** is a prop: `tip` and `tipAt` (`lib/ui/tip.ts`). Nothing writes `data-tip` attributes by hand, except where a part does not fit, such as a label in a strip.
- **Menus and lists are fixed to the window** (`lib/ui/place.ts`), placed by their trigger's box. A bar that clips its content, as the page bar does, would cut them off, and focusing an item in them would scroll that bar out of sight.
- **The CSS stays in `components.css`**, under the design system's class names, so the design system artifact and the app keep one set of rules. A part adds its class there, never a `<style>` block of its own.
- **A part that has only markup still gets a `<script lang="ts">` block.** Without one, svelte-check gives it no types, and `pnpm check` fails where it is imported.

### The title bar

- The window is undecorated (`"decorations": false` in `tauri.conf.json`).
- The toolbar is the title bar: the logo, the connection and database pickers, the search in the middle, and minimize, maximize or restore, and close drawn as Windows 11 draws them. Close turns `--window-close` red on hover.
- Its empty parts carry `data-tauri-drag-region`. They drag the window, and a double-click maximizes it.
- The buttons call `appWindow` in `ipc.ts`, so every call to Tauri stays there.
- The capability adds only `allow-minimize`, `allow-toggle-maximize`, `allow-close` and `allow-start-dragging`.

## Rationale

- One file per part means a change to a button reaches every button, and a page reads as what it shows.
- Typed props catch a wrong variant or a missing label in `pnpm check`. A misspelled class name fails silently.
- A title bar of our own puts the search and the connection where the eye lands first, and gives the window one bar at the top instead of two.

## Consequences

- `App.svelte` went from about 2,200 to about 1,900 lines. What is left is state and calls to the core; the markup is short.
- Windows 11's snap layouts flyout, shown when the pointer rests on the system maximize button, does not appear. Dragging the window to a screen edge still snaps it, and Tauri keeps the window's edges for resizing (checked by hand: `docs/testing/phase-1.md`, 8.5).
- A new part goes in `lib/ui/` with its class in `components.css`, and in the design system artifact when it is shown there.

## Alternatives considered

- **A component library** (shadcn-svelte, Bits UI): its own classes and Tailwind setup beside ours, and more code than the parts we need (ADR 0012).
- **A `<style>` block in each part:** Svelte scopes it, so the design system artifact could no longer share the rules, and a parent could not adjust a part (`.pb-status .pb-badge`).
- **Keeping the system title bar:** two bars at the top, and the title bar's room wasted.
