<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { untrack } from "svelte";
  import SqlEditor, { type RunRequest } from "./lib/editor/SqlEditor.svelte";
  import { errorTitle } from "./lib/errors";
  import DataGrid from "./lib/grid/DataGrid.svelte";
  import type { CellRect, GridCallbacks, GridSource, SortKey } from "./lib/grid/engine";
  import RowDetail from "./lib/grid/RowDetail.svelte";
  import TableBar, { PAGE_SIZES } from "./lib/grid/TableBar.svelte";
  import { asCoreError, ipc } from "./lib/ipc";
  import HistoryList from "./lib/sidebar/HistoryList.svelte";
  import KeyBrowser from "./lib/sidebar/KeyBrowser.svelte";
  import ObjectTree from "./lib/sidebar/ObjectTree.svelte";
  import QueryList from "./lib/sidebar/QueryList.svelte";
  import SaveQueryDialog from "./lib/sidebar/SaveQueryDialog.svelte";
  import DatabasePicker from "./lib/connect/DatabasePicker.svelte";
  import ConnectDialog, { type ConnectRequest, type NetworkEngine, isNetworkEngine } from "./lib/connect/ConnectDialog.svelte";
  import SavedList from "./lib/connect/SavedList.svelte";
  import type {
    ColumnMeta,
    ConnectionConfig,
    ColumnInfo,
    CoreError,
    Filter,
    HistoryEntry,
    KeyDetail,
    KeyInfo,
    ObjectInfo,
    OpenedConnection,
    RunEvent,
    SavedConnection,
    SavedQuery,
    TableRef,
  } from "./lib/types";

  /**
   * A table opened from the tree: one page at a time, filtered and sorted on
   * the server (docs/decisions/0006-table-browsing.md).
   */
  interface TableTab {
    database: string | null;
    table: TableRef;
    title: string;
    /** Column names, in table order; filled from the first page when the catalog gave none. */
    columns: string[];
    /** The primary key, which breaks ties so a row is on one page only. */
    key: string[];
    filters: Filter[];
    matchAny: boolean;
    sort: { column: string; descending: boolean }[];
    offset: number;
    limit: number;
    count: number | "counting" | null;
    pageSql: string;
    countSql: string;
  }

  const PAGE_SIZE_KEY = "paboable.pageSize";

  function readPageSize(): number {
    try {
      const value = Number(localStorage.getItem(PAGE_SIZE_KEY));
      return PAGE_SIZES.includes(value) ? value : 1_000;
    } catch {
      return 1_000;
    }
  }

  interface ResultSetView {
    result: number;
    statement: number;
    columns: ColumnMeta[];
    totalRows: number;
    finished: boolean;
    limited: boolean;
    /** The sort the header shows. */
    sort: SortKey[];
    /** Who sorted it: the server (the statement ran again) or the core. */
    sortedBy: "server" | "core" | null;
    /** For a set the server sorted: the statement it was written from, run again for "unsorted". */
    original: string | null;
  }

  /** The row caps on offer; null keeps every row. */
  const ROW_CAPS: (number | null)[] = [1_000, 10_000, 100_000, 500_000, 1_000_000, null];
  const ROW_CAP_KEY = "paboable.rowCap";

  function readRowCap(): number | null {
    try {
      const saved = localStorage.getItem(ROW_CAP_KEY);
      if (saved === "none") return null;
      const value = Number(saved);
      return ROW_CAPS.includes(value) ? value : 500_000;
    } catch {
      return 500_000;
    }
  }

  type Message =
    | { kind: "command"; statement: number; rowsAffected: number | null }
    | { kind: "notice"; statement: number | null; text: string }
    | { kind: "switched"; statement: number; database: string }
    | { kind: "error"; statement: number | null; error: CoreError };

  /**
   * After `CREATE DATABASE x` succeeds, the rest of the script runs in x
   * (decided 2026-11-09, docs/roadmap.md). Other clients stay put, so the
   * switch is announced in Messages. A settings screen will let it be turned off.
   */
  const FOLLOW_CREATED_DATABASE = true;
  /** One empty sort, so the grid's prop does not change on every row count update. */
  const NO_SORT: SortKey[] = [];

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

  /** What the sidebar shows while connected. */
  let sidebarTab: "objects" | "queries" | "history" = $state("objects");
  /** Bumped after a run that went to the history, and after a save. */
  let historyKey = $state(0);
  let queriesKey = $state(0);
  /** Whether the run in progress goes to the history. */
  let recording = false;
  /** The save dialog: a new query or the one being edited. */
  let saving: SavedQuery | null = $state(null);

  let tableTab = $state.raw<TableTab | null>(null);
  /** A Redis key open in the results, and what reading it found. */
  let keyView = $state.raw<{ name: string; detail: KeyDetail } | null>(null);
  let keyBrowser: KeyBrowser | undefined = $state();
  const keyValue = $derived(connection?.engine.dataModel === "keyValue");
  let filtersOpen = $state(false);
  let tableBar: TableBar | undefined = $state();
  let pageSize = $state(readPageSize());
  /** The run going now reads a page of `tableTab`. */
  let pageRun = false;
  /** Count the table's rows once the page run ends. */
  let countAfterPage = false;
  /** Bumped when the filters change, so a count for the old ones is dropped. */
  let countGeneration = 0;
  const rowsOnPage = $derived(tableTab && !running ? (sets[0]?.totalRows ?? 0) : null);

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

  /** The grid's cell cursor and selection, for the detail pane and the status bar. */
  let cursor: { result: number; row: number; column: number } | null = $state(null);
  let selected: CellRect | null = $state(null);
  let detailOpen = $state(false);
  let grid: DataGrid | undefined = $state();
  /** A short note in the status bar, such as a copy's outcome. */
  let statusNote: string | null = $state(null);
  let noteTimer: ReturnType<typeof setTimeout> | undefined;

  function note(text: string) {
    statusNote = text;
    clearTimeout(noteTimer);
    noteTimer = setTimeout(() => (statusNote = null), 2500);
  }

  const gridCallbacks: GridCallbacks = {
    fetchWindow: ipc.fetchWindow,
    copy: (result, rect, withHeaders) => {
      ipc
        .copyRange(result, rect.firstRow, rect.rowCount, rect.firstColumn, rect.columnCount, withHeaders)
        .then((text) => navigator.clipboard.writeText(text))
        .then(
          () => note(`Copied ${numberFormat.format(rect.rowCount * rect.columnCount)} cells${withHeaders ? " with column names" : ""}`),
          (caught: unknown) => note(`Not copied: ${asCoreError(caught).message}`),
        );
    },
    selectionChanged: (result, focus, rect) => {
      cursor = focus ? { result, row: focus.row, column: focus.column } : null;
      selected = rect;
    },
    sortRequested: (column, additive) => void sortBy(column, additive),
  };

  /** Opens a connection in place of the current one. Resolves to the error, or null. */
  async function openWith(opening: () => Promise<OpenedConnection>): Promise<CoreError | null> {
    connecting = true;
    try {
      const opened = await opening();
      if (connection) void ipc.disconnect(connection.id);
      clearResults();
      tableTab = null;
      keyView = null;
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
    const config: ConnectionConfig = { engine: "sqlite", target: { kind: "file", path, readOnly: false } };
    connectError = await openWith(() => ipc.connect(config));
  }

  function disconnect() {
    if (!connection || running) return;
    void ipc.disconnect(connection.id);
    clearResults();
    tableTab = null;
    keyView = null;
    connection = null;
    database = null;
  }

  /** The connect dialog: the engine it is for, and the saved connection it edits. */
  let connectDialog: { engine: NetworkEngine; saved: SavedConnection | null } | null = $state(null);
  let savedKey = $state(0);

  async function connectNetwork(request: ConnectRequest): Promise<CoreError | null> {
    const error = await openWith(async () => {
      // A saved connection opens by id: a password left blank is the kept one.
      const opened = request.savedId
        ? await ipc.connectSaved(request.savedId, request.password)
        : await ipc.connect(request.config);
      return { ...opened, title: request.title };
    });
    if (!error) connectDialog = null;
    return error;
  }

  /** A saved connection whose password is not kept asks for it first. */
  let askPassword: SavedConnection | null = $state(null);

  function needsPassword(saved: SavedConnection): boolean {
    const target = saved.config.target;
    return target.kind === "network" && !saved.savePassword && !(saved.config.engine === "mssql" && !target.user);
  }

  async function openSaved(saved: SavedConnection, password: string | null = null) {
    if (password === null && needsPassword(saved)) {
      askPassword = saved;
      return;
    }
    askPassword = null;
    connectError = await openWith(() => ipc.connectSaved(saved.id, password));
    savedKey++;
  }

  function editSaved(saved: SavedConnection) {
    const engine: NetworkEngine = isNetworkEngine(saved.config.engine) ? saved.config.engine : "postgres";
    connectDialog = { engine, saved };
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

  /** The row cap each result set stops at; remembered across launches. */
  let rowCap: number | null = $state(readRowCap());
  $effect(() => {
    const value = rowCap === null ? "none" : String(rowCap);
    try {
      localStorage.setItem(ROW_CAP_KEY, value);
    } catch {
      // Without storage the cap still applies; it is just not remembered.
    }
  });

  /** The last run, for Fetch All. */
  let lastRun: { request: RunRequest; database: string | null } | null = null;
  /** A sort that runs the statement again: its keys go to the set that run starts. */
  let pendingSort: { keys: SortKey[]; original: string | null } | null = null;

  /**
   * Runs `request`. One from the editor is marked there; one the app wrote
   * (opening a table, a sort) is not. `inDatabase` and `rowLimit` override the
   * editor's database and the row cap, for a sort that must match its original.
   */
  async function startRun(
    request: RunRequest,
    fromEditor = true,
    overrides: { inDatabase?: string | null; rowLimit?: number | null; record?: boolean; tablePage?: boolean } = {},
  ) {
    if (!connection || running) return;
    pageRun = overrides.tablePage === true;
    if (!pageRun) {
      tableTab = null;
      filtersOpen = false;
    }
    keyView = null;
    clearResults();
    statements = [];
    ranCount = null;
    running = true;
    const runDatabase = overrides.inDatabase !== undefined ? overrides.inDatabase : database;
    const rowLimit = request.unlimited ? null : overrides.rowLimit !== undefined ? overrides.rowLimit : rowCap;
    lastRun = { request, database: runDatabase };
    // What a person ran goes to the history; what the app wrote for them does not.
    const record = overrides.record ?? fromEditor;
    recording = record;
    try {
      const started = await ipc.runQuery(
        connection.id,
        runDatabase,
        request.sql,
        request.cursor,
        FOLLOW_CREATED_DATABASE,
        rowLimit,
        record,
        onEvent,
      );
      runId = started.run;
      ranCount = { ran: started.ranges.length, total: started.scriptStatements };
      if (fromEditor) editor?.showRan(request, started.ranges);
    } catch (caught) {
      running = false;
      pendingSort = null;
      if (record) historyKey++;
      messages = [{ kind: "error", statement: null, error: asCoreError(caught) }];
      activeTab = "messages";
    }
  }

  /** Runs the last run again without a row cap, keeping a server sort's arrows. */
  function fetchAll() {
    if (keyView) {
      void openKey(keyView.name, true);
      return;
    }
    if (!lastRun || running) return;
    const set = activeSet;
    if (set?.sortedBy === "server" && set.original) pendingSort = { keys: set.sort, original: set.original };
    void startRun({ ...lastRun.request, unlimited: true }, false, { inDatabase: lastRun.database });
  }

  /**
   * A header click (docs/decisions/0005-sorting-a-result.md): ascending, then
   * descending, then unsorted; with Shift the column becomes one more key.
   */
  async function sortBy(column: number, additive: boolean) {
    const set = activeSet;
    if (!set || running || !connection) return;
    const current = set.sort.find((key) => key.column === column);
    const next: SortKey | null = !current
      ? { column, descending: false }
      : !current.descending
        ? { column, descending: true }
        : null;
    let keys: SortKey[];
    if (!additive) {
      keys = next ? [next] : [];
    } else if (current) {
      keys = next
        ? set.sort.map((key) => (key.column === column ? next : key))
        : set.sort.filter((key) => key.column !== column);
    } else {
      keys = [...set.sort, { column, descending: false }];
    }

    // A table tab sorts the whole table on the server, from its first page.
    if (tableTab) {
      const sort = keys.flatMap((key) => {
        const name = set.columns[key.column]?.name;
        return name === undefined ? [] : [{ column: name, descending: key.descending }];
      });
      void loadPage({ ...tableTab, sort, offset: 0 }, false);
      return;
    }

    // Unsorting what the server sorted runs the statement it came from.
    if (keys.length === 0 && set.sortedBy === "server" && set.original) {
      void startRun({ sql: set.original, cursor: null, origin: 0 }, false, { inDatabase: lastRun?.database });
      return;
    }
    try {
      const outcome = await ipc.sortResult(set.result, keys);
      if (outcome.kind === "rerun") {
        pendingSort = { keys, original: set.original ?? outcome.statement };
        void startRun({ sql: outcome.sql, cursor: null, origin: 0 }, false, {
          inDatabase: outcome.database,
          rowLimit: outcome.rowLimit,
        });
      } else {
        updateSet(set.result, { sort: keys, sortedBy: keys.length > 0 ? "core" : null });
        grid?.reload();
        if (keys.length > 0 && outcome.limited) note("Sorted only the rows loaded: this result stopped at the row cap");
      }
    } catch (caught) {
      note(`Not sorted: ${asCoreError(caught).message}`);
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
        if (pageRun && tableTab) {
          const tab = tableTab;
          const sort = tab.sort.flatMap((key) => {
            const column = event.columns.findIndex((meta) => meta.name === key.column);
            return column < 0 ? [] : [{ column, descending: key.descending }];
          });
          if (tab.columns.length === 0) tableTab = { ...tab, columns: event.columns.map((meta) => meta.name) };
          pendingSort = sort.length > 0 ? { keys: sort, original: null } : null;
        }
        sets = [
          ...sets,
          {
            result: event.result,
            statement: event.statement,
            columns: event.columns,
            totalRows: 0,
            finished: false,
            limited: false,
            sort: pendingSort?.keys ?? [],
            sortedBy: pendingSort ? "server" : null,
            original: pendingSort?.original ?? null,
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
        pendingSort = null;
        if (recording) historyKey++;
        if (pageRun && tableTab && countAfterPage && !event.failed && !event.cancelled) {
          const rows = sets[0]?.totalRows ?? 0;
          // A first page that is not full holds the whole table: nothing to count.
          if (tableTab.offset === 0 && rows < tableTab.limit) tableTab = { ...tableTab, count: rows };
          else void countTable();
        }
        countAfterPage = false;
        elapsedMs = event.elapsedMs;
        if (sets.length === 0 || event.failed) activeTab = "messages";
        if (messages.some((message) => message.kind === "command" || message.kind === "switched")) refreshKey++;
        break;
    }
  }

  function stopRun() {
    if (runId !== null) void ipc.cancelRun(runId);
  }

  /** Opens a table or view from the tree as a table tab, in the database it belongs to. */
  async function openObject(object: ObjectInfo, inDatabase: string | null) {
    if (object.kind !== "table" && object.kind !== "view" && object.kind !== "materializedView") return;
    if (running || !connection) return;
    if (inDatabase !== null) database = inDatabase;
    const table: TableRef = { schema: object.schema, name: object.name };
    let columns: ColumnInfo[] = [];
    try {
      columns = await ipc.fetchColumns(connection.id, database, table);
    } catch {
      // The page still opens; without the catalog it has no key, and its columns come from the page.
    }
    const key = columns
      .filter((column) => column.primaryKey !== null)
      .sort((a, b) => (a.primaryKey ?? 0) - (b.primaryKey ?? 0))
      .map((column) => column.name);
    filtersOpen = false;
    await loadPage(
      {
        database,
        table,
        title: object.schema ? `${object.schema}.${object.name}` : object.name,
        columns: columns.map((column) => column.name),
        key,
        filters: [],
        matchAny: false,
        sort: [],
        offset: 0,
        limit: pageSize,
        count: null,
        pageSql: "",
        countSql: "",
      },
      true,
    );
  }

  /** Reads one page of a table tab; with `recount`, counts the rows its filters match after. */
  async function loadPage(next: TableTab, recount: boolean) {
    if (!connection || running) return;
    let statements;
    try {
      statements = await ipc.browseStatements(connection.id, {
        table: next.table,
        filters: next.filters,
        matchAny: next.matchAny,
        sort: next.sort,
        key: next.key,
        offset: next.offset,
        limit: next.limit,
      });
    } catch (caught) {
      note(`Not filtered: ${asCoreError(caught).message}`);
      return;
    }
    if (recount) countGeneration++;
    tableTab = { ...next, pageSql: statements.page, countSql: statements.count, count: recount ? null : next.count };
    countAfterPage = recount;
    await startRun({ sql: statements.page, cursor: null, origin: 0 }, false, {
      inDatabase: next.database,
      rowLimit: null,
      tablePage: true,
    });
  }

  async function countTable() {
    const tab = tableTab;
    if (!tab || !connection) return;
    const generation = ++countGeneration;
    tableTab = { ...tab, count: "counting" };
    try {
      const total = await ipc.countRows(connection.id, tab.database, tab.countSql);
      if (generation === countGeneration && tableTab) tableTab = { ...tableTab, count: total };
    } catch (caught) {
      if (generation === countGeneration && tableTab) {
        tableTab = { ...tableTab, count: null };
        note(`Not counted: ${asCoreError(caught).message}`);
      }
    }
  }

  /** Reads a key into the results, up to the row cap unless `unlimited`. */
  async function openKey(name: string, unlimited = false) {
    if (!connection || running) return;
    try {
      const read = await ipc.readKey(connection.id, database, name, unlimited ? null : rowCap);
      clearResults();
      tableTab = null;
      filtersOpen = false;
      keyView = { name, detail: read.detail };
      messages = read.notices.map((text) => ({ kind: "notice", statement: null, text }));
      if (read.result !== null) {
        sets = [
          {
            result: read.result,
            statement: 0,
            columns: read.columns,
            totalRows: read.totalRows,
            finished: true,
            limited: read.limited,
            sort: [],
            sortedBy: null,
            original: null,
          },
        ];
        activeTab = 0;
      } else {
        activeTab = "messages";
      }
    } catch (caught) {
      note(`Not read: ${asCoreError(caught).message}`);
      if (asCoreError(caught).code === "not_found") keyBrowser?.refresh();
    }
  }

  const KEY_UNITS: Record<KeyDetail["kind"], string> = {
    string: "bytes",
    hash: "fields",
    list: "items",
    set: "members",
    sortedSet: "members",
    stream: "entries",
    other: "",
  };

  function expiresIn(ms: number | null): string {
    if (ms === null) return "no expiry";
    const seconds = Math.ceil(ms / 1000);
    return seconds < 120 ? `expires in ${seconds} s` : `expires in ${numberFormat.format(Math.round(seconds / 60))} min`;
  }

  function pageTo(offset: number) {
    if (tableTab && offset >= 0) void loadPage({ ...tableTab, offset }, false);
  }

  function copyPageSql() {
    if (!tableTab) return;
    navigator.clipboard.writeText(tableTab.pageSql).then(
      () => note("Copied the page's SQL"),
      (caught: unknown) => note(`Not copied: ${String(caught)}`),
    );
  }

  /** Runs a statement from the history in the database it ran in, when it ran here. */
  function runEntry(entry: HistoryEntry) {
    if (!connection || running) return;
    const here = entry.connection === connection.title && connection.engine.hasDatabases;
    if (here && entry.database) database = entry.database;
    editor?.insert(entry.statement);
    void startRun({ sql: entry.statement, cursor: null, origin: 0 }, false, { record: true });
  }

  function runSaved(query: SavedQuery) {
    if (!connection || running) return;
    editor?.insert(query.sql);
    void startRun({ sql: query.sql, cursor: null, origin: 0 }, false, { record: true });
  }

  /** Ctrl+Shift+D: keeps the editor's text, or its selection, as a named query. */
  function saveFromEditor() {
    const sql = editor?.text().trim() ?? "";
    if (sql === "") {
      note("Nothing to save: the editor is empty");
      return;
    }
    const firstLine = sql.split("\n")[0] ?? "";
    saving = {
      id: "",
      name: firstLine.length > 60 ? `${firstLine.slice(0, 60)}…` : firstLine,
      sql,
      connectionId: connection?.savedId ?? null,
      updatedAt: 0,
    };
  }

  /** The editor's names, from the catalog of the database it runs in. */
  function completeAt(text: string, cursor: number) {
    const open = connection;
    if (!open?.engine.sqlGrammar) return Promise.resolve(null);
    return ipc.complete(open.id, database, text, cursor).catch(() => null);
  }

  // A run that may have changed the schema, or a refresh of the tree, makes completion read the catalog again.
  $effect(() => {
    void refreshKey;
    const open = untrack(() => connection);
    if (open) void ipc.forgetCatalog(open.id);
  });

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
      if (connection) {
        void ipc.forgetCatalog(connection.id);
        void tree?.refresh();
        keyBrowser?.refresh();
      }
    } else if (ctrl && event.shiftKey && key === "d") {
      event.preventDefault();
      if (!saving) saveFromEditor();
    } else if (ctrl && key === "f" && tableTab) {
      // The editor's own Ctrl+F (search) prevents the default, so it never gets here.
      event.preventDefault();
      void tableBar?.focusFilters();
    } else if (event.altKey && (event.key === "PageDown" || event.key === "PageUp") && tableTab) {
      event.preventDefault();
      if (running) return;
      if (event.key === "PageUp") {
        if (tableTab.offset > 0) pageTo(Math.max(0, tableTab.offset - tableTab.limit));
      } else if (rowsOnPage !== null && rowsOnPage >= tableTab.limit) {
        pageTo(tableTab.offset + tableTab.limit);
      }
    } else if (ctrl && event.altKey && key === "i") {
      event.preventDefault();
      detailOpen = !detailOpen;
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
    <button class="toolbar-button" onclick={() => (connectDialog = { engine: "postgres", saved: null })} disabled={connecting}>PostgreSQL…</button>
    <button class="toolbar-button" onclick={() => (connectDialog = { engine: "mssql", saved: null })} disabled={connecting}>SQL Server…</button>
    <button class="toolbar-button" onclick={() => (connectDialog = { engine: "redis", saved: null })} disabled={connecting}>Redis…</button>
    {#if connection}
      <div class="ml-2 flex min-w-0 items-center gap-2 text-[12px]">
        <span class="size-2 shrink-0 rounded-full bg-success"></span>
        <span class="truncate font-semibold">{connection.title}</span>
        <span class="shrink-0 text-muted">{connection.engine.displayName} {connection.serverVersion ?? ""}</span>
        <button
          class="toolbar-button text-muted"
          onclick={disconnect}
          disabled={running}
          title="Disconnect and show the saved connections">Disconnect</button
        >
      </div>
      {#if connection.engine.hasDatabases}
        <DatabasePicker bind:this={picker} conn={connection.id} {database} onPick={useDatabase} />
      {/if}
    {/if}
    <div class="flex-1"></div>
    <button
      class="toolbar-button"
      onclick={saveFromEditor}
      disabled={saving !== null}
      title="Keep the editor's text, or the selection, as a named query (Ctrl+Shift+D)">Save query</button
    >
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
    <select
      class="toolbar-select"
      bind:value={rowCap}
      title="The most rows a result keeps. Ctrl+Alt+Enter runs the statement with no limit."
      aria-label="Row limit"
    >
      {#each ROW_CAPS as cap (cap ?? "none")}
        <option value={cap}>{cap === null ? "No limit" : `Limit ${numberFormat.format(cap)}`}</option>
      {/each}
    </select>
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
        <div class="flex h-full flex-col">
          <nav class="flex h-8 shrink-0 items-end gap-0.5 border-b border-line px-2 text-[12px]">
            <button class="result-tab" class:active={sidebarTab === "objects"} onclick={() => (sidebarTab = "objects")}
              >{keyValue ? "Keys" : "Objects"}</button
            >
            <button class="result-tab" class:active={sidebarTab === "queries"} onclick={() => (sidebarTab = "queries")}
              >Queries</button
            >
            <button class="result-tab" class:active={sidebarTab === "history"} onclick={() => (sidebarTab = "history")}
              >History</button
            >
          </nav>
          <div class="min-h-0 flex-1">
            <!-- The tree stays mounted so its expanded nodes survive a tab switch. -->
            <div class="h-full" class:hidden={sidebarTab !== "objects"}>
              {#if keyValue}
                <KeyBrowser
                  bind:this={keyBrowser}
                  conn={connection.id}
                  {database}
                  {refreshKey}
                  openKey={keyView?.name ?? null}
                  onOpen={(key: KeyInfo) => void openKey(key.name)}
                />
              {:else}
                <ObjectTree
                  bind:this={tree}
                  {connection}
                  {database}
                  {refreshKey}
                  onOpen={openObject}
                  onUseDatabase={useDatabase}
                />
              {/if}
            </div>
            {#if sidebarTab === "queries"}
              <QueryList
                connectionId={connection.savedId}
                refreshKey={queriesKey}
                onInsert={(sql) => editor?.insert(sql)}
                onRun={runSaved}
                onEdit={(query) => (saving = query)}
              />
            {:else if sidebarTab === "history"}
              <HistoryList
                connection={connection.title}
                refreshKey={historyKey}
                onInsert={(sql) => editor?.insert(sql)}
                onRun={runEntry}
              />
            {/if}
          </div>
        </div>
      {:else}
        <div class="flex h-full flex-col">
          <div class="min-h-0 flex-1">
            <SavedList
              refreshKey={savedKey}
              busy={connecting}
              onOpen={(saved) => void openSaved(saved)}
              onEdit={editSaved}
            />
          </div>
          <p class="border-t border-line p-3 text-[11.5px] leading-relaxed text-muted">
            Cloudflare D1's local databases are SQLite files under <code class="text-fg">.wrangler/state/v3/d1</code>:
            open one with Open SQLite.
          </p>
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
          plain={keyValue}
          initial={"select 1 as one, 'hello' as greeting;\n"}
          onRun={(request) => void startRun(request)}
          complete={completeAt}
        />
      </section>
      <div
        class="h-1 shrink-0 cursor-row-resize hover:bg-accent/40"
        role="separator"
        aria-orientation="horizontal"
        onpointerdown={(event) => dragSplit(event, "y")}
      ></div>

      <section class="flex min-h-0 flex-1 flex-col">
        {#if keyView}
          {@const view = keyView}
          <div class="flex h-8 shrink-0 items-center gap-3 border-b border-line bg-surface-2 px-3 text-[12px]">
            <span class="min-w-0 truncate font-mono font-semibold select-text">{view.name}</span>
            <span class="text-muted">{view.detail.kind === "sortedSet" ? "sorted set" : view.detail.kind}</span>
            {#if KEY_UNITS[view.detail.kind]}
              <span class="text-muted">{numberFormat.format(view.detail.length)} {KEY_UNITS[view.detail.kind]}</span>
            {/if}
            <span class="text-muted">{expiresIn(view.detail.ttlMs)}</span>
            <div class="flex-1"></div>
            <button class="toolbar-button" onclick={() => void openKey(view.name)} disabled={running} title="Read the key again"
              >Reload</button
            >
          </div>
        {/if}
        {#if tableTab}
          {@const tab = tableTab}
          <!-- Made again for each table, so its draft filters start from that table's. -->
          {#key `${tab.database}\u0000${tab.title}`}
            <TableBar
              bind:this={tableBar}
              title={tab.title}
              columns={tab.columns}
              filters={tab.filters}
              matchAny={tab.matchAny}
              offset={tab.offset}
              limit={tab.limit}
              {rowsOnPage}
              count={tab.count}
              busy={running}
              open={filtersOpen}
              onApply={(filters, matchAny) => void loadPage({ ...tab, filters, matchAny, offset: 0 }, true)}
              onPage={pageTo}
              onLimit={(limit) => {
                pageSize = limit;
                try {
                  localStorage.setItem(PAGE_SIZE_KEY, String(limit));
                } catch {
                  // Not remembered, still used.
                }
                void loadPage({ ...tab, limit, offset: Math.floor(tab.offset / limit) * limit }, false);
              }}
              onRefresh={() => void loadPage(tab, true)}
              onCount={() => void countTable()}
              onCopySql={copyPageSql}
              onToggle={() => (filtersOpen = !filtersOpen)}
            />
          {/key}
        {/if}
        <nav class="flex h-8 shrink-0 items-end gap-0.5 border-b border-line bg-surface-2 px-2 text-[12px]">
          {#each sets as set, index (set.result)}
            <button class="result-tab" class:active={activeTab === index} onclick={() => (activeTab = index)}>
              Result {index + 1}
              <span class="text-muted">{numberFormat.format(set.totalRows)}{set.limited ? "+" : ""}</span>
            </button>
          {/each}
          <button
            class="result-tab ml-auto"
            class:active={detailOpen}
            onclick={() => (detailOpen = !detailOpen)}
            title="Show the selected row's fields (Ctrl+Alt+I)">Row detail</button
          >
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
            <div class="flex h-full">
              <div class="min-w-0 flex-1">
                <DataGrid
                  bind:this={grid}
                  source={gridSource}
                  totalRows={gridRows}
                  sortKeys={activeSet?.sort ?? NO_SORT}
                  callbacks={gridCallbacks}
                />
              </div>
              {#if detailOpen && gridSource}
                <aside class="w-80 shrink-0 border-l border-line">
                  <RowDetail
                    result={gridSource.result}
                    columns={gridSource.columns}
                    row={cursor?.result === gridSource.result ? cursor.row : null}
                    column={cursor?.result === gridSource.result ? cursor.column : null}
                    onPickColumn={(column) => {
                      if (cursor) grid?.focusCell(cursor.row, column);
                    }}
                  />
                </aside>
              {/if}
            </div>
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
    {#if selected && selected.rowCount * selected.columnCount > 1}
      <span>{numberFormat.format(selected.rowCount)} × {numberFormat.format(selected.columnCount)} selected</span>
    {/if}
    {#if statusNote}
      <span class="text-fg">{statusNote}</span>
    {/if}
    {#if activeSet}
      <span>
        {numberFormat.format(activeSet.totalRows)} rows{activeSet.finished ? "" : " so far"}
        {#if activeSet.limited}
          <span class="text-danger"> · stopped at the row limit</span>
          <button class="ml-1 rounded px-1.5 text-accent hover:bg-surface-3" onclick={fetchAll} disabled={running}
            >Fetch all</button
          >
        {/if}
        {#if activeSet.sortedBy === "server"}<span> · sorted by the server</span>{/if}
        {#if activeSet.sortedBy === "core"}<span> · sorted in Paboable</span>{/if}
      </span>
    {/if}
    <div class="flex-1"></div>
    <span>Paboable</span>
  </footer>
</div>

{#if connectDialog}
  <ConnectDialog
    engine={connectDialog.engine}
    saved={connectDialog.saved}
    onConnect={connectNetwork}
    onSaved={() => savedKey++}
    onClose={() => (connectDialog = null)}
  />
{/if}

{#if saving}
  <SaveQueryDialog
    query={saving}
    connectionId={connection?.savedId ?? null}
    onSaved={() => {
      queriesKey++;
      note("Query saved");
    }}
    onClose={() => {
      saving = null;
      editor?.focus();
    }}
  />
{/if}

{#if askPassword}
  {@const target = askPassword}
  <div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[18vh]" role="presentation">
    <form
      class="w-90 rounded-lg border border-line bg-surface p-5 shadow-xl"
      onsubmit={(event) => {
        event.preventDefault();
        const field = event.currentTarget.elements.namedItem("password");
        void openSaved(target, field instanceof HTMLInputElement ? field.value : "");
      }}
    >
      <h2 class="mb-1 text-[14px] font-semibold">Password for {target.name}</h2>
      <p class="mb-3 text-[12px] text-muted">This connection does not keep its password.</p>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        name="password"
        type="password"
        autofocus
        class="mb-4 h-7 w-full rounded border border-line bg-surface px-2 text-[12px] outline-none select-text focus:border-accent"
        onkeydown={(event) => {
          if (event.key === "Escape") askPassword = null;
        }}
      />
      <div class="flex justify-end gap-2">
        <button type="button" class="toolbar-button" onclick={() => (askPassword = null)}>Cancel</button>
        <button type="submit" class="toolbar-button font-semibold text-accent">Connect</button>
      </div>
    </form>
  </div>
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
  .toolbar-select {
    height: 26px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
    color: var(--text);
    font-size: 12px;
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
