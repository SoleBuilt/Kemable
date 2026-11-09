<script lang="ts">
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedQuery } from "../types";

  interface Props {
    /** The saved connection open now; its queries and the global ones show. */
    connectionId: string | null;
    /** Bumped after a save, so the list reads the file again. */
    refreshKey: number;
    onInsert: (sql: string) => void;
    onRun: (query: SavedQuery) => void;
    onEdit: (query: SavedQuery) => void;
  }

  let { connectionId, refreshKey, onInsert, onRun, onEdit }: Props = $props();

  let queries: SavedQuery[] = $state.raw([]);
  let filter = $state("");
  let error: CoreError | null = $state(null);
  let confirming: string | null = $state(null);

  $effect(() => {
    void refreshKey;
    const id = connectionId;
    ipc.listQueries(id).then(
      (list) => {
        queries = list;
        error = null;
      },
      (caught: unknown) => (error = asCoreError(caught)),
    );
  });

  const shown = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    return needle
      ? queries.filter((query) => query.name.toLowerCase().includes(needle) || query.sql.toLowerCase().includes(needle))
      : queries;
  });

  async function remove(query: SavedQuery) {
    confirming = null;
    try {
      await ipc.deleteQuery(query.id);
      queries = queries.filter((other) => other.id !== query.id);
    } catch (caught) {
      error = asCoreError(caught);
    }
  }
</script>

<div class="flex h-full flex-col text-[12px]">
  <div class="border-b border-line p-2">
    <input
      class="h-7 w-full rounded border border-line bg-surface px-2 outline-none select-text placeholder:text-muted focus:border-accent"
      placeholder="Filter saved queries"
      spellcheck="false"
      bind:value={filter}
    />
  </div>
  <div class="min-h-0 flex-1 overflow-auto py-1">
    {#if error}
      <div class="px-3 py-2">
        <div class="font-semibold text-danger">{errorTitle(error)}</div>
        <div class="text-[11.5px] wrap-break-word text-muted">{error.message}</div>
      </div>
    {/if}
    {#each shown as query (query.id)}
      <div class="group px-2 py-1 hover:bg-surface-3">
        <button
          class="block w-full text-left"
          title={`${query.sql}\n\nDouble-click to insert it in the editor`}
          ondblclick={() => onInsert(query.sql)}
        >
          <span class="flex items-center gap-1.5">
            <span class="truncate">{query.name}</span>
            {#if query.connectionId === null}<span class="text-[10px] text-muted">all connections</span>{/if}
          </span>
          <span class="block truncate font-mono text-[11px] text-muted">{query.sql.trim().split("\n")[0]}</span>
        </button>
        {#if confirming === query.id}
          <span class="flex items-center gap-1 pt-1 text-[11px]">
            Delete?
            <button class="rounded px-1.5 text-danger hover:bg-surface" onclick={() => remove(query)}>Yes</button>
            <button class="rounded px-1.5 hover:bg-surface" onclick={() => (confirming = null)}>No</button>
          </span>
        {:else}
          <span class="hidden gap-1 pt-1 text-[11px] group-focus-within:flex group-hover:flex">
            <button class="rounded px-1.5 hover:bg-surface hover:text-accent" onclick={() => onInsert(query.sql)}>Insert</button>
            <button class="rounded px-1.5 hover:bg-surface hover:text-accent" onclick={() => onRun(query)}>Run</button>
            <button class="rounded px-1.5 hover:bg-surface hover:text-accent" onclick={() => onEdit(query)}>Edit</button>
            <button class="ml-auto rounded px-1.5 text-muted hover:bg-surface hover:text-danger" onclick={() => (confirming = query.id)}
              >Delete</button
            >
          </span>
        {/if}
      </div>
    {:else}
      {#if !error}
        <div class="p-3 leading-relaxed text-muted">
          No saved queries. Write one in the editor and press Ctrl+Shift+D to keep it.
        </div>
      {/if}
    {/each}
  </div>
</div>
