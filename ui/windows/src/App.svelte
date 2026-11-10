<script lang="ts">
  import type { EditorState } from "@codemirror/state";
  import { open } from "@tauri-apps/plugin-dialog";
  import { tick, untrack } from "svelte";
  import SqlEditor, { type RunRequest } from "./lib/editor/SqlEditor.svelte";
  import { errorTitle } from "./lib/errors";
  import DataGrid from "./lib/grid/DataGrid.svelte";
  import CellEditor, { type Move } from "./lib/grid/CellEditor.svelte";
  import EditPreview from "./lib/grid/EditPreview.svelte";
  import { setCompactRows, type Cell, type CellBox, type CellRect, type EditOverlay, type GridCallbacks, type GridSource, type SortKey } from "./lib/grid/engine";
  import FilterRow, { type Condition } from "./lib/grid/FilterRow.svelte";
  import Pager, { PAGE_SIZES } from "./lib/grid/Pager.svelte";
  import RowDetail from "./lib/grid/RowDetail.svelte";
  import { asCoreError, ipc } from "./lib/ipc";
  import HistoryList from "./lib/sidebar/HistoryList.svelte";
  import KeyBrowser from "./lib/sidebar/KeyBrowser.svelte";
  import ObjectTree, { type TreeTarget } from "./lib/sidebar/ObjectTree.svelte";
  import QueryList from "./lib/sidebar/QueryList.svelte";
  import SaveQueryDialog from "./lib/sidebar/SaveQueryDialog.svelte";
  import TreeMenu from "./lib/sidebar/TreeMenu.svelte";
  import DatabasePicker from "./lib/connect/DatabasePicker.svelte";
  import ConnectDialog, { type ConnectRequest, type NetworkEngine, isNetworkEngine } from "./lib/connect/ConnectDialog.svelte";
  import Combobox from "./lib/ui/Combobox.svelte";
  import CommandPalette, { type Command } from "./lib/ui/CommandPalette.svelte";
  import Icon from "./lib/ui/Icon.svelte";
  import SettingsDialog, { ROW_CAPS, applyTheme, readSettings, writeSettings, type Settings } from "./lib/ui/SettingsDialog.svelte";
  import type {
    ByteRange,
    ColumnMeta,
    ConnectionConfig,
    CellEdit,
    ColumnInfo,
    CoreError,
    EditRequest,
    Filter,
    HistoryEntry,
    KeyDetail,
    KeyInfo,
    NewValue,
    ObjectInfo,
    OpenedConnection,
    RunEvent,
    SavedConnection,
    SavedQuery,
    TableRef,
  } from "./lib/types";

  /**
   * A table opened from the tree: one page at a time, filtered and sorted on
   * the server (docs/decisions/0006-table-browsing.md), on a page of its own
   * (docs/decisions/0013-pages-and-tabs.md).
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

  type Message =
    | { kind: "command"; statement: number; rowsAffected: number | null }
    | { kind: "notice"; statement: number | null; text: string }
    | { kind: "switched"; statement: number; database: string }
    | { kind: "error"; statement: number | null; error: CoreError };

  /** One empty sort, so the grid's prop does not change on every row count update. */
  const NO_SORT: SortKey[] = [];

  /**
   * A table tab's unsaved edits (docs/decisions/0010-editing-rows.md): new
   * cell values by "row:column" (null for NULL), rows marked for delete, and
   * how many new rows follow the page's. Replaced, never changed in place.
   */
  interface Edits {
    cells: Map<string, string | null>;
    deleted: Set<number>;
    added: number;
  }
  const NO_EDITS: Edits = { cells: new Map(), deleted: new Set(), added: 0 };

  // --- Pages: query tabs and table tabs (docs/decisions/0013-pages-and-tabs.md) ---

  /** What a page holds while another is shown. The page shown keeps these in the live state below. */
  interface PageState {
    sets: ResultSetView[];
    messages: Message[];
    activeTab: number | "messages";
    statements: string[];
    ranCount: { ran: number; total: number } | null;
    elapsedMs: number | null;
    lastRun: { request: RunRequest; database: string | null } | null;
    tableTab: TableTab | null;
    edits: Edits;
    filtersOpen: boolean;
    keyView: { name: string; detail: KeyDetail } | null;
    cursor: { result: number; row: number; column: number } | null;
    selected: CellRect | null;
    /** A query page shows its results only once something ran in it. */
    ran: boolean;
    /** What the last run sent from the editor, to frame the statement of the result shown. */
    ranRequest: RunRequest | null;
    ranRanges: ByteRange[];
  }

  interface Page {
    id: number;
    kind: "query" | "table";
    /** A query page's number, for its title ("Query 2"). */
    number: number;
    /** A query page's editor, kept while another page is shown. */
    editor: EditorState | null;
    state: PageState;
  }

  function emptyState(): PageState {
    return {
      sets: [],
      messages: [],
      activeTab: 0,
      statements: [],
      ranCount: null,
      elapsedMs: null,
      lastRun: null,
      tableTab: null,
      edits: NO_EDITS,
      filtersOpen: false,
      keyView: null,
      cursor: null,
      selected: null,
      ran: false,
      ranRequest: null,
      ranRanges: [],
    };
  }

  let nextPageId = 2;
  let pages: Page[] = $state.raw([{ id: 1, kind: "query", number: 1, editor: null, state: emptyState() }]);
  let activePage = $state(1);
  const currentPage = $derived(pages.find((page) => page.id === activePage));
  const onTablePage = $derived(currentPage?.kind === "table");

  // --- The connection and the page shown ---------------------------------

  const startSettings = readSettings();
  let settings: Settings = $state(startSettings);
  applyTheme(startSettings.theme);
  setCompactRows(startSettings.compactRows);

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
  let ran = $state(false);
  let ranRequest = $state.raw<RunRequest | null>(null);
  let ranRanges = $state.raw<ByteRange[]>([]);
  let tree: ObjectTree | undefined = $state();
  let treeRefreshing = $state(false);

  let editorHeight = $state(188);
  let sidebarWidth = $state(260);
  let editor: SqlEditor | undefined = $state();

  /** The sidebar's folding panes (Objects always shows). */
  let queriesOpen = $state(false);
  let historyOpen = $state(false);
  let queryCount: number | null = $state(null);
  let historyCount: number | null = $state(null);
  /** Bumped after a run that went to the history, and after a save. */
  let historyKey = $state(0);
  let queriesKey = $state(0);
  /** Whether the run in progress goes to the history. */
  let recording = false;
  /** The save dialog: a new query or the one being edited. */
  let saving: SavedQuery | null = $state(null);

  let tableTab = $state.raw<TableTab | null>(null);
  let edits = $state.raw<Edits>(NO_EDITS);
  /** The cell being edited, and the value it had when read (undefined for a new row's cell). */
  let editing = $state.raw<{
    row: number;
    column: number;
    box: CellBox;
    initial: string | null;
    replaced: boolean;
    original: string | null | undefined;
  } | null>(null);
  let preview = $state.raw<{ statements: string[]; busy: boolean; error: CoreError | null } | null>(null);
  const hasEdits = $derived(edits.cells.size > 0 || edits.deleted.size > 0 || edits.added > 0);
  /** A Redis key open in the results, and what reading it found. */
  let keyView = $state.raw<{ name: string; detail: KeyDetail } | null>(null);
  let keyBrowser: KeyBrowser | undefined = $state();
  const keyValue = $derived(connection?.engine.dataModel === "keyValue");
  let filtersOpen = $state(false);
  let filterRow: FilterRow | undefined = $state();
  let pager: Pager | undefined = $state();
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
  const gridRows = $derived((activeSet?.totalRows ?? 0) + (tableTab && activeTab === 0 ? edits.added : 0));
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

  /** The page shown's live state, to keep while another page is shown. */
  function stash(): PageState {
    return {
      sets,
      messages,
      activeTab,
      statements,
      ranCount,
      elapsedMs,
      lastRun,
      tableTab,
      edits,
      filtersOpen,
      keyView,
      cursor,
      selected,
      ran,
      ranRequest,
      ranRanges,
    };
  }

  function unstash(state: PageState) {
    sets = state.sets;
    messages = state.messages;
    activeTab = state.activeTab;
    statements = state.statements;
    ranCount = state.ranCount;
    elapsedMs = state.elapsedMs;
    lastRun = state.lastRun;
    tableTab = state.tableTab;
    edits = state.edits;
    filtersOpen = state.filtersOpen;
    keyView = state.keyView;
    cursor = state.cursor;
    selected = state.selected;
    ran = state.ran;
    ranRequest = state.ranRequest;
    ranRanges = state.ranRanges;
    editing = null;
    preview = null;
  }

  /** Keeps the page shown (its editor too), so another can be shown. */
  function keepCurrent(): Page[] {
    return pages.map((page) =>
      page.id === activePage ? { ...page, state: stash(), editor: page.kind === "query" ? (editor?.saveState() ?? page.editor) : null } : page,
    );
  }

  /** Shows page `id`. A run in progress keeps its page shown until it ends. */
  function switchTo(id: number): boolean {
    if (id === activePage) return true;
    if (running) {
      note("Wait for the run to end, or stop it, before you switch pages");
      return false;
    }
    const kept = keepCurrent();
    const target = kept.find((page) => page.id === id);
    if (!target) return false;
    pages = kept;
    activePage = id;
    unstash(target.state);
    if (target.kind === "query") {
      editor?.restoreState(target.editor);
      void tick().then(() => editor?.focus());
    }
    return true;
  }

  function newQueryPage() {
    if (running) {
      note("Wait for the run to end, or stop it, before you open a page");
      return;
    }
    const number = Math.max(0, ...pages.filter((page) => page.kind === "query").map((page) => page.number)) + 1;
    const page: Page = { id: nextPageId++, kind: "query", number, editor: null, state: emptyState() };
    pages = [...keepCurrent(), page];
    activePage = page.id;
    unstash(page.state);
    editor?.restoreState(null);
    void tick().then(() => editor?.focus());
  }

  function releaseState(state: PageState) {
    for (const set of state.sets) void ipc.releaseResult(set.result);
  }

  /** Closes page `id`; the last query page closing leaves a fresh empty one, so there is always an editor. */
  function closePage(id: number) {
    const page = pages.find((candidate) => candidate.id === id);
    if (!page) return;
    const shown = id === activePage;
    if (shown && running) {
      note("Stop the run before you close its page");
      return;
    }
    const state = shown ? stash() : page.state;
    const pending = state.edits.cells.size > 0 || state.edits.deleted.size > 0 || state.edits.added > 0;
    if (pending) {
      if (!shown) switchTo(id);
      note("Save or discard the table's edits first (Ctrl+S)");
      return;
    }
    releaseState(state);
    const index = pages.findIndex((candidate) => candidate.id === id);
    let rest = (shown ? pages : keepCurrent()).filter((candidate) => candidate.id !== id);
    if (!rest.some((candidate) => candidate.kind === "query")) {
      rest = [...rest, { id: nextPageId++, kind: "query", number: page.kind === "query" ? page.number + 1 : 1, editor: null, state: emptyState() }];
    }
    pages = rest;
    if (!shown) return;
    const next = rest[Math.min(index, rest.length - 1)] ?? rest[0];
    if (!next) return;
    activePage = next.id;
    unstash(next.state);
    if (next.kind === "query") editor?.restoreState(next.editor);
  }

  /** Every page's result sets go, and one empty query page stays; the editor keeps its text. */
  function resetPages() {
    for (const page of pages) if (page.id !== activePage) releaseState(page.state);
    clearResults();
    const kept = currentPage?.kind === "query" ? currentPage : null;
    pages = [{ id: nextPageId++, kind: "query", number: 1, editor: null, state: emptyState() }];
    activePage = pages[0]?.id ?? 1;
    unstash(emptyState());
    if (!kept) editor?.restoreState(null);
  }

  function pageTitle(page: Page): string {
    if (page.kind === "query") return `Query ${page.number}`;
    const tab = page.id === activePage ? tableTab : page.state.tableTab;
    return tab?.table.name ?? "Table";
  }

  function pageCount(page: Page): string {
    const tab = page.id === activePage ? tableTab : page.state.tableTab;
    return typeof tab?.count === "number" ? numberFormat.format(tab.count) : "";
  }

  function tabsWheel(event: WheelEvent) {
    const element = event.currentTarget as HTMLElement;
    if (event.deltaY !== 0 && element.scrollWidth > element.clientWidth) element.scrollLeft += event.deltaY;
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
    editRequested: (cell, typed) => void beginEdit(cell, typed),
    deleteRequested: (firstRow, rowCount) => markDeleted(firstRow, rowCount),
  };

  function anyEdits(): boolean {
    if (hasEdits) return true;
    return pages.some((page) => page.id !== activePage && (page.state.edits.cells.size > 0 || page.state.edits.deleted.size > 0 || page.state.edits.added > 0));
  }

  /** Opens a connection in place of the current one. Resolves to the error, or null. */
  async function openWith(opening: () => Promise<OpenedConnection>): Promise<CoreError | null> {
    if (anyEdits()) return { code: "config.invalid", message: "Save or discard the table's edits first (Ctrl+S)" };
    connecting = true;
    try {
      const opened = await opening();
      if (connection) void ipc.disconnect(connection.id);
      resetPages();
      connection = opened;
      database = opened.database;
      await tick();
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
    if (anyEdits()) {
      note("Save or discard the table's edits first (Ctrl+S)");
      return;
    }
    void ipc.disconnect(connection.id);
    resetPages();
    connection = null;
    database = null;
  }

  /** The connect dialog: the engine it is for, and the saved connection it edits. */
  let connectDialog: { engine: NetworkEngine; saved: SavedConnection | null } | null = $state(null);
  let savedKey = $state(0);
  let library: SavedConnection[] = $state.raw([]);

  function readLibrary() {
    ipc.listSaved().then(
      (list) => (library = list),
      () => (library = []),
    );
  }

  $effect(() => {
    void savedKey;
    readLibrary();
  });

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
    if (saved.savePassword) return false;
    if (target.kind === "http") return true;
    return target.kind === "network" && !(saved.config.engine === "mssql" && !target.user);
  }

  async function openSaved(saved: SavedConnection, password: string | null = null) {
    if (password === null && needsPassword(saved)) {
      askPassword = saved;
      return;
    }
    askPassword = null;
    connectDialog = null;
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

  function setRowCap(cap: number | null) {
    settings = { ...settings, rowCap: cap };
    writeSettings(settings);
  }

  function saveSettings(next: Settings) {
    const density = next.compactRows !== settings.compactRows;
    settings = next;
    writeSettings(next);
    applyTheme(next.theme);
    setCompactRows(next.compactRows);
    settingsOpen = false;
    if (density) grid?.reload();
  }

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
    if (hasEdits) {
      note("Save or discard the table's edits first (Ctrl+S)");
      return;
    }
    pageRun = overrides.tablePage === true;
    if (!pageRun && onTablePage) return;
    keyView = null;
    clearResults();
    statements = [];
    ranCount = null;
    running = true;
    ran = true;
    const runDatabase = overrides.inDatabase !== undefined ? overrides.inDatabase : database;
    const rowLimit = request.unlimited ? null : overrides.rowLimit !== undefined ? overrides.rowLimit : settings.rowCap;
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
        settings.followCreatedDatabase,
        rowLimit,
        record,
        onEvent,
      );
      runId = started.run;
      ranCount = { ran: started.ranges.length, total: started.scriptStatements };
      if (fromEditor) {
        editor?.showRan(request, started.ranges);
        ranRequest = request;
        ranRanges = started.ranges;
      } else if (!pageRun) {
        ranRequest = null;
        ranRanges = [];
      }
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

  // With two results or more, the statement behind the result shown is framed in the editor (D23).
  $effect(() => {
    const set = activeSet;
    const request = ranRequest;
    const ranges = ranRanges;
    const many = sets.length > 1;
    untrack(() => {
      const range = set && many && !onTablePage ? ranges[set.statement] : undefined;
      editor?.frame(range ? request : null, range ?? null);
    });
  });

  /** Opens a table or view from the tree on a page of its own; `newTab` always opens another page. */
  async function openObject(object: ObjectInfo, inDatabase: string | null, newTab = false) {
    if (object.kind !== "table" && object.kind !== "view" && object.kind !== "materializedView") return;
    if (!connection) return;
    if (running) {
      note("Wait for the run to end, or stop it, before you open a table");
      return;
    }
    const title = object.schema ? `${object.schema}.${object.name}` : object.name;
    const dbName = inDatabase ?? database;
    if (!newTab) {
      const shown = (page: Page) => (page.id === activePage ? tableTab : page.state.tableTab);
      const existing = pages.find((page) => page.kind === "table" && shown(page)?.title === title && shown(page)?.database === dbName);
      if (existing) {
        switchTo(existing.id);
        return;
      }
    }
    const table: TableRef = { schema: object.schema, name: object.name };
    let columns: ColumnInfo[] = [];
    try {
      columns = await ipc.fetchColumns(connection.id, dbName, table);
    } catch {
      // The page still opens; without the catalog it has no key, and its columns come from the page.
    }
    const key = columns
      .filter((column) => column.primaryKey !== null)
      .sort((a, b) => (a.primaryKey ?? 0) - (b.primaryKey ?? 0))
      .map((column) => column.name);
    const next: TableTab = {
      database: dbName,
      table,
      title,
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
    };
    const page: Page = { id: nextPageId++, kind: "table", number: 0, editor: null, state: { ...emptyState(), tableTab: next } };
    pages = [...keepCurrent(), page];
    activePage = page.id;
    unstash(page.state);
    await loadPage(next, true);
  }

  /** Reads one page of a table tab; with `recount`, counts the rows its filters match after. */
  async function loadPage(next: TableTab, recount: boolean) {
    if (!connection || running) return;
    if (hasEdits) {
      note("Save or discard the table's edits first (Ctrl+S)");
      return;
    }
    let browse;
    try {
      browse = await ipc.browseStatements(connection.id, {
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
    tableTab = { ...next, pageSql: browse.page, countSql: browse.count, count: recount ? null : next.count };
    countAfterPage = recount;
    await startRun({ sql: browse.page, cursor: null, origin: 0 }, false, {
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

  /** A table tab's filters as the filter row edits them: columns by position. */
  const tableConditions = $derived.by((): Condition[] => {
    const tab = tableTab;
    if (!tab) return [];
    return tab.filters.map((filter) => ({
      column: filter.op === "raw" ? -1 : Math.max(0, tab.columns.indexOf(filter.column)),
      op: filter.op,
      value: filter.value,
      second: filter.second,
    }));
  });

  function applyTableFilters(conditions: Condition[], matchAny: boolean) {
    const tab = tableTab;
    if (!tab) return;
    const filters: Filter[] = conditions.map((condition) => ({
      column: condition.op === "raw" ? "" : (tab.columns[condition.column] ?? ""),
      op: condition.op,
      value: condition.value,
      second: condition.second,
    }));
    void loadPage({ ...tab, filters, matchAny, offset: 0 }, true);
  }

  async function toggleFilters() {
    filtersOpen = !filtersOpen;
    if (filtersOpen) {
      await tick();
      await filterRow?.focus();
    }
  }

  /** Reads a key into the results, up to the row cap unless `unlimited`. */
  async function openKey(name: string, unlimited = false) {
    if (!connection || running) return;
    try {
      const read = await ipc.readKey(connection.id, database, name, unlimited ? null : settings.rowCap);
      clearResults();
      ran = true;
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

  // --- Editing a table tab (docs/decisions/0010-editing-rows.md) ---------

  /** The page's result set, while a table tab shows it. */
  const pageSet = $derived(tableTab ? sets[0] : undefined);
  const canEdit = $derived(
    tableTab !== null && tableTab.key.length > 0 && !running && pageSet?.finished === true && activeTab === 0,
  );
  const editOverlay: EditOverlay | null = $derived.by(() => {
    const set = pageSet;
    const pending = edits;
    if (!set || !hasEdits) return null;
    return {
      baseRows: set.totalRows,
      cell: (row, column) => {
        const key = `${row}:${column}`;
        return pending.cells.has(key) ? (pending.cells.get(key) ?? null) : undefined;
      },
      deleted: (row) => pending.deleted.has(row),
    };
  });
  const editSummary = $derived.by(() => {
    const set = pageSet;
    if (!set) return "";
    const changed = new Set<number>();
    for (const key of edits.cells.keys()) {
      const row = Number(key.split(":")[0]);
      if (row < set.totalRows && !edits.deleted.has(row)) changed.add(row);
    }
    const parts = [];
    if (changed.size > 0) parts.push(`${changed.size} changed`);
    if (edits.deleted.size > 0) parts.push(`${edits.deleted.size} to delete`);
    if (edits.added > 0) parts.push(`${edits.added} new`);
    return parts.join(" · ");
  });
  /** Every page row in the selection is marked for delete, so the button unmarks them. */
  const selectionDeletes = $derived.by(() => {
    const set = pageSet;
    const rect = selected;
    if (!set || !rect || rect.firstRow >= set.totalRows) return false;
    const last = Math.min(rect.firstRow + rect.rowCount, set.totalRows);
    for (let row = rect.firstRow; row < last; row++) if (!edits.deleted.has(row)) return false;
    return true;
  });

  function cannotEdit() {
    if (tableTab && tableTab.key.length === 0) note("This table has no primary key, so its rows cannot be named for an edit");
  }

  async function beginEdit(cell: Cell, typed: string | null) {
    const set = pageSet;
    if (!canEdit || !set) return cannotEdit();
    if (edits.deleted.has(cell.row)) return;
    const key = `${cell.row}:${cell.column}`;
    let original: string | null | undefined;
    if (cell.row < set.totalRows) {
      try {
        original = await ipc.cellText(set.result, cell.row, cell.column);
      } catch (caught) {
        note(`Not read: ${asCoreError(caught).message}`);
        return;
      }
    }
    const current = edits.cells.has(key) ? (edits.cells.get(key) ?? null) : original === undefined ? "" : original;
    const box = grid?.cellBox(cell.row, cell.column);
    if (!box) return;
    editing = { row: cell.row, column: cell.column, box, initial: typed ?? current, replaced: typed !== null, original };
  }

  function commitEdit(value: string | null, move: Move) {
    const target = editing;
    const set = pageSet;
    if (!target || !set) return;
    editing = null;
    const key = `${target.row}:${target.column}`;
    const cells = new Map(edits.cells);
    // Back to what it was: no longer an edit.
    if (target.original !== undefined && value === target.original) cells.delete(key);
    else cells.set(key, value);
    edits = { ...edits, cells };
    const lastRow = set.totalRows + edits.added - 1;
    const lastColumn = set.columns.length - 1;
    let { row, column } = target;
    if (move === "down") row = Math.min(lastRow, row + 1);
    else if (move === "right") column = Math.min(lastColumn, column + 1);
    else if (move === "left") column = Math.max(0, column - 1);
    if (move !== null) {
      grid?.focusCell(row, column);
      grid?.focus();
    }
  }

  function cancelEdit() {
    editing = null;
    grid?.focus();
  }

  /** Delete on rows: marks the page's rows (or unmarks them, when all are marked) and drops new ones. */
  function markDeleted(firstRow: number, rowCount: number) {
    const set = pageSet;
    if (!canEdit || !set) return cannotEdit();
    const base = set.totalRows;
    const rows = Array.from({ length: rowCount }, (_, offset) => firstRow + offset);
    const existing = rows.filter((row) => row < base);
    const removed = new Set(rows.filter((row) => row >= base));
    const deleted = new Set(edits.deleted);
    const unmark = existing.length > 0 && existing.every((row) => deleted.has(row));
    for (const row of existing) {
      if (unmark) deleted.delete(row);
      else deleted.add(row);
    }
    let cells = edits.cells;
    if (removed.size > 0) {
      // The new rows after a removed one move up.
      cells = new Map();
      for (const [key, value] of edits.cells) {
        const [row = 0, column = 0] = key.split(":").map(Number);
        if (removed.has(row)) continue;
        const shift = row >= base ? [...removed].filter((gone) => gone < row).length : 0;
        cells.set(`${row - shift}:${column}`, value);
      }
    }
    edits = { cells, deleted, added: edits.added - removed.size };
  }

  async function addRow() {
    const set = pageSet;
    if (!canEdit || !set) return cannotEdit();
    edits = { ...edits, added: edits.added + 1 };
    await tick();
    grid?.focusCell(set.totalRows + edits.added - 1, 0);
    grid?.focus();
  }

  async function setNullAtCursor() {
    const set = pageSet;
    const at = cursor;
    if (!canEdit || !set || !at || at.result !== set.result || edits.deleted.has(at.row)) return cannotEdit();
    const original = at.row < set.totalRows ? await ipc.cellText(set.result, at.row, at.column) : undefined;
    const key = `${at.row}:${at.column}`;
    const cells = new Map(edits.cells);
    if (original === null) cells.delete(key);
    else cells.set(key, null);
    edits = { ...edits, cells };
  }

  function discardEdits() {
    edits = NO_EDITS;
    editing = null;
    preview = null;
  }

  function editRequest(): EditRequest | null {
    const set = pageSet;
    const tab = tableTab;
    if (!set || !tab) return null;
    const base = set.totalRows;
    const toValue = (value: string | null): NewValue => (value === null ? { kind: "null" } : { kind: "text", text: value });
    const updates = new Map<number, CellEdit[]>();
    const inserts: CellEdit[][] = Array.from({ length: edits.added }, () => []);
    for (const [key, value] of edits.cells) {
      const [row = 0, column = 0] = key.split(":").map(Number);
      const cell = { column, value: toValue(value) };
      if (row >= base) inserts[row - base]?.push(cell);
      else if (!edits.deleted.has(row)) updates.set(row, [...(updates.get(row) ?? []), cell]);
    }
    return {
      result: set.result,
      table: tab.table,
      key: tab.key,
      updates: [...updates].map(([row, cells]) => ({ row, cells })),
      deletes: [...edits.deleted].sort((a, b) => a - b),
      inserts,
    };
  }

  /** Ctrl+S: the statements Save would run, to read before they run. */
  async function openPreview() {
    const request = editRequest();
    if (!request || !connection || !hasEdits || editing) return;
    try {
      preview = { statements: await ipc.editPreview(connection.id, request), busy: false, error: null };
    } catch (caught) {
      note(`Not saved: ${asCoreError(caught).message}`);
    }
  }

  async function saveEdits() {
    const shown = preview;
    const tab = tableTab;
    if (!shown || !connection || !tab || shown.busy) return;
    preview = { ...shown, busy: true, error: null };
    try {
      await ipc.applyEdits(connection.id, tab.database, shown.statements);
      edits = NO_EDITS;
      preview = null;
      note(`Saved ${shown.statements.length} ${shown.statements.length === 1 ? "change" : "changes"}`);
      await loadPage(tab, true);
    } catch (caught) {
      preview = { ...shown, busy: false, error: asCoreError(caught) };
    }
  }

  function pageTo(offset: number) {
    if (tableTab && offset >= 0) void loadPage({ ...tableTab, offset }, false);
  }

  function setPageSize(limit: number) {
    const tab = tableTab;
    if (!tab) return;
    pageSize = limit;
    try {
      localStorage.setItem(PAGE_SIZE_KEY, String(limit));
    } catch {
      // Not remembered, still used.
    }
    void loadPage({ ...tab, limit, offset: 0 }, false);
  }

  function copyPageSql() {
    if (!tableTab) return;
    navigator.clipboard.writeText(tableTab.pageSql).then(
      () => note("Copied the page's SELECT"),
      (caught: unknown) => note(`Not copied: ${String(caught)}`),
    );
  }

  function copyText(text: string, what: string) {
    navigator.clipboard.writeText(text).then(
      () => note(`Copied ${what}`),
      (caught: unknown) => note(`Not copied: ${String(caught)}`),
    );
  }

  /** Runs a statement from the history in the database it ran in, when it ran here. */
  function runEntry(entry: HistoryEntry) {
    if (!connection || running) return;
    if (onTablePage) newQueryPage();
    const here = entry.connection === connection.title && connection.engine.hasDatabases;
    if (here && entry.database) database = entry.database;
    editor?.insert(entry.statement);
    void startRun({ sql: entry.statement, cursor: null, origin: 0 }, false, { record: true });
  }

  function runSaved(query: SavedQuery) {
    if (!connection || running) return;
    if (onTablePage) newQueryPage();
    editor?.insert(query.sql);
    void startRun({ sql: query.sql, cursor: null, origin: 0 }, false, { record: true });
  }

  function insertSql(sql: string) {
    if (onTablePage) newQueryPage();
    editor?.insert(sql);
  }

  /** Ctrl+Shift+D: keeps the editor's text, or its selection, as a named query. */
  function saveFromEditor() {
    if (onTablePage) return;
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

  async function refreshObjects() {
    if (!connection) return;
    void ipc.forgetCatalog(connection.id);
    if (keyValue) {
      keyBrowser?.refresh();
      return;
    }
    treeRefreshing = true;
    try {
      await tree?.refresh();
    } finally {
      treeRefreshing = false;
    }
  }

  // --- Menus and dialogs ---------------------------------------------------

  let paletteOpen = $state(false);
  let settingsOpen = $state(false);
  let connMenuOpen = $state(false);
  let runMenuOpen = $state(false);
  let treeMenu = $state.raw<{ target: TreeTarget; x: number; y: number } | null>(null);

  function openPalette() {
    readLibrary();
    paletteOpen = true;
  }

  function newConnection() {
    connectDialog = { engine: "postgres", saved: null };
  }

  const ENGINE_NAMES: Record<string, string> = { postgres: "PostgreSQL", mssql: "SQL Server", redis: "Redis", d1: "Cloudflare D1", sqlite: "SQLite" };

  function savedWhere(entry: SavedConnection): string {
    const target = entry.config.target;
    const place = target.kind === "network" ? target.host : target.kind === "http" ? "Cloudflare API" : target.kind === "file" ? target.path : "";
    return `${ENGINE_NAMES[entry.config.engine] ?? entry.config.engine} · ${place}`;
  }

  const commands: Command[] = $derived([
    ...(connection
      ? [{ id: "new-tab", group: "Start", label: "New query tab", icon: "file" as const, keys: ["Ctrl", "T"], run: newQueryPage }]
      : []),
    { id: "new-connection", group: "Start", label: "New connection…", icon: "plus", keys: ["Ctrl", "N"], run: newConnection },
    { id: "open-file", group: "Start", label: "Open file…", icon: "file", keys: ["Ctrl", "O"], run: () => void openFile() },
    { id: "memory", group: "Start", label: "Open an in-memory database", icon: "database", run: () => void connectTo(":memory:") },
    ...library.map((entry) => ({
      id: `saved:${entry.id}`,
      group: "Saved connections",
      label: entry.name,
      meta: savedWhere(entry),
      icon: "database" as const,
      run: () => void openSaved(entry),
    })),
    { id: "settings", group: "Settings", label: "Settings…", icon: "settings", keys: ["Ctrl", ","], run: () => (settingsOpen = true) },
    { id: "profile", group: "Settings", label: "Profile…", meta: "Later", icon: "key", disabled: true, run: () => {} },
  ]);

  // App-wide keys. WebView2 reloads the page on F5 and Ctrl+R, which would
  // drop every open connection, so those never reach it.
  function onWindowKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (ctrl && key === "k") {
      event.preventDefault();
      if (paletteOpen) paletteOpen = false;
      else openPalette();
    } else if (ctrl && key === "r") {
      event.preventDefault();
      void refreshObjects();
    } else if (ctrl && key === "n" && !event.shiftKey) {
      event.preventDefault();
      newConnection();
    } else if (ctrl && key === "o") {
      event.preventDefault();
      void openFile();
    } else if (ctrl && key === ",") {
      event.preventDefault();
      settingsOpen = true;
    } else if (ctrl && key === "t" && connection) {
      event.preventDefault();
      newQueryPage();
    } else if (ctrl && key === "w" && connection) {
      event.preventDefault();
      closePage(activePage);
    } else if (ctrl && event.key === "Tab" && connection && pages.length > 1) {
      event.preventDefault();
      const index = pages.findIndex((page) => page.id === activePage);
      const next = pages[(index + (event.shiftKey ? -1 : 1) + pages.length) % pages.length];
      if (next) switchTo(next.id);
    } else if (ctrl && !event.shiftKey && key === "s") {
      event.preventDefault();
      if (hasEdits) void openPreview();
    } else if (event.altKey && event.key === "Insert" && tableTab) {
      event.preventDefault();
      void addRow();
    } else if (ctrl && event.shiftKey && key === "n" && tableTab) {
      event.preventDefault();
      void setNullAtCursor();
    } else if (ctrl && event.shiftKey && key === "d") {
      event.preventDefault();
      if (!saving) saveFromEditor();
    } else if (ctrl && key === "f" && tableTab) {
      // The editor's own Ctrl+F (search) prevents the default, so it never gets here.
      event.preventDefault();
      if (!filtersOpen) void toggleFilters();
      else void filterRow?.focus();
    } else if (tableTab && ctrl && !event.altKey && (event.key === "[" || event.key === "]")) {
      event.preventDefault();
      pager?.step(event.key === "[" ? "previous" : "next");
    } else if (tableTab && ctrl && event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      pager?.step(event.key === "ArrowUp" ? "first" : "last");
    } else if (ctrl && event.altKey && key === "i") {
      event.preventDefault();
      detailOpen = !detailOpen;
    } else if (event.key === "F5") {
      event.preventDefault();
      if (connection && !running && !onTablePage) runFromToolbar("all");
    }
  }

  function runFromToolbar(mode: "statement" | "all") {
    runMenuOpen = false;
    const request = editor?.request(mode);
    if (request) void startRun(request);
  }

  function statementLabel(index: number | null): string {
    if (index === null) return "";
    const text = statements[index] ?? "";
    return text.length > 80 ? `${text.slice(0, 80)}…` : text;
  }

  /** The editor's height, dragged on the line above the results. */
  function dragEditor(event: PointerEvent) {
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const startY = event.clientY;
    const startHeight = editorHeight;
    const move = (moveEvent: PointerEvent) => {
      editorHeight = Math.max(72, Math.min(window.innerHeight - 300, startHeight + moveEvent.clientY - startY));
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  function dragSidebar(event: PointerEvent) {
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startWidth = sidebarWidth;
    const move = (moveEvent: PointerEvent) => {
      sidebarWidth = Math.max(180, Math.min(480, startWidth + moveEvent.clientX - startX));
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  const workRows = $derived(
    onTablePage ? "auto auto minmax(0, 1fr)" : ran ? `auto ${editorHeight}px auto minmax(0, 1fr)` : "auto minmax(0, 1fr)",
  );
  const numberFormat = new Intl.NumberFormat();
  const limitOptions = ROW_CAPS.map((cap) => ({ value: cap, label: cap === null ? "No limit" : `Limit ${numberFormat.format(cap)}` }));
  const tableError = $derived(onTablePage ? messages.find((message) => message.kind === "error") : undefined);
  const connectionColor = "gray";
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="pb-app" style:grid-template-rows={connectError ? "auto auto minmax(0, 1fr) auto" : undefined}>
  <!-- Toolbar: where you are (docs/decisions/0012-design-system.md, D14) -->
  {#if connection}
    {@const open = connection}
    <header class="pb-toolbar pb-toolbar--center has-conn" style:--conn="var(--conn-{connectionColor})">
      <div class="pb-tool-left">
        <div class="pb-combo" style="width: 240px">
          <button
            type="button"
            class="pb-input pb-select pb-combo-trigger pb-conn"
            aria-haspopup="menu"
            aria-expanded={connMenuOpen}
            aria-label="Connection {open.title}"
            data-tip-start=""
            data-tip="Edit or disconnect. Ctrl+K switches"
            onclick={() => (connMenuOpen = !connMenuOpen)}
          >
            <span class="pb-dot"></span><span class="pb-combo-text"
              ><span class="pb-name">{open.title}</span>&nbsp; <span class="pb-engine">{open.engine.displayName} {open.serverVersion ?? ""}</span></span
            >
          </button>
          {#if connMenuOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="pb-clickaway" onclick={() => (connMenuOpen = false)}></div>
            <div class="pb-pop" style="right: auto; width: 280px">
              <div class="pb-menu" role="menu" aria-label="Connection">
                <div class="pb-conn-card"><span><span class="pb-dot"></span>{open.title}</span><span>{open.engine.displayName} {open.serverVersion ?? ""}</span></div>
                <div class="pb-menu-sep"></div>
                {#if open.savedId}
                  {@const savedId = open.savedId}
                  <button
                    class="pb-menu-item"
                    role="menuitem"
                    onclick={() => {
                      connMenuOpen = false;
                      const entry = library.find((candidate) => candidate.id === savedId);
                      if (entry) editSaved(entry);
                    }}><Icon name="settings" />Edit this connection…</button
                  >
                {/if}
                <button
                  class="pb-menu-item is-danger"
                  role="menuitem"
                  disabled={running}
                  onclick={() => {
                    connMenuOpen = false;
                    disconnect();
                  }}><Icon name="unplug" />Disconnect</button
                >
              </div>
            </div>
          {/if}
        </div>
        {#if open.engine.hasDatabases}
          <DatabasePicker conn={open.id} {database} onPick={useDatabase} onError={note} />
        {/if}
      </div>
      <button type="button" class="pb-cmdk-trigger" style="width: 340px" aria-haspopup="dialog" aria-expanded={paletteOpen} onclick={openPalette}
        ><Icon name="search" /><span class="pb-cmdk-trigger-text">Search connections and commands…</span><span class="pb-keys"
          ><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">K</kbd></span
        ></button
      >
      <div class="pb-tool-right"></div>
    </header>
  {:else}
    <header class="pb-toolbar pb-toolbar--center">
      <div class="pb-tool-left"></div>
      <button type="button" class="pb-cmdk-trigger" style="width: 420px" aria-haspopup="dialog" aria-expanded={paletteOpen} onclick={openPalette}
        ><Icon name="search" /><span class="pb-cmdk-trigger-text">Search connections and commands…</span><span class="pb-keys"
          ><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">K</kbd></span
        ></button
      >
      <div class="pb-tool-right"></div>
    </header>
  {/if}

  {#if connectError}
    <div class="pb-info pb-info--danger pb-info--strip" role="alert">
      <Icon name="error" />
      <div class="pb-info-text"><span class="pb-info-title">{errorTitle(connectError)}</span><span class="pb-info-msg">{connectError.message}</span></div>
      <button class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Dismiss" onclick={() => (connectError = null)}><Icon name="x" /></button>
    </div>
  {/if}

  {#if connection}
    {@const open = connection}
    <div class="pb-mid" style:grid-template-columns="{sidebarWidth}px minmax(0, 1fr)">
      <!-- Sidebar: Objects fills it; Queries and History fold (D19) -->
      <aside class="pb-side" aria-label="Objects, saved queries and history">
        <section class="pb-pane is-open is-main" aria-label={keyValue ? "Keys" : "Objects"}>
          <div class="pb-pane-head is-static">
            {keyValue ? "Keys" : "Objects"}
            <span class="pb-pane-actions">
              <button
                class="pb-btn pb-btn--ghost pb-pane-btn"
                aria-label={keyValue ? "Scan the keys again" : "Refresh objects"}
                data-tip-end=""
                data-tip={keyValue ? "Scan the keys again  Ctrl+R" : "Refresh objects  Ctrl+R"}
                disabled={treeRefreshing}
                onclick={() => void refreshObjects()}><Icon name="refresh" spin={treeRefreshing} /></button
              >
            </span>
          </div>
          {#if keyValue}
            <KeyBrowser
              bind:this={keyBrowser}
              conn={open.id}
              {database}
              {refreshKey}
              openKey={keyView?.name ?? null}
              onOpen={(key: KeyInfo) => void openKey(key.name)}
              onBusy={(busy) => (treeRefreshing = busy)}
            />
          {:else}
            <ObjectTree
              bind:this={tree}
              connection={open}
              {database}
              {refreshKey}
              onOpen={(object, inDatabase) => void openObject(object, inDatabase)}
              onUseDatabase={useDatabase}
              onContext={(target, x, y) => (treeMenu = { target, x, y })}
            />
          {/if}
        </section>
        {#if !keyValue}
          <section class="pb-pane" class:is-open={queriesOpen} aria-label="Saved queries">
            <button class="pb-pane-head" aria-expanded={queriesOpen} onclick={() => (queriesOpen = !queriesOpen)}>
              <Icon name={queriesOpen ? "chev-down" : "chev-right"} class="pb-twist" />Queries{#if queryCount !== null}<span class="pb-pane-count"
                  >{queryCount}</span
                >{/if}
            </button>
            <!-- Kept mounted while folded, so the header's count stays right. -->
            <div class="pb-pane-body" hidden={!queriesOpen}>
              <QueryList
                connectionId={open.savedId}
                refreshKey={queriesKey}
                onInsert={insertSql}
                onRun={runSaved}
                onEdit={(query) => (saving = query)}
                onCount={(count) => (queryCount = count)}
              />
            </div>
          </section>
        {/if}
        <section class="pb-pane" class:is-open={historyOpen} aria-label="History">
          <button class="pb-pane-head" aria-expanded={historyOpen} onclick={() => (historyOpen = !historyOpen)}>
            <Icon name={historyOpen ? "chev-down" : "chev-right"} class="pb-twist" />History{#if historyCount !== null}<span class="pb-pane-count"
                >{historyCount}{historyCount === 100 ? "+" : ""}</span
              >{/if}
          </button>
          <div class="pb-pane-body" hidden={!historyOpen}>
            <HistoryList connection={open.title} refreshKey={historyKey} onInsert={insertSql} onRun={runEntry} onCount={(count) => (historyCount = count)} />
          </div>
        </section>
      </aside>
      <div class="pb-vsplit" style:left="{sidebarWidth - 3}px" role="separator" aria-orientation="vertical" aria-label="Resize the sidebar" onpointerdown={dragSidebar}></div>

      <main class="pb-work" style:grid-template-rows={workRows}>
        <!-- Pages: tabs on the left, the open page's tools on the right (D15) -->
        <div class="pb-doctabs">
          <div class="pb-doctabs-left">
            <div class="pb-doctabs-tabs" role="tablist" aria-label="Open pages" onwheel={tabsWheel}>
              {#each pages as page (page.id)}
                <span
                  class="pb-doctab is-link"
                  class:is-active={page.id === activePage}
                  role="tab"
                  tabindex="0"
                  aria-selected={page.id === activePage}
                  onclick={() => switchTo(page.id)}
                  onkeydown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      switchTo(page.id);
                    }
                  }}
                >
                  <Icon name={page.kind === "table" ? "table" : "file"} />{pageTitle(page)}
                  {#if page.kind === "table" && pageCount(page)}<span class="pb-count">{pageCount(page)}</span>{/if}
                  <button
                    type="button"
                    class="pb-doctab-x"
                    aria-label="Close {pageTitle(page)}"
                    onclick={(event) => {
                      event.stopPropagation();
                      closePage(page.id);
                    }}><Icon name="x" /></button
                  >
                </span>
              {/each}
            </div>
            <button type="button" class="pb-doctab-new" aria-label="New query tab" data-tip="New query  Ctrl+T" onclick={newQueryPage}><Icon name="plus" /></button>
          </div>
          <div class="pb-doctabs-tools">
            {#if onTablePage && tableTab}
              {@const tab = tableTab}
              {#if !hasEdits}
                {#if tab.key.length === 0}
                  <span class="pb-badge pb-badge--warning" data-tip="No primary key, so a row cannot be named for an edit"><Icon name="lock" size="sm" />Read-only</span>
                {/if}
                <button class="pb-btn pb-btn--ghost pb-btn--sm" class:is-open={filtersOpen} aria-pressed={filtersOpen} data-tip="Filter  Ctrl+F" onclick={() => void toggleFilters()}
                  ><Icon name="filter" />Filter{#if tab.filters.length > 0}<span class="pb-badge pb-badge--sm">{tab.filters.length}</span>{/if}</button
                >
                {#if tab.key.length > 0}
                  <button class="pb-btn pb-btn--ghost pb-btn--sm" disabled={!canEdit} data-tip="Add a row  Alt+Insert" onclick={() => void addRow()}><Icon name="plus" />Row</button>
                  <button
                    class="pb-btn pb-btn--ghost pb-btn--sm"
                    disabled={!canEdit || !selected}
                    data-tip="Mark the selected rows for delete  Delete"
                    onclick={() => selected && markDeleted(selected.firstRow, selected.rowCount)}><Icon name="trash" />Delete rows</button
                  >
                {/if}
                <span class="pb-sep"></span>
                <button class="pb-btn pb-btn--ghost pb-btn--sm" data-tip-end="" data-tip="Copy the SELECT this page ran, to edit and run it yourself" onclick={copyPageSql}
                  ><Icon name="copy" />Copy SQL</button
                >
              {:else}
                <button
                  class="pb-btn pb-btn--ghost pb-btn--sm"
                  disabled={!canEdit || !selected}
                  data-tip={selectionDeletes ? "Unmark the selected rows  Delete" : "Mark the selected rows for delete  Delete"}
                  onclick={() => selected && markDeleted(selected.firstRow, selected.rowCount)}
                  ><Icon name="trash" />{selectionDeletes ? "Keep rows" : "Delete rows"}</button
                >
                <span class="pb-badge"><span class="pb-badge-dot"></span>{editSummary}</span>
                <button class="pb-btn pb-btn--secondary pb-btn--sm" disabled={running} onclick={discardEdits}>Discard</button>
                <button
                  class="pb-btn pb-btn--primary pb-btn--sm"
                  disabled={running || editing !== null}
                  data-tip-end=""
                  data-tip="See the statements, then save  Ctrl+S"
                  onclick={() => void openPreview()}>Preview &amp; save</button
                >
              {/if}
            {:else}
              <div class="pb-runbar pb-runbar--inline" role="toolbar" aria-label="Run">
                {#if running}
                  <button class="pb-btn pb-btn--primary pb-btn--sm" data-tip-start="" data-tip="Stop  Esc" onclick={stopRun}><Icon name="stop" />Stop</button>
                  <span class="pb-runbar-status"><Icon name="loader" spin />Running…</span>
                {:else}
                  <div class="pb-anchor">
                    <span class="pb-split">
                      <button class="pb-btn pb-btn--primary pb-btn--sm" data-tip="Run statement  Ctrl+Enter" onclick={() => runFromToolbar("statement")}
                        ><Icon name="play" />Run</button
                      >
                      <button
                        class="pb-btn pb-btn--primary pb-btn--sm"
                        aria-label="Run options"
                        aria-haspopup="menu"
                        aria-expanded={runMenuOpen}
                        onclick={() => (runMenuOpen = !runMenuOpen)}><Icon name="chev-down" class="pb-chev" /></button
                      >
                    </span>
                    {#if runMenuOpen}
                      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                      <div class="pb-clickaway" onclick={() => (runMenuOpen = false)}></div>
                      <div class="pb-pop pb-pop--end">
                        <div class="pb-menu" role="menu" style="min-width: 280px">
                          <button class="pb-menu-item" role="menuitem" onclick={() => runFromToolbar("statement")}
                            ><Icon name="play" />Run statement<span class="pb-keys"><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">↵</kbd></span></button
                          >
                          <button class="pb-menu-item" role="menuitem" onclick={() => runFromToolbar("all")}
                            ><Icon name="play" />Run all<span class="pb-keys"
                              ><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">Shift</kbd><kbd class="pb-kbd">↵</kbd></span
                            ></button
                          >
                        </div>
                      </div>
                    {/if}
                  </div>
                {/if}
                {#if !keyValue}
                  <Combobox label="Row limit" small width={132} placement="end" listWidth={180} options={limitOptions} value={settings.rowCap} onPick={setRowCap} tip="Rows to fetch before stopping" />
                  <span class="pb-sep"></span>
                  <button class="pb-btn pb-btn--ghost pb-btn--sm" disabled={saving !== null} data-tip-end="" data-tip="Save this query  Ctrl+Shift+D" onclick={saveFromEditor}
                    ><Icon name="save" />Save query</button
                  >
                {/if}
              </div>
            {/if}
          </div>
        </div>

        <!-- One editor for every query page; a table page hides it (D17). -->
        <section class="pb-editor-host" hidden={onTablePage} aria-label="Editor">
          <SqlEditor
            bind:this={editor}
            {grammar}
            plain={keyValue}
            initial={"select 1 as one, 'hello' as greeting;\n"}
            onRun={(request) => void startRun(request)}
            complete={completeAt}
          />
        </section>

        {#if onTablePage}
          <div>
            {#if filtersOpen && tableTab}
              {#key `${tableTab.database}\u0000${tableTab.title}`}
                <FilterRow
                  bind:this={filterRow}
                  columns={tableTab.columns}
                  conditions={tableConditions}
                  matchAny={tableTab.matchAny}
                  allowRaw
                  busy={running}
                  onApply={applyTableFilters}
                />
              {/key}
            {/if}
            {#if tableError && tableError.kind === "error"}
              <div class="pb-info pb-info--danger pb-info--strip" role="alert">
                <Icon name="error" />
                <div class="pb-info-text"><span class="pb-info-title">{errorTitle(tableError.error)}</span><span class="pb-info-msg">{tableError.error.message}</span></div>
              </div>
            {/if}
          </div>
        {:else if ran}
          <div style="position: relative">
            <!-- Drag to share the height between the editor and the results; double-click puts it back. -->
            <!-- A separator that can be focused is a widget: the arrow keys move it. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="pb-splitter"
              role="separator"
              aria-orientation="horizontal"
              aria-label="Resize the editor and the results"
              aria-valuenow={editorHeight}
              tabindex="0"
              onpointerdown={dragEditor}
              ondblclick={() => (editorHeight = 188)}
              onkeydown={(event) => {
                if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                  event.preventDefault();
                  editorHeight = Math.max(72, Math.min(window.innerHeight - 300, editorHeight + (event.key === "ArrowDown" ? 16 : -16)));
                }
              }}
            ></div>
            {#if keyView}
              {@const view = keyView}
              <div class="pb-bar pb-keybar">
                <span class="pb-mono pb-key-name">{view.name}</span>
                <span class="pb-badge pb-badge--sm pb-badge--outline">{view.detail.kind === "sortedSet" ? "zset" : view.detail.kind}</span>
                {#if KEY_UNITS[view.detail.kind]}<span class="pb-count">{numberFormat.format(view.detail.length)} {KEY_UNITS[view.detail.kind]}</span>{/if}
                <span class="pb-count">{expiresIn(view.detail.ttlMs)}</span>
                <span class="pb-spacer"></span>
                <button class="pb-btn pb-btn--ghost pb-btn--sm" disabled={running} data-tip="Read the key again" onclick={() => void openKey(view.name)}
                  ><Icon name="refresh" />Reload</button
                >
                <button class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Copy the key name" data-tip-end="" data-tip="Copy the key name" onclick={() => copyText(view.name, "the key name")}
                  ><Icon name="copy" /></button
                >
              </div>
            {/if}
            <div class="pb-bar">
              <div class="pb-tabs" role="tablist">
                {#each sets as set, index (set.result)}
                  {@const range = ranRanges[set.statement]}
                  <button
                    class="pb-tab"
                    role="tab"
                    aria-selected={activeTab === index}
                    data-tip-start=""
                    data-tip={range && ranRequest ? `From statement ${set.statement + 1} of the run` : undefined}
                    onclick={() => (activeTab = index)}
                  >
                    Result {index + 1}
                    {#if set.limited}
                      <span class="pb-badge pb-badge--sm pb-badge--warning">{numberFormat.format(set.totalRows)}+</span>
                    {:else}
                      <span class="pb-count">{numberFormat.format(set.totalRows)}</span>
                    {/if}
                  </button>
                {/each}
                <button class="pb-tab" role="tab" aria-selected={activeTab === "messages"} onclick={() => (activeTab = "messages")}>
                  Messages{#if errorCount > 0}<span class="pb-count-badge">{errorCount}</span>{/if}
                </button>
              </div>
              <span class="pb-spacer"></span>
              <button
                class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon"
                class:is-open={detailOpen}
                aria-label="Row detail"
                aria-pressed={detailOpen}
                data-tip-end=""
                data-tip="Row detail  Ctrl+Alt+I"
                onclick={() => (detailOpen = !detailOpen)}><Icon name="pane" /></button
              >
            </div>
          </div>
        {/if}

        {#if onTablePage || ran}
          <div class="pb-results">
            {#if activeTab === "messages" && !onTablePage}
              <div class="pb-messages" role="log">
                {#each messages as message, index (index)}
                  <div class="pb-msg" class:is-error={message.kind === "error"}>
                    {#if message.kind === "error"}
                      <Icon name="error" class="pb-bad" />
                    {:else if message.kind === "switched"}
                      <Icon name="info" />
                    {:else if message.kind === "command"}
                      <Icon name="check-circle" class="pb-ok" />
                    {:else}
                      <Icon name="info" />
                    {/if}
                    <div class="pb-msg-text">
                      {#if message.kind === "error"}
                        <span><span class="pb-msg-title">{errorTitle(message.error)}</span> <span class="pb-msg-detail">{message.error.message}</span></span>
                      {:else if message.kind === "command"}
                        <span>{message.rowsAffected === null ? "Done" : `${numberFormat.format(message.rowsAffected)} rows affected`}</span>
                      {:else if message.kind === "switched"}
                        <span>Switched to {message.database}; the statements after this run there.</span>
                      {:else}
                        <span>{message.text}</span>
                      {/if}
                      {#if message.statement !== null}<span class="pb-msg-stmt">{statementLabel(message.statement)}</span>{/if}
                    </div>
                  </div>
                {:else}
                  <p class="pb-side-hint">No messages.</p>
                {/each}
              </div>
            {:else}
              <div class="pb-results-grid">
                <DataGrid
                  bind:this={grid}
                  source={gridSource}
                  totalRows={gridRows}
                  sortKeys={activeSet?.sort ?? NO_SORT}
                  callbacks={gridCallbacks}
                  edits={editOverlay}
                />
                {#if editing}
                  {#key `${editing.row}:${editing.column}`}
                    <CellEditor box={editing.box} initial={editing.initial} replaced={editing.replaced} onCommit={commitEdit} onCancel={cancelEdit} />
                  {/key}
                {/if}
              </div>
              {#if detailOpen && gridSource}
                <aside class="pb-detail-pane" aria-label="Row detail">
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
            {/if}
          </div>
        {/if}
      </main>
    </div>
  {:else}
    <!-- Welcome: no sidebar; the palette holds the saved connections (D25) -->
    <main class="pb-empty">
      <div class="pb-empty-mark"><Icon name="database" /></div>
      <div class="pb-empty-words">
        <h1 class="pb-empty-title">No connection open</h1>
        <p class="pb-empty-text">Search for a saved connection, make a new one, or open a SQLite file.</p>
      </div>
      <div class="pb-row">
        <button class="pb-btn pb-btn--primary" disabled={connecting} onclick={newConnection}><Icon name="plus" />New connection…</button>
        <button class="pb-btn pb-btn--secondary" disabled={connecting} onclick={() => void openFile()}><Icon name="file" />Open file…</button>
        <button class="pb-btn pb-btn--ghost" disabled={connecting} onclick={() => void connectTo(":memory:")}>Open an in-memory database</button>
      </div>
      <div class="pb-empty-keys">
        <span><span class="pb-keys"><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">K</kbd></span>Search</span>
        <span><span class="pb-keys"><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">N</kbd></span>New connection</span>
        <span><span class="pb-keys"><kbd class="pb-kbd">Ctrl</kbd><kbd class="pb-kbd">O</kbd></span>Open file</span>
      </div>
    </main>
  {/if}

  <!-- Status bar: what the result is; the pager for a table page (D18) -->
  <footer class="pb-status">
    {#if !connection}
      <span>{connecting ? "Connecting…" : "No connection"}</span>
      <span class="pb-spacer"></span>
      <span>Paboable</span>
    {:else}
      {#if running}
        <span class="is-live"><Icon name="loader" size="sm" spin />Running…</span>
      {:else if elapsedMs !== null}
        <span>{numberFormat.format(elapsedMs)} ms</span>
      {/if}
      {#if ranCount && !onTablePage}
        <span class:is-note={ranCount.ran < ranCount.total}>
          {ranCount.ran < ranCount.total
            ? `Ran ${ranCount.ran} of ${ranCount.total} statements`
            : `Ran ${ranCount.ran} ${ranCount.ran === 1 ? "statement" : "statements"}`}
        </span>
      {/if}
      {#if connection.engine.hasDatabases && database}<span>in {database}</span>{/if}
      {#if !ran && !onTablePage}<span>No result yet</span>{/if}
      {#if selected && selected.rowCount * selected.columnCount > 1}
        <span>{numberFormat.format(selected.rowCount)} × {numberFormat.format(selected.columnCount)} selected</span>
      {/if}
      {#if activeSet}
        {#if onTablePage && tableTab}
          <span
            >{numberFormat.format(activeSet.totalRows)} rows{#if typeof tableTab.count === "number"}&nbsp;of {numberFormat.format(tableTab.count)}{:else if tableTab.count === "counting"}&nbsp;of …{:else}&nbsp;<button
                class="pb-btn pb-btn--link"
                disabled={running}
                onclick={() => void countTable()}>Count</button
              >{/if}</span
          >
        {:else}
          <span>{numberFormat.format(activeSet.totalRows)} rows{activeSet.finished ? "" : " so far"}</span>
          {#if activeSet.limited}
            <span class="pb-badge pb-badge--warning"><span class="pb-badge-dot"></span>Stopped at {numberFormat.format(activeSet.totalRows)} rows</span>
            <button class="pb-btn pb-btn--link" disabled={running} onclick={fetchAll}>Fetch all</button>
          {/if}
          {#if activeSet.sortedBy === "server"}<span>sorted by the server</span>{/if}
          {#if activeSet.sortedBy === "core"}<span>sorted in Paboable</span>{/if}
        {/if}
      {/if}
      {#if hasEdits}<span class="pb-badge"><span class="pb-badge-dot"></span>Unsaved changes</span>{/if}
      {#if statusNote}<span class="is-note">{statusNote}</span>{/if}
      <span class="pb-spacer"></span>
      {#if onTablePage && tableTab}
        <Pager
          bind:this={pager}
          offset={tableTab.offset}
          limit={tableTab.limit}
          {rowsOnPage}
          count={typeof tableTab.count === "number" ? tableTab.count : null}
          held={running || hasEdits}
          onPage={pageTo}
          onLimit={setPageSize}
        />
      {/if}
    {/if}
  </footer>
</div>

{#if paletteOpen}
  <CommandPalette {commands} onClose={() => (paletteOpen = false)} />
{/if}

{#if treeMenu}
  <TreeMenu
    target={treeMenu.target}
    x={treeMenu.x}
    y={treeMenu.y}
    onClose={() => (treeMenu = null)}
    onOpen={(target) => void openObject(target.object, target.database)}
    onUseDatabase={useDatabase}
    onCopy={copyText}
    onRefresh={() => void refreshObjects()}
  />
{/if}

{#if settingsOpen}
  <SettingsDialog {settings} onSave={saveSettings} onClose={() => (settingsOpen = false)} />
{/if}

{#if connectDialog}
  <ConnectDialog
    engine={connectDialog.engine}
    saved={connectDialog.saved}
    onConnect={connectNetwork}
    onSaved={() => savedKey++}
    onClose={() => (connectDialog = null)}
    onOpenSaved={(saved) => void openSaved(saved)}
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

{#if preview}
  <EditPreview
    statements={preview.statements}
    busy={preview.busy}
    error={preview.error}
    onSave={saveEdits}
    onClose={() => {
      if (!preview?.busy) preview = null;
    }}
  />
{/if}

{#if askPassword}
  {@const target = askPassword}
  <div class="pb-overlay pb-overlay--fixed" role="presentation">
    <form
      class="pb-dialog"
      style="width: 420px"
      aria-labelledby="password-title"
      onsubmit={(event) => {
        event.preventDefault();
        const field = event.currentTarget.elements.namedItem("password");
        void openSaved(target, field instanceof HTMLInputElement ? field.value : "");
      }}
    >
      <div class="pb-dialog-head">
        <h2 class="pb-dialog-title" id="password-title">Password for {target.name}</h2>
        <button type="button" class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Close" onclick={() => (askPassword = null)}><Icon name="x" /></button>
      </div>
      <div class="pb-dialog-body">
        <p class="pb-dialog-text">This connection does not keep its {target.config.target.kind === "http" ? "API token" : "password"}.</p>
        <label class="pb-field">
          <span class="pb-label">{target.config.target.kind === "http" ? "API token" : "Password"}</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="pb-input"
            name="password"
            type="password"
            autofocus
            onkeydown={(event) => {
              if (event.key === "Escape") askPassword = null;
            }}
          />
        </label>
      </div>
      <div class="pb-dialog-foot">
        <span class="pb-spacer"></span>
        <button type="button" class="pb-btn pb-btn--ghost" onclick={() => (askPassword = null)}>Cancel</button>
        <button type="submit" class="pb-btn pb-btn--primary" data-tip-end="" data-tip="Connect  Enter">Connect</button>
      </div>
    </form>
  </div>
{/if}
