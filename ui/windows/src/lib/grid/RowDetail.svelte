<script lang="ts">
  import { ipc } from "../ipc";
  import type { ColumnMeta } from "../types";
  import Icon from "../ui/Icon.svelte";

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

<div class="pb-detail">
  {#if row === null}
    <p class="pb-side-hint">Select a cell to see its row.</p>
  {:else}
    <div class="pb-detail-head">
      Row {row + 1}
      <span class="pb-count">{focusedMeta ? `${focusedMeta.name} · ${focusedMeta.typeName || focusedMeta.kind}` : ""}</span>
      <span class="pb-spacer"></span>
      <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={copyFocused} data-tip-end="" data-tip="Copy the field's full value"
        ><Icon name={copied ? "check" : "copy"} />{copied ? "Copied" : "Copy"}</button
      >
    </div>
    <div class="pb-detail-value">
      {#if focused === undefined}
        <span class="pb-muted">…</span>
      {:else if focused === null}
        <span class="pb-null">NULL</span>
      {:else}
        <pre>{pretty(focused, focusedMeta)}</pre>
      {/if}
    </div>
    <div class="pb-detail-body" aria-label="Fields of the row">
      {#each columns as meta, index (index)}
        {@const value = values[index]}
        <button class="pb-field-row" class:is-selected={index === column} onclick={() => onPickColumn(index)}>
          <span>{meta.name}</span>
          {#if value === null}
            <span class="pb-null">NULL</span>
          {:else if value !== undefined}
            <span class="pb-mono">{short(value)}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
