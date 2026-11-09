<script lang="ts" module>
  /** What a run sends: the text and, for "run statement", the cursor's UTF-8 byte offset. */
  export interface RunRequest {
    sql: string;
    cursor: number | null;
    /** Where `sql` starts in the editor, in UTF-16 units: 0 for the whole text, the selection start for a selection. */
    origin: number;
    /** Keep every row: no row cap (Ctrl+Alt+Enter, Fetch All). */
    unlimited?: boolean;
  }
</script>

<script lang="ts">
  import { indentWithTab } from "@codemirror/commands";
  import { MSSQL, PostgreSQL, SQLite, sql, type SQLDialect } from "@codemirror/lang-sql";
  import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
  import { Compartment, EditorState, Prec, StateEffect, StateField } from "@codemirror/state";
  import { Decoration, type DecorationSet, EditorView, keymap } from "@codemirror/view";
  import { tags } from "@lezer/highlight";
  import { basicSetup } from "codemirror";
  import { onMount } from "svelte";
  import type { ByteRange, SqlGrammar } from "../types";

  interface Props {
    grammar: SqlGrammar | null;
    initial?: string;
    onRun: (request: RunRequest) => void;
  }

  let { grammar, initial = "", onRun }: Props = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  const language = new Compartment();
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  const DIALECTS: Record<SqlGrammar, SQLDialect> = {
    postgres: PostgreSQL,
    sqlServer: MSSQL,
    sqlite: SQLite,
  };

  // Colours come from the CSS tokens, so the editor follows the theme with the rest of the app.
  const highlight = HighlightStyle.define([
    { tag: tags.keyword, color: "var(--syntax-keyword)" },
    { tag: [tags.string, tags.special(tags.string)], color: "var(--syntax-string)" },
    { tag: [tags.number, tags.bool, tags.null], color: "var(--syntax-number)" },
    { tag: [tags.lineComment, tags.blockComment], color: "var(--syntax-comment)", fontStyle: "italic" },
    { tag: [tags.typeName, tags.standard(tags.name)], color: "var(--syntax-type)" },
    { tag: tags.operator, color: "var(--syntax-operator)" },
    { tag: [tags.name, tags.special(tags.name)], color: "var(--syntax-name)" },
  ]);

  const theme = EditorView.theme({
    "&": { height: "100%", backgroundColor: "var(--surface)", color: "var(--text)", fontSize: "13px" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55" },
    ".cm-content": { caretColor: "var(--accent)" },
    ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
    ".cm-gutters": { backgroundColor: "var(--surface)", color: "var(--text-muted)", border: "none" },
    ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--active-line)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: "var(--selection) !important",
    },
    ".cm-tooltip": { backgroundColor: "var(--surface-2)", border: "1px solid var(--border)", color: "var(--text)" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "var(--accent)",
      color: "var(--accent-text)",
    },
  });

  function languageFor(value: SqlGrammar | null) {
    return sql({ dialect: value ? DIALECTS[value] : PostgreSQL, upperCaseKeywords: false });
  }

  /** The run for Ctrl+Enter (`statement`) or Ctrl+Shift+Enter (`all`). A selection always runs as it is. */
  export function request(mode: "statement" | "all"): RunRequest | null {
    if (!view) return null;
    const state = view.state;
    const selection = state.selection.main;
    if (!selection.empty) return { sql: state.sliceDoc(selection.from, selection.to), cursor: null, origin: selection.from };
    const text = state.doc.toString();
    if (text.trim() === "") return null;
    if (mode === "all") return { sql: text, cursor: null, origin: 0 };
    // The core splits in UTF-8 bytes; CodeMirror counts UTF-16 units.
    return { sql: text, cursor: encoder.encode(text.slice(0, selection.head)).length, origin: 0 };
  }

  function run(mode: "statement" | "all", unlimited = false): boolean {
    const next = request(mode);
    if (next) onRun({ ...next, unlimited });
    return true;
  }

  export function focus(): void {
    view?.focus();
  }

  /** The whole text, or the selection when there is one. */
  export function text(): string {
    if (!view) return "";
    const selection = view.state.selection.main;
    return selection.empty ? view.state.doc.toString() : view.state.sliceDoc(selection.from, selection.to);
  }

  /** Puts `sql` in place of the selection, on its own lines, and selects it. */
  export function insert(sql: string): void {
    if (!view) return;
    const state = view.state;
    const { from, to } = state.selection.main;
    const before = from > 0 && state.sliceDoc(from - 1, from) !== "\n" ? "\n" : "";
    const after = to < state.doc.length && state.sliceDoc(to, to + 1) !== "\n" ? "\n" : "";
    const start = from + before.length;
    view.dispatch({
      changes: { from, to, insert: `${before}${sql}${after}` },
      selection: { anchor: start, head: start + sql.length },
      scrollIntoView: true,
    });
    view.focus();
  }

  // The statements a run sent, marked for a moment so it is plain what ran.
  const setRan = StateEffect.define<{ from: number; to: number }[]>();
  const ranMark = Decoration.mark({ class: "cm-ran" });
  const ranField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(marks, transaction) {
      let next = marks.map(transaction.changes);
      for (const effect of transaction.effects) {
        if (effect.is(setRan)) {
          next = Decoration.set(
            effect.value.filter((range) => range.to > range.from).map((range) => ranMark.range(range.from, range.to)),
            true,
          );
        }
      }
      return next;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
  let ranTimer: ReturnType<typeof setTimeout> | undefined;

  /** Marks what a run sent: `ranges` are UTF-8 byte ranges into `request.sql`. */
  export function showRan(request: RunRequest, ranges: ByteRange[]): void {
    if (!view) return;
    const bytes = encoder.encode(request.sql);
    const units = (byte: number) => decoder.decode(bytes.subarray(0, byte)).length;
    const length = view.state.doc.length;
    const marks = ranges.map((range) => ({
      from: Math.min(length, request.origin + units(range.start)),
      to: Math.min(length, request.origin + units(range.end)),
    }));
    view.dispatch({ effects: setRan.of(marks) });
    clearTimeout(ranTimer);
    ranTimer = setTimeout(() => view?.dispatch({ effects: setRan.of([]) }), 1400);
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: initial,
        extensions: [
          Prec.highest(
            keymap.of([
              { key: "Mod-Enter", run: () => run("statement") },
              { key: "Mod-Alt-Enter", run: () => run("statement", true) },
              { key: "Shift-Mod-Enter", run: () => run("all") },
              { key: "F5", run: () => run("all") },
            ]),
          ),
          basicSetup,
          keymap.of([indentWithTab]),
          language.of(languageFor(grammar)),
          syntaxHighlighting(highlight),
          theme,
          ranField,
        ],
      }),
    });
    return () => view?.destroy();
  });

  $effect(() => {
    const value = grammar;
    view?.dispatch({ effects: language.reconfigure(languageFor(value)) });
  });
</script>

<div bind:this={host} class="h-full w-full overflow-hidden select-text"></div>
