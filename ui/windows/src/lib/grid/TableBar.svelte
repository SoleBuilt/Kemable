<script lang="ts" module>
  /** The page sizes on offer for a table tab. */
  export const PAGE_SIZES = [100, 500, 1_000, 5_000, 10_000];
</script>

<script lang="ts">
  import { tick } from "svelte";
  import type { Filter, FilterOp } from "../types";

  interface Props {
    /** The table's name as the tab shows it. */
    title: string;
    columns: string[];
    /** The filters the page was read with. */
    filters: Filter[];
    matchAny: boolean;
    offset: number;
    limit: number;
    /** Rows on this page once it has loaded; null while it loads. */
    rowsOnPage: number | null;
    /** Rows the filters match: a number, "counting", or null when unknown. */
    count: number | "counting" | null;
    busy: boolean;
    open: boolean;
    onApply: (filters: Filter[], matchAny: boolean) => void;
    onPage: (offset: number) => void;
    onLimit: (limit: number) => void;
    onRefresh: () => void;
    onCount: () => void;
    onCopySql: () => void;
    onToggle: () => void;
  }

  let {
    title,
    columns,
    filters,
    matchAny,
    offset,
    limit,
    rowsOnPage,
    count,
    busy,
    open,
    onApply,
    onPage,
    onLimit,
    onRefresh,
    onCount,
    onCopySql,
    onToggle,
  }: Props = $props();

  const OPS: { op: FilterOp; label: string }[] = [
    { op: "equals", label: "=" },
    { op: "notEquals", label: "≠" },
    { op: "contains", label: "contains" },
    { op: "notContains", label: "does not contain" },
    { op: "startsWith", label: "starts with" },
    { op: "endsWith", label: "ends with" },
    { op: "greater", label: ">" },
    { op: "greaterOrEqual", label: "≥" },
    { op: "less", label: "<" },
    { op: "lessOrEqual", label: "≤" },
    { op: "in", label: "in (a, b, c)" },
    { op: "between", label: "between" },
    { op: "isNull", label: "is NULL" },
    { op: "isNotNull", label: "is not NULL" },
  ];
  /** The column choice that makes a row a SQL condition. */
  const RAW = "\u0000sql";

  // The draft starts as what the page was read with; the bar is made again for each table.
  // svelte-ignore state_referenced_locally
  let draft: Filter[] = $state(filters.length > 0 ? filters.map((filter) => ({ ...filter })) : []);
  // svelte-ignore state_referenced_locally
  let draftAny = $state(matchAny);
  let rows: HTMLDivElement | undefined = $state();

  const numberFormat = new Intl.NumberFormat();
  const known = $derived(typeof count === "number" ? count : null);
  const lastOffset = $derived(known === null ? null : Math.max(0, Math.floor((known - 1) / limit) * limit));
  const hasNext = $derived(
    rowsOnPage !== null && (known !== null ? offset + limit < known : rowsOnPage >= limit),
  );
  const range = $derived.by(() => {
    if (rowsOnPage === null) return "Loading…";
    if (rowsOnPage === 0) return offset === 0 ? "No rows" : `No rows after ${numberFormat.format(offset)}`;
    return `${numberFormat.format(offset + 1)}–${numberFormat.format(offset + rowsOnPage)}`;
  });

  function blank(): Filter {
    return { column: columns[0] ?? "", op: "contains", value: "", second: "" };
  }

  /** Opens the filters and puts the cursor in the first value. */
  export async function focusFilters() {
    if (!open) onToggle();
    if (draft.length === 0) draft.push(blank());
    await tick();
    rows?.querySelector<HTMLInputElement>("input")?.focus();
  }

  async function add() {
    draft.push(blank());
    await tick();
    rows?.querySelectorAll<HTMLInputElement>("input[data-value]")[draft.length - 1]?.focus();
  }

  function apply() {
    const usable = draft.filter(
      (filter) => filter.op === "isNull" || filter.op === "isNotNull" || filter.value.trim() !== "",
    );
    onApply(
      usable.map((filter) => ({ ...filter })),
      draftAny,
    );
  }

  function clearAll() {
    draft = [];
    onApply([], draftAny);
  }

  function pick(filter: Filter, choice: string) {
    if (choice === RAW) {
      filter.op = "raw";
      filter.column = "";
    } else {
      if (filter.op === "raw") filter.op = "contains";
      filter.column = choice;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      apply();
    } else if (event.key === "Escape" && open) {
      event.preventDefault();
      onToggle();
    }
  }
</script>

