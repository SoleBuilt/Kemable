<script lang="ts">
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedQuery } from "../types";
  import Icon from "../ui/Icon.svelte";

  interface Props {
    /** The saved connection open now; its queries and the global ones show. */
    connectionId: string | null;
    /** Bumped after a save, so the list reads the file again. */
    refreshKey: number;
    onInsert: (sql: string) => void;
    onRun: (query: SavedQuery) => void;
    onEdit: (query: SavedQuery) => void;
    /** How many queries there are, for the pane's header. */
    onCount?: (count: number) => void;
  }

  let { connectionId, refreshKey, onInsert, onRun, onEdit, onCount }: Props = $props();

  let queries: SavedQuery[] = $state.raw([]);
  let filter = $state("");
  let error: CoreError | null = $state(null);
  let confirming: string | null = $state(null);
  let selected: string | null = $state(null);

  $effect(() => {
    void refreshKey;
    const id = connectionId;
    ipc.listQueries(id).then(
      (list) => {
        queries = list;
        error = null;
        onCount?.(list.length);
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
      onCount?.(queries.length);
    } catch (caught) {
      error = asCoreError(caught);
    }
  }
</script>

<div class="pb-search">
  <Icon name="search" />
  <input placeholder="Find a saved query" aria-label="Find a saved query" spellcheck="false" bind:value={filter} />
</div>
<div class="pb-side-body">
  {#if error}
    <div class="pb-tree-error"><span class="pb-tree-error-title">{errorTitle(error)}</span><span>{error.message}</span></div>
  {/if}
  <div class="pb-list">
    {#each shown as query (query.id)}
      <div class="pb-list-entry">
        <button
          class="pb-list-item"
          class:is-selected={selected === query.id}
          data-tip-start="" data-tip="Double-click to put it in the editor"
          onclick={() => (selected = query.id)}
          ondblclick={() => onInsert(query.sql)}
        >
          <span class="pb-list-line"
            ><Icon name="bookmark" class="pb-muted" /><span class="pb-list-name">{query.name}</span>{#if query.connectionId === null}<span
                class="pb-badge pb-badge--sm pb-badge--outline">all</span
              >{/if}</span
          >
          <span class="pb-code-line">{query.sql.trim().split("\n")[0]}</span>
        </button>
        <span class="pb-list-actions">
          {#if confirming === query.id}
            <span class="pb-meta">Delete it?</span>
            <button class="pb-btn pb-btn--ghost pb-btn--sm is-danger" onclick={() => remove(query)}>Delete</button>
            <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => (confirming = null)}>Keep</button>
          {:else}
            <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => onInsert(query.sql)}>Insert</button>
            <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => onRun(query)}><Icon name="play" />Run</button>
            <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => onEdit(query)}>Edit</button>
            <span class="pb-spacer"></span>
            <button class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Delete {query.name}" onclick={() => (confirming = query.id)}
              ><Icon name="trash" /></button
            >
          {/if}
        </span>
      </div>
    {:else}
      {#if !error}
        <p class="pb-side-hint">No saved queries. Write one in the editor and press Ctrl+Shift+D to keep it.</p>
      {/if}
    {/each}
  </div>
</div>
