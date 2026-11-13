<script lang="ts">
  import { tick, untrack } from "svelte";
  import { errorTitle } from "../errors";
  import DataGrid from "../grid/DataGrid.svelte";
  import FilterRow, { type Condition } from "../grid/FilterRow.svelte";
  import RowDetail from "../grid/RowDetail.svelte";
  import type { GridCallbacks, GridSelection, GridSource } from "../grid/engine";
  import { ipc } from "../ipc";
  import type { EngineInfo } from "../types";
  import EmptyState from "../ui/EmptyState.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import type { LiveSession } from "./session.svelte";

  /**
   * A Live page (docs/decisions/0020-live-tab.md): the channels to listen to,
   * then what arrived on them, one event a row, in the grid every result uses.
   */
  interface Props {
    session: LiveSession;
    engine: EngineInfo;
    /** The database the toolbar shows, which a query page runs in. */
    database: string | null;
    detailOpen: boolean;
    /** Starts the listening, as the page bar's Listen does. */
    onListen: () => void;
    /** Copies a selection as the results do. */
    onCopy: (result: number, selection: GridSelection, withHeaders: boolean) => void;
    onNote: (text: string) => void;
  }

  let { session, engine, database, detailOpen, onListen, onCopy, onNote }: Props = $props();

  // An event reaches only listeners in the database it was sent in. Once the
  // toolbar moves to another one, what a query page sends no longer comes here.
  const elsewhere = $derived(engine.hasDatabases && session.busy && session.database !== null && database !== null && session.database !== database);

  const numberFormat = new Intl.NumberFormat();
  const takesPatterns = $derived(engine.liveTopics.includes("pattern"));

  /** How to send a first event, in the engine's own words. */
  const HINTS: Record<string, string> = {
    postgres: "Type the channels and press Listen. Then send one from a query tab: NOTIFY orders, 'hello'",
    redis: "Type the channels or patterns and press Listen. Then send one from a query tab: PUBLISH news hello",
  };

  let grid: DataGrid | undefined = $state();
  let filterRow: FilterRow | undefined = $state();
  let topics: HTMLDivElement | undefined = $state();
  let cursor: { result: number; row: number; column: number } | null = $state(null);

  // Only a new listening gives the grid a new source; the counts travel separately.
  const source: GridSource | null = $derived.by(() => {
    const result = session.result;
    const columns = session.columns;
    return result === null ? null : { result, columns };
  });

  const callbacks: GridCallbacks = {
    fetchWindow: ipc.fetchWindow,
    copy: (result, selection, withHeaders) => onCopy(result, selection, withHeaders),
    selectionChanged: (result, focus) => {
      cursor = focus ? { result, row: focus.row, column: focus.column } : null;
    },
    sortRequested: (column, additive) => {
      void session.sortBy(column, additive).then((error) => {
        if (error) onNote(`Not sorted: ${error.message}`);
      });
    },
  };

  // The core arranged the rows again: the grid reads them again.
  $effect(() => {
    void session.arranged;
    untrack(() => grid?.reload());
  });

  /** Puts the cursor in the first channel box. */
  export async function focusTopics() {
    await tick();
    topics?.querySelector<HTMLInputElement>("input")?.focus();
  }

  /** Opens the filter row, or goes to it. */
  export async function focusFilter() {
    if (session.result === null) return;
    session.filtersOpen = true;
    await tick();
    await filterRow?.focus();
  }

  function applyFilter(conditions: Condition[], matchAny: boolean) {
    void session.applyFilter(conditions, matchAny).then((error) => {
      if (error) onNote(`Not filtered: ${error.message}`);
    });
  }

  function onTopicKey(event: KeyboardEvent) {
    if (event.key === "Enter" && event.target instanceof HTMLInputElement) {
      event.preventDefault();
      onListen();
    }
  }
</script>

<section class="pb-live" aria-label="Live">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="pb-filters" bind:this={topics} onkeydown={onTopicKey}>
    <label class="pb-live-topic">
      <span>Channels</span>
      <Input
        size="sm"
        mono
        placeholder={'orders, jobs, "Mixed Case"'}
        disabled={session.busy}
        tip="Names, split by spaces or commas. Enter listens"
        tipAt="start"
        bind:value={session.channels}
      />
    </label>
    {#if takesPatterns}
      <label class="pb-live-topic">
        <span>Patterns</span>
        <Input
          size="sm"
          mono
          placeholder="user:*, __keyspace@0__:*"
          disabled={session.busy}
          tip="Globs: every channel one matches. Enter listens"
          bind:value={session.patterns}
        />
      </label>
    {/if}
  </div>

  {#if session.error}
    <InfoBar tone="danger" strip title={errorTitle(session.error)} message={session.error.message} onDismiss={() => (session.error = null)} />
  {/if}
  {#if elsewhere}
    <InfoBar
      tone="warning"
      strip
      title="Listening in {session.database}, while queries now run in {database}"
      message="What is sent in {database} does not reach this tab. Stop and Listen to listen there."
    />
  {/if}
  {#if session.limited}
    <InfoBar
      tone="warning"
      strip
      title="Stopped at {numberFormat.format(session.total)} events"
      message="A Live tab keeps that many. Clear, then Listen to go on."
      onDismiss={() => (session.limited = false)}
    />
  {/if}
  {#each session.notices as notice, index (index)}
    <InfoBar strip title={notice} onDismiss={() => (session.notices = session.notices.filter((_, other) => other !== index))} />
  {/each}

  {#if session.filtersOpen && session.result !== null}
    {#key session.result}
      <FilterRow
        bind:this={filterRow}
        columns={session.columns.map((meta) => meta.name)}
        conditions={session.filter?.conditions ?? []}
        matchAny={session.filter?.matchAny ?? false}
        onApply={applyFilter}
      />
    {/key}
  {/if}

  <div class="pb-results">
    {#if source}
      <div class="pb-results-grid">
        <DataGrid
          bind:this={grid}
          {source}
          totalRows={session.shown}
          sortKeys={session.sort}
          {callbacks}
          followEnd={session.follow && session.sort.length === 0}
        />
      </div>
      {#if detailOpen}
        <aside class="pb-detail-pane" aria-label="Row detail">
          <RowDetail
            result={source.result}
            columns={source.columns}
            row={cursor?.result === source.result ? cursor.row : null}
            column={cursor?.result === source.result ? cursor.column : null}
            onPickColumn={(column) => {
              if (cursor) grid?.focusCell(cursor.row, column);
            }}
          />
        </aside>
      {/if}
    {:else}
      <EmptyState
        compact
        icon="radio"
        title={session.status === "starting" ? "Starting to listen…" : "Nothing heard yet"}
        text={HINTS[engine.id] ?? "Type what to listen to and press Listen."}
        keys={[
          { keys: ["F5"], label: "Listen" },
          { keys: ["Shift", "F5"], label: "Stop" },
        ]}
      />
    {/if}
  </div>
</section>
