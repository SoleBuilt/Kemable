<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { untrack } from "svelte";
  import SqlEditor, { type RunRequest } from "./lib/editor/SqlEditor.svelte";
  import { errorTitle } from "./lib/errors";
  import DataGrid from "./lib/grid/DataGrid.svelte";
  import type { GridCallbacks, GridSource } from "./lib/grid/engine";
  import { asCoreError, ipc } from "./lib/ipc";
  import ObjectTree from "./lib/sidebar/ObjectTree.svelte";
  import DatabasePicker from "./lib/connect/DatabasePicker.svelte";
  import PostgresDialog from "./lib/connect/PostgresDialog.svelte";
  import type { ColumnMeta, ConnectionConfig, CoreError, ObjectInfo, OpenedConnection, RunEvent } from "./lib/types";

  interface ResultSetView {
    result: number;
    statement: number;
    columns: ColumnMeta[];
    totalRows: number;
    finished: boolean;
    limited: boolean;
  }

  type Message =
    | { kind: "command"; statement: number; rowsAffected: number | null }
    | { kind: "notice"; statement: number; text: string }
    | { kind: "switched"; statement: number; database: string }
    | { kind: "error"; statement: number | null; error: CoreError };

  /**
   * After `CREATE DATABASE x` succeeds, the rest of the script runs in x
   * (decided 2026-11-09, docs/roadmap.md). Other clients stay put, so the
   * switch is announced in Messages. A settings screen will let it be turned off.
   */
  const FOLLOW_CREATED_DATABASE = true;

  let connection = $state.raw<OpenedConnection | null>(null);
  let connectError: CoreError | null = $state(null);
  let connecting = $state(false);

  // Raw: a set is replaced, never mutated, so the grid gets plain objects.
  let sets: ResultSetView[] = $state.raw([]);
  let messages: Message[] = $state([]);
  let activeTab: number | "messages" = $state(0);
  let runId: number | null = $state(null);
  let running = $state(false);
  let elapsedMs: number | null = $state(null);
  let refreshKey = $state(0);
  let statements: string[] = [];
  /** The database the editor runs in, for an engine with databases. */
  let database: string | null = $state(null);
  /** How many statements the last run sent, of how many the text holds. */
  let ranCount: { ran: number; total: number } | null = $state(null);
  let tree: ObjectTree | undefined = $state();
  let picker: DatabasePicker | undefined = $state();

  let editorShare = $state(0.42);
  let sidebarWidth = $state(248);
  let editor: SqlEditor | undefined = $state();

  const grammar = $derived(connection?.engine.sqlGrammar ?? null);
  const activeSet = $derived(typeof activeTab === "number" ? sets[activeTab] : undefined);
  const activeResult = $derived(activeSet?.result ?? null);
  // Only a change of set gives the grid a new source; row counts travel separately.
  const gridSource: GridSource | null = $derived.by(() => {
    const result = activeResult;
    if (result === null) return null;
    return untrack(() => {
      const set = sets.find((candidate) => candidate.result === result);
      return set ? { result, columns: set.columns } : null;
    });
  });
  const gridRows = $derived(activeSet?.totalRows ?? 0);
  const errorCount = $derived(messages.filter((message) => message.kind === "error").length);

  const gridCallbacks: GridCallbacks = {
    fetchWindow: ipc.fetchWindow,
    copyCell: (result, row, column) => {
      void ipc.cellText(result, row, column).then((text) => navigator.clipboard.writeText(text ?? "NULL"));
    },
  };

  /** Opens a connection in place of the current one. Resolves to the error, or null. */
  async function connectWith(config: ConnectionConfig): Promise<CoreError | null> {
    connecting = true;
    try {
      const opened = await ipc.connect(config);
      if (connection) void ipc.disconnect(connection.id);
      clearResults();
      connection = opened;
      database = opened.database;
      editor?.focus();
      return null;
    } catch (caught) {
      return asCoreError(caught);
    } finally {
      connecting = false;
    }
  }

  async function connectTo(path: string) {
    connectError = await connectWith({ engine: "sqlite", target: { kind: "file", path, readOnly: false } });
  }

  let postgresDialog = $state(false);

  async function connectPostgres(config: ConnectionConfig): Promise<CoreError | null> {
    const error = await connectWith(config);
    if (!error) postgresDialog = false;
    return error;
  }

  async function openFile() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "SQLite", extensions: ["db", "sqlite", "sqlite3", "db3", "s3db"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof path === "string") await connectTo(path);
  }

  function clearResults() {
    for (const set of sets) void ipc.releaseResult(set.result);
    sets = [];
    messages = [];
    activeTab = 0;
    elapsedMs = null;
  }

  /** Runs `request`. One from the editor is marked there; one the app wrote (opening a table) is not. */
  async function startRun(request: RunRequest, fromEditor = true) {
    if (!connection || running) return;
    clearResults();
    statements = [];
    ranCount = null;
    running = true;
    try {
      const started = await ipc.runQuery(
        connection.id,
        database,
        request.sql,
        request.cursor,
        FOLLOW_CREATED_DATABASE,
        onEvent,
      );
      runId = started.run;
      ranCount = { ran: started.ranges.length, total: started.scriptStatements };
      if (fromEditor) editor?.showRan(request, started.ranges);
    } catch (caught) {
      running = false;
      messages = [{ kind: "error", statement: null, error: asCoreError(caught) }];
      activeTab = "messages";
    }
  }

  function updateSet(result: number, change: Partial<ResultSetView>) {
    sets = sets.map((set) => (set.result === result ? { ...set, ...change } : set));
  }

  function onEvent(event: RunEvent) {
    switch (event.type) {
      case "statementStarted":
        statements[event.index] = event.text;
        break;
      case "setStarted":
        sets = [
          ...sets,
          {
            result: event.result,
            statement: event.statement,
            columns: event.columns,
            totalRows: 0,
            finished: false,
            limited: false,
          },
        ];
        break;
      case "rows":
        updateSet(event.result, { totalRows: event.total });
        break;
      case "setFinished":
        updateSet(event.result, { totalRows: event.total, finished: true, limited: event.limited });
        break;
      case "commandDone":
        messages.push({ kind: "command", statement: event.statement, rowsAffected: event.rowsAffected });
        break;
      case "notice":
        messages.push({ kind: "notice", statement: event.statement, text: event.text });
        break;
      case "databaseChanged":
        database = event.database;
        messages.push({ kind: "switched", statement: event.statement, database: event.database });
        break;
      case "statementFailed":
        messages.push({ kind: "error", statement: event.statement, error: event.error });
        break;
      case "finished":
        running = false;
        runId = null;
        elapsedMs = event.elapsedMs;
        if (sets.length === 0 || event.failed) activeTab = "messages";
        if (messages.some((message) => message.kind === "command" || message.kind === "switched")) refreshKey++;
        break;
    }
  }

  function stopRun() {
    if (runId !== null) void ipc.cancelRun(runId);
  }

  function quote(name: string): string {
    return grammar === "sqlServer" ? `[${name.replaceAll("]", "]]")}]` : `"${name.replaceAll('"', '""')}"`;
  }

  /** Opens a table or view from the tree, in the database it belongs to. */
  function openObject(object: ObjectInfo, inDatabase: string | null) {
    if (object.kind !== "table" && object.kind !== "view" && object.kind !== "materializedView") return;
    if (running) return;
    if (inDatabase !== null) database = inDatabase;
    const name = object.schema ? `${quote(object.schema)}.${quote(object.name)}` : quote(object.name);
    void startRun({ sql: `select * from ${name}`, cursor: null, origin: 0 }, false);
  }

  function useDatabase(name: string) {
    if (running) return;
    database = name;
    editor?.focus();
  }

  // App-wide keys. WebView2 reloads the page on F5 and Ctrl+R, which would
  // drop every open connection, so those never reach it.
  function onWindowKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (ctrl && key === "k" && connection?.engine.hasDatabases) {
      event.preventDefault();
      void picker?.open();
    } else if (ctrl && key === "r") {
      event.preventDefault();
      if (connection) void tree?.refresh();
    } else if (event.key === "F5") {
      event.preventDefault();
      if (connection && !running) runFromToolbar("all");
    }
  }

  function runFromToolbar(mode: "statement" | "all") {
    const request = editor?.request(mode);
    if (request) void startRun(request);
  }

  function statementLabel(index: number | null): string {
    if (index === null) return "";
    const text = statements[index] ?? "";
    return text.length > 80 ? `${text.slice(0, 80)}…` : text;
  }

  function dragSplit(event: PointerEvent, axis: "x" | "y") {
    const target = event.currentTarget as HTMLElement;
    const container = target.parentElement;
    if (!container) return;
    target.setPointerCapture(event.pointerId);
    const rect = container.getBoundingClientRect();
    const move = (moveEvent: PointerEvent) => {
      if (axis === "y") editorShare = Math.min(0.85, Math.max(0.12, (moveEvent.clientY - rect.top) / rect.height));
      else sidebarWidth = Math.min(480, Math.max(160, moveEvent.clientX - rect.left));
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  const numberFormat = new Intl.NumberFormat();
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="flex h-full flex-col">
  <!-- Toolbar -->
  <header class="flex h-10 shrink-0 items-center gap-1 border-b border-line bg-surface-2 px-2">
    <button class="toolbar-button" onclick={openFile} disabled={connecting}>Open SQLite…</button>
    <button class="toolbar-button" onclick={() => connectTo(":memory:")} disabled={connecting}>In memory</button>
    <button class="toolbar-button" onclick={() => (postgresDialog = true)} disabled={connecting}>PostgreSQL…</button>
    {#if connection}
      <div class="ml-2 flex min-w-0 items-center gap-2 text-[12px]">
        <span class="size-2 shrink-0 rounded-full bg-success"></span>
        <span class="truncate font-semibold">{connection.title}</span>
        <span class="shrink-0 text-muted">{connection.engine.displayName} {connection.serverVersion ?? ""}</span>
      </div>
      {#if connection.engine.hasDatabases}
        <DatabasePicker bind:this={picker} conn={connection.id} {database} onPick={useDatabase} />
      {/if}
    {/if}
    <div class="flex-1"></div>
    <button
      class="toolbar-button"
      onclick={() => runFromToolbar("statement")}
      disabled={!connection || running}
      title="Run only the statement under the cursor, or the selection (Ctrl+Enter)">▶ Run statement</button
    >
    <button
      class="toolbar-button"
      onclick={() => runFromToolbar("all")}
      disabled={!connection || running}
      title="Run every statement in the editor (Ctrl+Shift+Enter or F5)">▶▶ Run all</button
    >
    <button class="toolbar-button" onclick={stopRun} disabled={!running} title="Stop the running statement">■ Stop</button>
  </header>

  {#if connectError}
    <div class="flex items-center gap-3 border-b border-line bg-surface px-3 py-2 text-[12px]">
      <span class="font-semibold text-danger">{errorTitle(connectError)}</span>
      <span class="min-w-0 flex-1 truncate text-muted">{connectError.message}</span>
      <button class="toolbar-button" onclick={() => (connectError = null)}>Dismiss</button>
    </div>
  {/if}

  <div class="flex min-h-0 flex-1">
    <!-- Sidebar -->
    <aside class="shrink-0 border-r border-line bg-surface-2" style:width="{sidebarWidth}px">
      {#if connection}
        <ObjectTree
          bind:this={tree}
          {connection}
          {database}
          {refreshKey}
          onOpen={openObject}
          onUseDatabase={useDatabase}
        />
      {:else}
        <div class="p-4 text-[12.5px] leading-relaxed text-muted">
          Open a SQLite file or connect to PostgreSQL (Neon and Supabase included) to start. Cloudflare D1's local
          databases are SQLite files under <code class="text-fg">.wrangler/state/v3/d1</code>.
        </div>
      {/if}
    </aside>
    <div
      class="w-1 shrink-0 cursor-col-resize hover:bg-accent/40"
      role="separator"
      aria-orientation="vertical"
      onpointerdown={(event) => dragSplit(event, "x")}
    ></div>

    <!-- Editor and results -->
    <main class="flex min-w-0 flex-1 flex-col">
      <section class="min-h-0 border-b border-line" style:height="{editorShare * 100}%">
        <SqlEditor
          bind:this={editor}
          {grammar}
          initial={"select 1 as one, 'hello' as greeting;\n"}
          onRun={(request) => void startRun(request)}
        />
      </section>
      <div
        class="h-1 shrink-0 cursor-row-resize hover:bg-accent/40"
        role="separator"
        aria-orientation="horizontal"
        onpointerdown={(event) => dragSplit(event, "y")}
      ></div>

      <section class="flex min-h-0 flex-1 flex-col">
        <nav class="flex h-8 shrink-0 items-end gap-0.5 border-b border-line bg-surface-2 px-2 text-[12px]">
          {#each sets as set, index (set.result)}
            <button class="result-tab" class:active={activeTab === index} onclick={() => (activeTab = index)}>
              Result {index + 1}
              <span class="text-muted">{numberFormat.format(set.totalRows)}{set.limited ? "+" : ""}</span>
            </button>
          {/each}
          <button class="result-tab" class:active={activeTab === "messages"} onclick={() => (activeTab = "messages")}>
            Messages
            {#if errorCount > 0}<span class="text-danger">{errorCount}</span>{/if}
          </button>
        </nav>

        <div class="min-h-0 flex-1">
          {#if activeTab === "messages"}
            <div class="h-full overflow-auto p-3 font-mono text-[12px] select-text">
              {#each messages as message, index (index)}
                <div class="mb-2">
                  {#if message.kind === "error"}
                    <div class="text-danger">{errorTitle(message.error)}: {message.error.message}</div>
                  {:else if message.kind === "command"}
                    <div>{message.rowsAffected === null ? "Done" : `${numberFormat.format(message.rowsAffected)} rows affected`}</div>
                  {:else if message.kind === "switched"}
                    <div class="text-accent">Switched to {message.database}; the statements after this run there.</div>
                  {:else}
                    <div class="text-muted">{message.text}</div>
                  {/if}
                  {#if message.statement !== null}
                    <div class="truncate text-muted">{statementLabel(message.statement)}</div>
                  {/if}
                </div>
              {:else}
                <div class="text-muted">No messages.</div>
              {/each}
            </div>
          {:else}
            <DataGrid source={gridSource} totalRows={gridRows} callbacks={gridCallbacks} />
          {/if}
        </div>
      </section>
    </main>
  </div>

  <!-- Status bar -->
  <footer class="flex h-6 shrink-0 items-center gap-4 border-t border-line bg-surface-2 px-3 text-[11.5px] text-muted">
    {#if running}
      <span class="text-accent">Running…</span>
    {:else if elapsedMs !== null}
      <span>{numberFormat.format(elapsedMs)} ms</span>
    {/if}
    {#if ranCount}
      <span class:text-fg={ranCount.ran < ranCount.total}>
        {ranCount.ran < ranCount.total
          ? `Ran ${ranCount.ran} of ${ranCount.total} statements`
          : `Ran ${ranCount.ran} ${ranCount.ran === 1 ? "statement" : "statements"}`}
      </span>
    {/if}
    {#if connection?.engine.hasDatabases && database}
      <span>in {database}</span>
    {/if}
    {#if activeSet}
      <span>
        {numberFormat.format(activeSet.totalRows)} rows{activeSet.finished ? "" : " so far"}
        {#if activeSet.limited}<span class="text-danger"> · stopped at the row limit</span>{/if}
      </span>
    {/if}
    <div class="flex-1"></div>
    <span>Paboable</span>
  </footer>
</div>

{#if postgresDialog}
  <PostgresDialog onConnect={connectPostgres} onClose={() => (postgresDialog = false)} />
{/if}

<style>
  :global(.toolbar-button) {
    height: 26px;
    padding: 0 10px;
    border-radius: 5px;
    font-size: 12px;
    color: var(--text);
  }
  :global(.toolbar-button:hover:not(:disabled)) {
    background: var(--surface-3);
  }
  :global(.toolbar-button:disabled) {
    color: var(--text-muted);
    opacity: 0.6;
  }
  .result-tab {
    display: flex;
    gap: 6px;
    align-items: center;
    height: 26px;
    padding: 0 10px;
    border-radius: 5px 5px 0 0;
    color: var(--text-muted);
  }
  .result-tab:hover {
    color: var(--text);
  }
  .result-tab.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: inset 0 2px 0 var(--accent);
  }
</style>
