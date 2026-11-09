# 0003: Tauri 2 and Svelte 5 for the Windows app

- **Status**: Accepted
- **Date**: 2026-11-09

## Context

The Windows app must feel fast and light, and its author works in TypeScript more than in a native toolkit. Options considered: WinUI 3 (C#), a Rust native toolkit (gpui, Slint, iced, egui), Go with Wails or Fyne, and Tauri.

Measured or known facts:

- WinUI 3 and Rust native toolkits draw through the system compositor with no browser engine. Their idle memory floor is lower than a WebView2 app's, which runs several Edge processes.
- Most of a database client's memory is its result sets, which live in Rust whatever the UI.
- Text input with a Vietnamese IME (Unikey, EVKey) works in WebView2 out of the box. A custom-drawn editor must implement TSF itself, a large layer of its own.
- Tauri 3 is in alpha on this date; Tauri 2 is stable.

## Decision

- Tauri 2, over WebView2.
- Svelte 5 with TypeScript, Vite and Tailwind CSS 4.
- CodeMirror 6 for the SQL editor.
- A data grid drawn on a canvas (`.claude/rules/data-grid.md`).

## Rationale

- Tauri keeps the app in Rust, so the core is called directly with no FFI layer.
- Svelte 5 over Solid: equal for this app's speed, since the grid and editor sit outside the framework; Svelte is easier to learn from a little React, has the larger set of libraries and documentation, and compiles to a small bundle.
- CodeMirror 6 over Monaco: a fraction of the size, with SQL dialects, multiple cursors and a Vim mode available.

## Consequences

- WebView2's idle memory is the floor. Keeping the UI thin keeps the rest small.
- The core stays UI-free (0001), so a native UI can replace this one later without touching it.
- A large result is never parsed in JavaScript (0002).

## Alternatives considered

- **WinUI 3.** Lightest of the native options with a mature toolkit, but C# with an FFI layer to the core, and the author's TypeScript does not carry over.
- **Go UI with a Rust core.** Wails is WebView2 as well, so no lighter. Fyne and Gio draw their own widgets with no native look. Go and Rust together need cgo and two runtimes in one process.
- **A Rust native UI (gpui, Slint, iced).** The lightest, but young widget sets, a SQL editor and IME to build, and no use of the author's TypeScript.
- **Solid.** Its JSX looks like React but behaves differently (components run once; destructured props lose reactivity), which trips someone coming from React; a smaller ecosystem.