<div class="shrink-0 border-b border-line bg-surface-2 text-[12px]">
  <div class="flex h-8 items-center gap-1 px-2">
    <button
      class="toolbar-button"
      class:text-accent={filters.length > 0}
      onclick={() => (open ? onToggle() : void focusFilters())}
      title="Filter the table on the server (Ctrl+F)"
    >
      Filter{filters.length > 0 ? ` (${filters.length})` : ""}
    </button>
    <span class="min-w-0 truncate font-semibold">{title}</span>
    <div class="flex-1"></div>
    <button class="pager" onclick={() => onPage(0)} disabled={busy || offset === 0} title="First page">⏮</button>
    <button
      class="pager"
      onclick={() => onPage(Math.max(0, offset - limit))}
      disabled={busy || offset === 0}
      title="Previous page (Alt+PageUp)">◀</button
    >
    <span class="px-1 whitespace-nowrap tabular-nums">
      {range}
      {#if count === "counting"}
        <span class="text-muted">of …</span>
      {:else if known !== null}
        <span class="text-muted">of {numberFormat.format(known)}</span>
      {:else if rowsOnPage !== null}
        <button class="rounded px-1 text-accent hover:bg-surface-3" onclick={onCount} disabled={busy}>Count</button>
      {/if}
    </span>
    <button class="pager" onclick={() => onPage(offset + limit)} disabled={busy || !hasNext} title="Next page (Alt+PageDown)"
      >▶</button
    >
    <button
      class="pager"
      onclick={() => lastOffset !== null && onPage(lastOffset)}
      disabled={busy || lastOffset === null || offset >= lastOffset}
      title="Last page">⏭</button
    >
    <select
      class="bar-select"
      value={limit}
      onchange={(event) => onLimit(Number(event.currentTarget.value))}
      disabled={busy}
      aria-label="Rows per page"
    >
      {#each PAGE_SIZES as size (size)}
        <option value={size}>{numberFormat.format(size)} / page</option>
      {/each}
    </select>
    <button class="toolbar-button" onclick={onRefresh} disabled={busy} title="Read the page again and count again">Refresh</button>
    <button class="toolbar-button" onclick={onCopySql} title="Copy the statement this page ran">Copy SQL</button>
  </div>

  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="flex flex-col gap-1 border-t border-line px-2 py-1.5" bind:this={rows} onkeydown={onKeydown}>
      {#each draft as filter, index (index)}
        <div class="flex items-center gap-1">
          <select
            class="bar-select w-44"
            value={filter.op === "raw" ? RAW : filter.column}
            onchange={(event) => pick(filter, event.currentTarget.value)}
            aria-label="Column"
          >
            {#each columns as column (column)}
              <option value={column}>{column}</option>
            {/each}
            <option value={RAW}>SQL condition…</option>
          </select>
          {#if filter.op !== "raw"}
            <select class="bar-select w-36" bind:value={filter.op} aria-label="Operator">
              {#each OPS as choice (choice.op)}
                <option value={choice.op}>{choice.label}</option>
              {/each}
            </select>
          {/if}
          {#if filter.op !== "isNull" && filter.op !== "isNotNull"}
            <input
              data-value
              class="bar-input min-w-0 flex-1"
              class:font-mono={filter.op === "raw"}
              placeholder={filter.op === "raw" ? "total > 100 and status <> 'void'" : filter.op === "between" ? "from" : "value"}
              spellcheck="false"
              bind:value={filter.value}
            />
          {:else}
            <span class="flex-1"></span>
          {/if}
          {#if filter.op === "between"}
            <input class="bar-input min-w-0 flex-1" placeholder="to" spellcheck="false" bind:value={filter.second} />
          {/if}
          <button
            class="pager"
            onclick={() => draft.splice(index, 1)}
            title="Remove this filter"
            aria-label="Remove this filter">✕</button
          >
        </div>
      {/each}
      <div class="flex items-center gap-1">
        <button class="toolbar-button" onclick={add}>+ Add filter</button>
        {#if draft.length > 1}
          <select class="bar-select" bind:value={draftAny} aria-label="How filters combine">
            <option value={false}>Match all</option>
            <option value={true}>Match any</option>
          </select>
        {/if}
        <div class="flex-1"></div>
        <button class="toolbar-button" onclick={clearAll} disabled={busy}>Clear</button>
        <button class="toolbar-button font-semibold text-accent" onclick={apply} disabled={busy} title="Apply (Enter)"
          >Apply</button
        >
      </div>
    </div>
  {/if}
</div>

<style>
  .pager {
    height: 24px;
    min-width: 24px;
    border-radius: 5px;
    color: var(--text);
  }
  .pager:hover:not(:disabled) {
    background: var(--surface-3);
  }
  .pager:disabled {
    color: var(--text-muted);
    opacity: 0.5;
  }
  .bar-select,
  .bar-input {
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
    color: var(--text);
    font-size: 12px;
  }
  .bar-input {
    user-select: text;
    outline: none;
  }
  .bar-input:focus {
    border-color: var(--accent);
  }
</style>
