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
  import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
  import { indentWithTab } from "@codemirror/commands";
  import { MSSQL, PostgreSQL, SQLite, sql, type SQLDialect } from "@codemirror/lang-sql";
  import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
  import { Compartment, EditorState, Prec, StateEffect, StateField } from "@codemirror/state";
  import { Decoration, type DecorationSet, EditorView, keymap } from "@codemirror/view";
  import { tags } from "@lezer/highlight";
  import { basicSetup } from "codemirror";
  import { onMount } from "svelte";
  import type { ByteRange, CompletionKind, CompletionReply, SqlGrammar } from "../types";

  interface Props {
    grammar: SqlGrammar | null;
    /** Commands, not SQL (Redis): no SQL highlighting or keywords. */
    plain?: boolean;
    initial?: string;
    onRun: (request: RunRequest) => void;
    /** Names from the catalog at a UTF-8 byte offset (docs/decisions/0007-autocomplete.md). */
    complete?: (text: string, cursor: number) => Promise<CompletionReply | null>;
    /** The text changed: typed, pasted, undone. */
    onChange?: () => void;
  }

  let { grammar, plain = false, initial = "", onRun, complete, onChange }: Props = $props();

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
    "&": { height: "100%", backgroundColor: "var(--canvas)", color: "var(--fg)", fontSize: "13px" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "20px" },
    ".cm-content": { caretColor: "var(--accent)" },
    ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
    ".cm-gutters": { backgroundColor: "var(--canvas)", color: "var(--fg-faint)", border: "none" },
    ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "var(--active-line)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
      backgroundColor: "var(--selection) !important",
    },
    ".cm-tooltip": {
      backgroundColor: "var(--raised)",
      border: "none",
      borderRadius: "var(--radius-md)",
      boxShadow: "var(--shadow-popover)",
      color: "var(--fg)",
      overflow: "hidden",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "var(--selection)",
      color: "var(--fg)",
    },
    // The statement behind the result shown: a thin frame and a faint tint, behind the text (D23).
    ".cm-stmt": { backgroundColor: "var(--muted)", boxShadow: "inset 1px 0 var(--input), inset -1px 0 var(--input)" },
    ".cm-stmt.cm-stmt-first": {
      boxShadow: "inset 1px 0 var(--input), inset -1px 0 var(--input), inset 0 1px var(--input)",
      borderTopLeftRadius: "var(--radius-sm)",
      borderTopRightRadius: "var(--radius-sm)",
    },
    ".cm-stmt.cm-stmt-last": {
      boxShadow: "inset 1px 0 var(--input), inset -1px 0 var(--input), inset 0 -1px var(--input)",
      borderBottomLeftRadius: "var(--radius-sm)",
      borderBottomRightRadius: "var(--radius-sm)",
    },
    ".cm-stmt.cm-stmt-first.cm-stmt-last": { boxShadow: "inset 0 0 0 1px var(--input)" },
  });

  /** CodeMirror's icon for each kind of name. */
  const ICONS: Record<CompletionKind, string> = {
    schema: "namespace",
    table: "class",
    view: "interface",
    column: "property",
    alias: "variable",
  };
  /** Past this size, completion would send too much text on each word. */
  const MAX_COMPLETE_CHARS = 2_000_000;
  const WORD = /^[\p{L}\p{N}_$]*$/u;

  /**
   * Names from the catalog, beside lang-sql's keywords. It asks Rust only as
   * a word starts (validFor keeps the list while the word goes on), and opens
   * by itself only after a letter, a dot or a quote; Ctrl+Space always opens it.
   */
  async function catalogNames(context: CompletionContext): Promise<CompletionResult | null> {
    const ask = complete;
    if (!ask) return null;
    const word = context.matchBefore(/[\p{L}\p{N}_$]+$/u);
    const before = context.state.sliceDoc(Math.max(0, context.pos - 1), context.pos);
    if (!context.explicit && !word && !/[."[`]/.test(before)) return null;
    if (context.state.doc.length > MAX_COMPLETE_CHARS) return null;
    const head = context.state.sliceDoc(0, context.pos);
    const bytes = encoder.encode(head);
    const reply = await ask(context.state.doc.toString(), bytes.length);
    if (!reply || context.aborted || reply.items.length === 0) return null;
    const typed = decoder.decode(bytes.subarray(reply.from)).length;
    const options: Completion[] = reply.items.map((item) => ({
      label: item.label,
      apply: item.apply,
      type: ICONS[item.kind],
      detail: item.detail,
      boost: item.boost,
    }));
    return { from: context.pos - typed, options, validFor: WORD };
  }

  function languageFor(value: SqlGrammar | null, commands: boolean) {
    if (commands) return [];
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

  // The statement a result tab came from, framed while that tab is shown.
  const setFrame = StateEffect.define<{ from: number; to: number } | null>();
  const frameField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(lines, transaction) {
      let next = lines.map(transaction.changes);
      for (const effect of transaction.effects) {
        if (!effect.is(setFrame)) continue;
        const range = effect.value;
        if (!range || range.to <= range.from) {
          next = Decoration.none;
          continue;
        }
        const doc = transaction.state.doc;
        const first = doc.lineAt(range.from).number;
        // A statement's range ends after its last character, which may sit at the start of the next line.
        const last = doc.lineAt(Math.max(range.from, range.to - 1)).number;
        const marks = [];
        for (let number = first; number <= last; number++) {
          const classes = ["cm-stmt", number === first ? "cm-stmt-first" : "", number === last ? "cm-stmt-last" : ""];
          marks.push(Decoration.line({ class: classes.join(" ").trim() }).range(doc.line(number).from));
        }
        next = Decoration.set(marks);
      }
      return next;
    },
    provide: (field) => EditorView.decorations.from(field),
  });

  /** Frames the statement at `range` (UTF-8 bytes into `request.sql`), or clears the frame. */
  export function frame(request: RunRequest | null, range: ByteRange | null): void {
    if (!view) return;
    if (!request || !range) {
      view.dispatch({ effects: setFrame.of(null) });
      return;
    }
    const bytes = encoder.encode(request.sql);
    const units = (byte: number) => decoder.decode(bytes.subarray(0, byte)).length;
    const length = view.state.doc.length;
    view.dispatch({
      effects: setFrame.of({
        from: Math.min(length, request.origin + units(range.start)),
        to: Math.min(length, request.origin + units(range.end)),
      }),
    });
  }

  function makeState(doc: string): EditorState {
    return EditorState.create({
      doc,
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
          language.of(languageFor(grammar, plain)),
          syntaxHighlighting(highlight),
          EditorState.languageData.of(() => [{ autocomplete: catalogNames }]),
          theme,
          ranField,
          frameField,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChange?.();
          }),
        ],
    });
  }

  /**
   * One editor serves every query tab: a tab keeps its own state (text, selection,
   * undo history) while another is shown (docs/decisions/0013-pages-and-tabs.md).
   */
  export function saveState(): EditorState | null {
    return view?.state ?? null;
  }

  /** A tab's state from kept text, with the cursor at `cursor` (cut to the text): a page brought back at launch. */
  export function stateWith(text: string, cursor: number): EditorState {
    const state = makeState(text);
    const at = Math.max(0, Math.min(cursor, state.doc.length));
    return state.update({ selection: { anchor: at } }).state;
  }

  /** Shows `state` (a tab's saved one), or a fresh empty editor when null. */
  export function restoreState(state: EditorState | null): void {
    if (!view) return;
    view.setState(state ?? makeState(""));
    view.dispatch({ effects: language.reconfigure(languageFor(grammar, plain)) });
  }

  onMount(() => {
    view = new EditorView({ parent: host, state: makeState(initial) });
    return () => view?.destroy();
  });

  $effect(() => {
    const value = grammar;
    const commands = plain;
    view?.dispatch({ effects: language.reconfigure(languageFor(value, commands)) });
  });
</script>

<div bind:this={host} class="h-full w-full overflow-hidden select-text"></div>
