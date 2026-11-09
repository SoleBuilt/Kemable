<script lang="ts">
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, HistoryEntry } from "../types";

  interface Props {
    /** The connection's title: the list shows its entries, or every entry. */
    connection: string | null;
    /** Bumped after each run, so the list reads the history again. */
    refreshKey: number;
    onInsert: (sql: string) => void;
    onRun: (entry: HistoryEntry) => void;
  }

  let { connection, refreshKey, onInsert, onRun }: Props = $props();

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

<div class="flex h-full flex-col text-[12px]">
  <div class="flex flex-col gap-1.5 border-b border-line p-2">
    <input
      class="h-7 w-full rounded border border-line bg-surface px-2 outline-none select-text placeholder:text-muted focus:border-accent"
      placeholder="Search history"
      spellcheck="false"
      bind:value={text}
      oninput={search}
    />
    <div class="flex items-center gap-2 text-[11.5px] text-muted">
      <label class="flex items-center gap-1"><input type="checkbox" bind:checked={onlyThis} /> This connection</label>
      <span class="ml-auto">
        {#if confirmClear}
          Clear all?
          <button class="rounded px-1 text-danger hover:bg-surface" onclick={clearAll}>Yes</button>
          <button class="rounded px-1 hover:bg-surface" onclick={() => (confirmClear = false)}>No</button>
        {:else}
          <button class="rounded px-1 hover:bg-surface-3 hover:text-fg" onclick={() => (confirmClear = true)}>Clear</button>
        {/if}
      </span>
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-auto">
    {#if error}
      <div class="px-3 py-2">
        <div class="font-semibold text-danger">{errorTitle(error)}</div>
        <div class="text-[11.5px] wrap-break-word text-muted">{error.message}</div>
      </div>
    {/if}
    {#each entries as entry (entry.id)}
      <div class="group border-b border-line/60 px-2 py-1.5 hover:bg-surface-3">
        <button
          class="block w-full text-left"
          title={`${entry.statement}\n\nDouble-click to insert it in the editor`}
          ondblclick={() => onInsert(entry.statement)}
        >
          <span class="block truncate font-mono">{firstLine(entry.statement)}</span>
          <span class="flex gap-2 text-[11px] text-muted">
            <span class:text-danger={entry.outcome === "failed"}>
              {entry.outcome === "succeeded" ? "✓" : entry.outcome === "failed" ? "✕" : "■"}
            </span>
            <span>{time.format(entry.ranAt)}</span>
            <span>{number.format(entry.durationMs)} ms</span>
            {#if entry.rows !== null}<span>{number.format(entry.rows)} rows</span>{/if}
            {#if entry.database}<span class="truncate">{entry.database}</span>{/if}
          </span>
        </button>
        <span class="hidden gap-1 pt-1 text-[11px] group-focus-within:flex group-hover:flex">
          <button class="rounded px-1.5 hover:bg-surface hover:text-accent" onclick={() => onInsert(entry.statement)}>Insert</button>
          <button class="rounded px-1.5 hover:bg-surface hover:text-accent" onclick={() => onRun(entry)}>Run</button>
          <button class="ml-auto rounded px-1.5 text-muted hover:bg-surface hover:text-danger" onclick={() => remove(entry)}
            >Delete</button
          >
        </span>
      </div>
    {:else}
      {#if !error}<div class="p-3 text-muted">No queries yet. What you run from the editor shows here.</div>{/if}
    {/each}
    {#if more}
      <button class="w-full py-2 text-accent hover:bg-surface-3" onclick={() => load(true)}>Load more</button>
    {/if}
  </div>
</div>
