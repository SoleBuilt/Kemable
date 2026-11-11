<script lang="ts">
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, HistoryEntry } from "../types";
  import Checkbox from "../ui/Checkbox.svelte";
  import Button from "../ui/Button.svelte";
  import Confirm from "../ui/Confirm.svelte";
  import ErrorNote from "../ui/ErrorNote.svelte";
  import Icon from "../ui/Icon.svelte";
  import ListItem from "../ui/ListItem.svelte";
  import Note from "../ui/Note.svelte";
  import SearchField from "../ui/SearchField.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    /** The connection's title: the list shows its entries, or every entry. */
    connection: string | null;
    /** Bumped after each run, so the list reads the history again. */
    refreshKey: number;
    onInsert: (sql: string) => void;
    onRun: (entry: HistoryEntry) => void;
    /** How many entries are loaded, for the pane's header. */
    onCount?: (count: number) => void;
  }

  let { connection, refreshKey, onInsert, onRun, onCount }: Props = $props();

  const PAGE = 100;

  let entries: HistoryEntry[] = $state.raw([]);
  let text = $state("");
  let onlyThis = $state(true);
  let more = $state(false);
  let error: CoreError | null = $state(null);
  let confirmClear = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function load(append = false) {
    try {
      const before = append ? (entries[entries.length - 1]?.id ?? null) : null;
      const page = await ipc.historySearch({
        text: text.trim(),
        connection: onlyThis ? connection : null,
        before,
        limit: PAGE,
      });
      entries = append ? [...entries, ...page] : page;
      more = page.length === PAGE;
      error = null;
      onCount?.(entries.length);
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  $effect(() => {
    void refreshKey;
    void onlyThis;
    void connection;
    void load();
  });

  function search() {
    clearTimeout(timer);
    timer = setTimeout(() => void load(), 200);
  }

  async function remove(entry: HistoryEntry) {
    await ipc.historyDelete(entry.id);
    entries = entries.filter((other) => other.id !== entry.id);
    onCount?.(entries.length);
  }

  async function clearAll() {
    confirmClear = false;
    await ipc.historyDelete(null);
    await load();
  }

  const time = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });
  const number = new Intl.NumberFormat();

  function firstLine(sql: string): string {
    const line = sql.trim().split("\n")[0] ?? "";
    return line.length > 120 ? `${line.slice(0, 120)}…` : line;
  }
</script>

<SearchField placeholder="Find in history" bind:value={text} oninput={search} />
<div class="pb-side-tools">
  <Checkbox bind:checked={onlyThis}>This connection</Checkbox>
  <Spacer />
  {#if confirmClear}
    <Confirm question="Clear it all?" action="Clear" onConfirm={clearAll} onCancel={() => (confirmClear = false)} />
  {:else}
    <Button variant="ghost" size="sm" onclick={() => (confirmClear = true)}>Clear</Button>
  {/if}
</div>
<div class="pb-side-body">
  {#if error}<ErrorNote {error} />{/if}
  <div class="pb-list">
    {#each entries as entry (entry.id)}
      <ListItem tip="Double-click to put it in the editor" ondblclick={() => onInsert(entry.statement)}>
        <span class="pb-list-line">
          {#if entry.outcome === "succeeded"}
            <Icon name="check-circle" size="sm" class="pb-ok" />
          {:else if entry.outcome === "failed"}
            <Icon name="error" size="sm" class="pb-bad" />
          {:else}
            <Icon name="stop" size="sm" class="pb-muted" />
          {/if}
          <span class="pb-meta pb-num">{time.format(entry.ranAt)}</span>
        </span>
        <span class="pb-code-line">{firstLine(entry.statement)}</span>
        <span class="pb-meta"
          >{[entry.database, `${number.format(entry.durationMs)} ms`, entry.rows === null ? null : `${number.format(entry.rows)} rows`]
            .filter(Boolean)
            .join(" · ")}</span
        >
        {#snippet actions()}
          <Button variant="ghost" size="sm" onclick={() => onInsert(entry.statement)}>Insert</Button>
          <Button variant="ghost" size="sm" icon="play" onclick={() => onRun(entry)}>Run</Button>
          <Spacer />
          <Button variant="ghost" size="sm" icon="trash" label="Delete from the history" onclick={() => remove(entry)} />
        {/snippet}
      </ListItem>
    {:else}
      {#if !error}<Note>No queries yet. What you run from the editor shows here.</Note>{/if}
    {/each}
    {#if more}
      <Button variant="link" style="margin: 8px 12px" onclick={() => load(true)}>Load more</Button>
    {/if}
  </div>
</div>
