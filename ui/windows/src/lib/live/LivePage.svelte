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
  import Segmented from "../ui/Segmented.svelte";
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
    /** A Supabase project's URL: the page can listen through Realtime too. */
    realtimeUrl?: string | null;
    detailOpen: boolean;
    /** Starts the listening, as the page bar's Listen does. */
    onListen: () => void;
    /** Copies a selection as the results do. */
    onCopy: (result: number, selection: GridSelection, withHeaders: boolean) => void;
    onNote: (text: string) => void;
  }

  let { session, engine, database, realtimeUrl = null, detailOpen, onListen, onCopy, onNote }: Props = $props();

  const SOURCES = [
    { value: "listen" as const, label: "LISTEN" },
    { value: "realtime" as const, label: "Realtime" },
  ];
  const realtime = $derived(realtimeUrl !== null && session.source === "realtime");

  // An event reaches only listeners in the database it was sent in. Once the
  // toolbar moves to another one, what a query page sends no longer comes here.
  const elsewhere = $derived(
    !realtime && engine.hasDatabases && session.busy && session.database !== null && database !== null && session.database !== database,
  );

  const numberFormat = new Intl.NumberFormat();
  const takesPatterns = $derived(engine.liveTopics.includes("pattern"));
  const takesChannels = $derived(engine.liveTopics.includes("channel"));
  /** RabbitMQ: exchanges to watch, and queues to take from (docs/decisions/0026-rabbitmq.md). */
  const takesBindings = $derived(engine.liveTopics.includes("binding"));
  /** Firebase: the Realtime Database's paths. */
  const takesPaths = $derived(engine.liveTopics.includes("path"));

  /** How to send a first event, in the engine's own words. */
  const HINTS: Record<string, string> = {
    postgres: "Type the channels and press Listen. Then send one from a query tab: NOTIFY orders, 'hello'",
    redis: "Type the channels or patterns and press Listen. Then send one from a query tab: PUBLISH news hello",
    rabbitmq: "Type an exchange to watch (amq.topic:orders.#) and press Listen. Then send one from a query tab: publish amq.topic orders.new hello",
    firebase: "Type the Realtime Database's paths (/chats/room1) and press Listen. Then change one from the Firebase console or your app.",
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
    {#if realtimeUrl}
      <Segmented label="Listen through" options={SOURCES} value={session.source} onChange={(next) => (session.source = next)} />
    {/if}
    {#if realtime}
      <label class="pb-live-topic">
        <span>Tables</span>
        <Input
          size="sm"
          mono
          placeholder="public.orders, audit.*"
          disabled={session.busy}
          tip="schema.table, schema.* or *: their inserts, updates and deletes. Enter listens"
          tipAt="start"
          bind:value={session.tables}
        />
      </label>
      <label class="pb-live-topic">
        <span>Broadcast</span>
        <Input size="sm" mono placeholder="room1" disabled={session.busy} tip="Broadcast channels, by name. Enter listens" bind:value={session.broadcasts} />
      </label>
      <label class="pb-live-topic">
        <span>API key</span>
        <Input
          size="sm"
          type="password"
          placeholder="Kept once it works"
          disabled={session.busy}
          tip={`The anon or service_role key of ${realtimeUrl}. Kept in Windows Credential Manager`}
          bind:value={session.apiKey}
        />
      </label>
    {:else if takesBindings}
      <label class="pb-live-topic">
        <span>Exchanges</span>
        <Input
          size="sm"
          mono
          placeholder="amq.topic:orders.#, events"
          disabled={session.busy}
          tip="exchange or exchange:key. A queue of this page's own gets a copy of what they route; it goes when the page stops. Enter listens"
          tipAt="start"
          bind:value={session.bindings}
        />
      </label>
      <label class="pb-live-topic">
        <span>Take from queues</span>
        <Input
          size="sm"
          mono
          placeholder="jobs"
          disabled={session.busy}
          tip="Consumes them: each message heard is acknowledged and the queue no longer holds it. Enter listens"
          bind:value={session.queues}
        />
      </label>
    {:else if takesPaths}
      <label class="pb-live-topic">
        <span>Paths</span>
        <Input
          size="sm"
          mono
          placeholder="/chats/room1, /presence"
          disabled={session.busy}
          tip="Realtime Database paths: every change at or below each. Enter listens"
          tipAt="start"
          bind:value={session.paths}
        />
      </label>
    {:else if takesChannels}
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
        text={realtime
          ? "Type the tables (public.orders) or broadcast channels, and the project's API key, then press Listen. The table must be in the supabase_realtime publication."
          : (HINTS[engine.id] ?? "Type what to listen to and press Listen.")}
        keys={[
          { keys: ["F5"], label: "Listen" },
          { keys: ["Shift", "F5"], label: "Stop" },
        ]}
      />
    {/if}
  </div>
</section>
