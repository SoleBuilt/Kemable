<script lang="ts">
  import { ipc } from "../ipc";
  import type { ColumnMeta } from "../types";

  interface Props {
    result: number;
    columns: ColumnMeta[];
    /** The cell cursor's row and column; null when nothing is selected. */
    row: number | null;
    column: number | null;
    onPickColumn: (column: number) => void;
  }

  let { result, columns, row, column, onPickColumn }: Props = $props();

  /** A long value shows this much in the list; the focused field shows in full. */
  const LIST_CHARS = 600;

  let values: (string | null)[] = $state.raw([]);
  let loadedFor = "";
  let timer: ReturnType<typeof setTimeout> | undefined;
  let copied = $state(false);

  // Holding an arrow key moves the cursor fast; read the row once it settles.
  $effect(() => {
    const key = row === null ? "" : `${result}:${row}`;
    clearTimeout(timer);
    if (key === "" || row === null) {
      values = [];
      loadedFor = "";
      return;
    }
    if (key === loadedFor) return;
    const target = row;
    timer = setTimeout(() => {
      void ipc.rowValues(result, target).then(
        (read) => {
          values = read;
          loadedFor = key;
        },
        () => (values = []),
      );
    }, 60);
  });

  const focused = $derived(column === null ? undefined : values[column]);
  const focusedMeta = $derived(column === null ? undefined : columns[column]);

  /** JSON shown indented, when the value is JSON; anything else as it is. */
  function pretty(text: string, meta: ColumnMeta | undefined): string {
    const trimmed = text.trimStart();
    if (meta?.kind !== "json" && !(trimmed.startsWith("{") || trimmed.startsWith("["))) return text;
    try {
      return JSON.stringify(JSON.parse(text), null, 2);
    } catch {
      return text;
    }
  }

  function short(text: string): string {
    return text.length > LIST_CHARS ? `${text.slice(0, LIST_CHARS)}…` : text;
  }

  async function copyFocused() {
    if (focused === undefined) return;
    await navigator.clipboard.writeText(focused ?? "");
    copied = true;
    setTimeout(() => (copied = false), 1200);
  }
</script>

<div class="flex h-full flex-col bg-surface text-[12px]">
  {#if row === null}
    <div class="p-3 text-muted">Select a cell to see its row.</div>
  {:else}
    <div class="flex h-8 shrink-0 items-center gap-2 border-b border-line px-3 text-muted">
      <span class="font-semibold text-fg">Row {row + 1}</span>
      <span class="truncate">{focusedMeta ? `${focusedMeta.name} · ${focusedMeta.typeName || focusedMeta.kind}` : ""}</span>
      <button class="ml-auto rounded px-1.5 hover:bg-surface-3 hover:text-fg" onclick={copyFocused} title="Copy the field's full value">
        {copied ? "Copied" : "Copy"}
      </button>
    </div>

    <div class="max-h-[45%] shrink-0 overflow-auto border-b border-line p-3 font-mono text-[12px] select-text">
      {#if focused === undefined}
        <span class="text-muted">…</span>
      {:else if focused === null}
        <span class="text-muted italic">NULL</span>
      {:else}
        <pre class="m-0 whitespace-pre-wrap wrap-break-word">{pretty(focused, focusedMeta)}</pre>
      {/if}
    </div>

    <div class="min-h-0 flex-1 overflow-auto py-1" aria-label="Fields of the row">
      {#each columns as meta, index (index)}
        {@const value = values[index]}
        <button
          class="block w-full px-3 py-1 text-left hover:bg-surface-3"
          class:bg-surface-3={index === column}
          onclick={() => onPickColumn(index)}
        >
          <span class="block truncate text-[11px] text-muted">{meta.name}</span>
          {#if value === null}
            <span class="text-muted italic">NULL</span>
          {:else if value !== undefined}
            <span class="block font-mono whitespace-pre-wrap wrap-break-word">{short(value)}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
