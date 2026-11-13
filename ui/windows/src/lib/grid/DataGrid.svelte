<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { type CellBox, type EditOverlay, GridEngine, type GridCallbacks, type GridSource, type SortKey } from "./engine";

  interface Props {
    source: GridSource | null;
    totalRows: number;
    /** The sort the header shows. */
    sortKeys: SortKey[];
    callbacks: GridCallbacks;
    /** Unsaved edits drawn over the rows; null when there are none. */
    edits?: EditOverlay | null;
    /** Keep the last row in view as rows arrive (a Live tab that follows its newest event). */
    followEnd?: boolean;
  }

  let { source, totalRows, sortKeys, callbacks, edits = null, followEnd = false }: Props = $props();

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

  /** Where a cell is drawn, scrolled into view, for an editor laid over it. */
  export function cellBox(row: number, column: number): CellBox | null {
    return engine?.cellBox({ row, column }) ?? null;
  }

  /** Gives the keyboard back to the grid, after an editor closes. */
  export function focus() {
    scroller.focus();
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
    // On a new engine too: the grid mounts again after the Messages tab.
    const current = engine;
    const overlay = edits;
    untrack(() => current?.setEdits(overlay));
  });

  $effect(() => {
    const rows = totalRows;
    const follow = followEnd;
    untrack(() => {
      engine?.setTotalRows(rows);
      if (follow) engine?.showLastRow();
    });
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
    ondblclick={(event) => engine?.doubleClick(event)}
  >
    <div bind:this={spacer}></div>
  </div>
  <canvas bind:this={canvas} class="pointer-events-none absolute top-0 left-0"></canvas>
</div>

<style>
  .grid-scroller {
    scrollbar-width: thin;
    scrollbar-color: var(--scroll-thumb) transparent;
  }
</style>
