<script lang="ts">
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, HistoryEntry } from "../types";
  import Icon from "../ui/Icon.svelte";

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

<div class="pb-search">
  <Icon name="search" />
  <input placeholder="Find in history" aria-label="Find in history" spellcheck="false" bind:value={text} oninput={search} />
</div>
<div class="pb-side-tools">
  <label class="pb-check"><input type="checkbox" bind:checked={onlyThis} /> This connection</label>
  <span class="pb-spacer"></span>
  {#if confirmClear}
    <span class="pb-meta">Clear it all?</span>
    <button class="pb-btn pb-btn--ghost pb-btn--sm is-danger" onclick={clearAll}>Clear</button>
    <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => (confirmClear = false)}>Keep</button>
  {:else}
    <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => (confirmClear = true)}>Clear</button>
  {/if}
</div>
<div class="pb-side-body">
  {#if error}
    <div class="pb-tree-error"><span class="pb-tree-error-title">{errorTitle(error)}</span><span>{error.message}</span></div>
  {/if}
  <div class="pb-list">
    {#each entries as entry (entry.id)}
      <div class="pb-list-entry">
        <button class="pb-list-item" data-tip-start="" data-tip="Double-click to put it in the editor" ondblclick={() => onInsert(entry.statement)}>
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
        </button>
        <span class="pb-list-actions">
          <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => onInsert(entry.statement)}>Insert</button>
          <button class="pb-btn pb-btn--ghost pb-btn--sm" onclick={() => onRun(entry)}><Icon name="play" />Run</button>
          <span class="pb-spacer"></span>
          <button class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Delete from the history" onclick={() => remove(entry)}
            ><Icon name="trash" /></button
          >
        </span>
      </div>
    {:else}
      {#if !error}<p class="pb-side-hint">No queries yet. What you run from the editor shows here.</p>{/if}
    {/each}
    {#if more}
      <button class="pb-btn pb-btn--link" style="margin: 8px 12px" onclick={() => load(true)}>Load more</button>
    {/if}
  </div>
</div>
