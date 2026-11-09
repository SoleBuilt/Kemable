<script lang="ts" module>
  /** What a run sends: the text and, for "run statement", the cursor's UTF-8 byte offset. */
  export interface RunRequest {
    sql: string;
    cursor: number | null;
  }
</script>

<script lang="ts">
  import { indentWithTab } from "@codemirror/commands";
  import { MSSQL, PostgreSQL, SQLite, sql, type SQLDialect } from "@codemirror/lang-sql";
  import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
  import { Compartment, EditorState, Prec } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { tags } from "@lezer/highlight";
  import { basicSetup } from "codemirror";
  import { onMount } from "svelte";
  import type { SqlGrammar } from "../types";

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
    if (!selection.empty) return { sql: state.sliceDoc(selection.from, selection.to), cursor: null };
    const text = state.doc.toString();
    if (text.trim() === "") return null;
    if (mode === "all") return { sql: text, cursor: null };
    // The core splits in UTF-8 bytes; CodeMirror counts UTF-16 units.
    return { sql: text, cursor: encoder.encode(text.slice(0, selection.head)).length };
  }

  function run(mode: "statement" | "all"): boolean {
    const next = request(mode);
    if (next) onRun(next);
    return true;
  }

  export function focus(): void {
    view?.focus();
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
              { key: "Shift-Mod-Enter", run: () => run("all") },
              { key: "F5", run: () => run("all") },
            ]),
          ),
          basicSetup,
          keymap.of([indentWithTab]),
          language.of(languageFor(grammar)),
          syntaxHighlighting(highlight),
          theme,
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
