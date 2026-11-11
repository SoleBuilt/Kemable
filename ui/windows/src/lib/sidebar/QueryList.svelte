<script lang="ts">
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedQuery } from "../types";
  import Badge from "../ui/Badge.svelte";
  import Button from "../ui/Button.svelte";
  import Confirm from "../ui/Confirm.svelte";
  import ErrorNote from "../ui/ErrorNote.svelte";
  import Icon from "../ui/Icon.svelte";
  import ListItem from "../ui/ListItem.svelte";
  import Note from "../ui/Note.svelte";
  import SearchField from "../ui/SearchField.svelte";
  import Spacer from "../ui/Spacer.svelte";

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

<SearchField placeholder="Find a saved query" bind:value={filter} />
<div class="pb-side-body">
  {#if error}<ErrorNote {error} />{/if}
  <div class="pb-list">
    {#each shown as query (query.id)}
      <ListItem selected={selected === query.id} tip="Double-click to put it in the editor" onclick={() => (selected = query.id)} ondblclick={() => onInsert(query.sql)}>
        <span class="pb-list-line"
          ><Icon name="bookmark" class="pb-muted" /><span class="pb-list-name">{query.name}</span>{#if query.connectionId === null}<Badge size="sm" outline
              >all</Badge
            >{/if}</span
        >
        <span class="pb-code-line">{query.sql.trim().split("\n")[0]}</span>
        {#snippet actions()}
          {#if confirming === query.id}
            <Confirm question="Delete it?" action="Delete" onConfirm={() => remove(query)} onCancel={() => (confirming = null)} />
          {:else}
            <Button variant="ghost" size="sm" onclick={() => onInsert(query.sql)}>Insert</Button>
            <Button variant="ghost" size="sm" icon="play" onclick={() => onRun(query)}>Run</Button>
            <Button variant="ghost" size="sm" onclick={() => onEdit(query)}>Edit</Button>
            <Spacer />
            <Button variant="ghost" size="sm" icon="trash" label="Delete {query.name}" onclick={() => (confirming = query.id)} />
          {/if}
        {/snippet}
      </ListItem>
    {:else}
      {#if !error}<Note>No saved queries. Write one in the editor and press Ctrl+Shift+D to keep it.</Note>{/if}
    {/each}
  </div>
</div>
