<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { GridEngine, type GridCallbacks, type GridSource, type SortKey } from "./engine";

  interface Props {
    source: GridSource | null;
    totalRows: number;
    /** The sort the header shows. */
    sortKeys: SortKey[];
    callbacks: GridCallbacks;
  }

  let { source, totalRows, sortKeys, callbacks }: Props = $props();

  let scroller: HTMLDivElement;
  let spacer: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let engine: GridEngine | null = $state(null);

  /** Reads the rows again, after the core sorted them. */
  export function reload() {
    engine?.reload();
  }

  /** Moves the cell cursor, for the detail pane's field list. */
  export function focusCell(row: number, column: number) {
    engine?.focusCell({ row, column });
  }

  onMount(() => {
    const created = new GridEngine(scroller, spacer, canvas, callbacks);
    const observer = new ResizeObserver(() => created.resize(scroller.clientWidth, scroller.clientHeight));
    observer.observe(scroller);
    created.resize(scroller.clientWidth, scroller.clientHeight);
    engine = created;
    return () => {
      observer.disconnect();
      created.destroy();
    };
  });

  // The engine is imperative: Svelte only tells it what changed. A new source
  // resets the row count, so it takes the current count with it; two sets of
  // the same size would otherwise leave the count at zero.
  $effect(() => {
    const current = engine;
    const next = source;
    if (!current) return;
    untrack(() => {
      current.setSource(next);
      current.setTotalRows(totalRows);
    });
  });

  $effect(() => {
    const keys = sortKeys;
    untrack(() => engine?.setSortKeys(keys));
  });

  $effect(() => {
    const rows = totalRows;
    untrack(() => engine?.setTotalRows(rows));
  });
</script>

<div class="relative h-full w-full overflow-hidden bg-surface">
  <div
    bind:this={scroller}
    class="grid-scroller absolute inset-0 overflow-auto outline-none"
    tabindex="0"
    role="grid"
    aria-rowcount={totalRows}
    aria-colcount={source?.columns.length ?? 0}
    onpointerdown={(event) => {
      scroller.focus();
      engine?.pointerDown(event);
    }}
    onpointermove={(event) => engine?.pointerMove(event)}
    onpointerup={(event) => engine?.pointerUp(event)}
    onkeydown={(event) => engine?.keyDown(event)}
  >
    <div bind:this={spacer}></div>
  </div>
  <canvas bind:this={canvas} class="pointer-events-none absolute top-0 left-0"></canvas>
</div>

<style>
  .grid-scroller {
    scrollbar-width: thin;
    scrollbar-color: var(--surface-3) transparent;
  }
</style>
