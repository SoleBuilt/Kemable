<script lang="ts">
  import type { EditorState } from "@codemirror/state";
  import {
    NO_CONNECTION,
    addRecentFile,
    connectionKey,
    fileName,
    readPages,
    readPagesWithoutConnection,
    readRecentFiles,
    writePages,
    type KeptPage,
  } from "./lib/pages";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { tick, untrack } from "svelte";
  import SqlEditor, { type RunRequest } from "./lib/editor/SqlEditor.svelte";
  import { errorTitle } from "./lib/errors";
  import DataGrid from "./lib/grid/DataGrid.svelte";
  import CellEditor, { type Move } from "./lib/grid/CellEditor.svelte";
  import EditPreview from "./lib/grid/EditPreview.svelte";
  import { nextSortKeys, setCompactRows, type Cell, type CellBox, type EditOverlay, type GridCallbacks, type GridSelection, type GridSource, type SortKey } from "./lib/grid/engine";
  import LivePage from "./lib/live/LivePage.svelte";
  import LiveTools from "./lib/live/LiveTools.svelte";
  import { LiveSession } from "./lib/live/session.svelte";
  import FilterRow, { type Condition } from "./lib/grid/FilterRow.svelte";
  import Pager, { PAGE_SIZES } from "./lib/grid/Pager.svelte";
  import RowDetail from "./lib/grid/RowDetail.svelte";
  import { appWindow, asCoreError, ipc } from "./lib/ipc";
  import { flushStore, readPref, writePref } from "./lib/store";
  import HistoryList from "./lib/sidebar/HistoryList.svelte";
  import KeyBrowser from "./lib/sidebar/KeyBrowser.svelte";
  import ObjectTree, { type TreeScope, type TreeTarget } from "./lib/sidebar/ObjectTree.svelte";
  import QueryList from "./lib/sidebar/QueryList.svelte";
  import SaveQueryDialog from "./lib/sidebar/SaveQueryDialog.svelte";
  import ObjectActionDialog, { type ActionKind } from "./lib/sidebar/ObjectActionDialog.svelte";
  import StructureDialog from "./lib/sidebar/StructureDialog.svelte";
  import ExportDialog from "./lib/data/ExportDialog.svelte";
  import GenerateRowsDialog from "./lib/data/GenerateRowsDialog.svelte";
  import ImportDialog from "./lib/data/ImportDialog.svelte";
  import UnsavedDialog from "./lib/editor/UnsavedDialog.svelte";
  import TreeMenu from "./lib/sidebar/TreeMenu.svelte";
  import DatabasePicker from "./lib/connect/DatabasePicker.svelte";
  import ProfilesDialog from "./lib/connect/ProfilesDialog.svelte";
  import ConnectDialog, { type ConnectRequest, type NetworkEngine, isNetworkEngine } from "./lib/connect/ConnectDialog.svelte";
  import ConnectionMenu from "./lib/connect/ConnectionMenu.svelte";
  import PasswordDialog from "./lib/connect/PasswordDialog.svelte";
  import RunControls from "./lib/editor/RunControls.svelte";
  import KeyBar from "./lib/grid/KeyBar.svelte";
  import MessageList, { type Message } from "./lib/grid/MessageList.svelte";
  import ResultBar, { type ResultTabItem } from "./lib/grid/ResultBar.svelte";
  import TableTools from "./lib/grid/TableTools.svelte";
  import WhereStrip from "./lib/grid/WhereStrip.svelte";
  import PageTabs, { type PageTabItem } from "./lib/shell/PageTabs.svelte";
  import PaletteTrigger from "./lib/shell/PaletteTrigger.svelte";
  import SidePane from "./lib/shell/SidePane.svelte";
  import StatusBar from "./lib/shell/StatusBar.svelte";
  import TitleBar from "./lib/shell/TitleBar.svelte";
  import TermsDialog, { TERMS_VERSION } from "./lib/shell/TermsDialog.svelte";
  import Welcome from "./lib/shell/Welcome.svelte";
  import Badge from "./lib/ui/Badge.svelte";
  import Button from "./lib/ui/Button.svelte";
  import CommandPalette, { type Command } from "./lib/ui/CommandPalette.svelte";
  import Icon from "./lib/ui/Icon.svelte";
  import InfoBar from "./lib/ui/InfoBar.svelte";
  import SettingsDialog, {
    applyMica,
    applyStyle,
    applyTheme,
    applyZoom,
    readSettings,
    writeSettings,
    zoomPercent,
    type Settings,
  } from "./lib/ui/SettingsDialog.svelte";
  import Splitter from "./lib/ui/Splitter.svelte";
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
    ResultFilter,
    ResultWhere,
    RowSpan,
    RunEvent,
    SavedConnection,
    SavedQuery,
    TableRef,
    ResultSource,
    LineEnding,
    TextEncoding,
    TextFile,
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

  // The terms are asked once a version of them (lib/shell/TermsDialog.svelte).
  let termsNeeded = $state(readPref("termsAccepted") !== TERMS_VERSION);

  function readPageSize(): number {
    const value = Number(readPref("pageSize"));
    return PAGE_SIZES.includes(value) ? value : 1_000;
  }

  interface ResultSetView {
    result: number;
    statement: number;
    /** Where in its statement (a SQL Server batch) the set came from: UTF-8 bytes into the statement. */
    part: ByteRange | null;
    /** The one table its rows come from, for "Open <table> to edit"; undefined until asked. */
    source: ResultSource | null | undefined;
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
    /** The filter on the rows held (docs/decisions/0011-filtering-a-result.md); `totalRows` is then the rows shown. */
    filter: ResultFilterView | null;
  }

  interface ResultFilterView {
    conditions: Condition[];
    matchAny: boolean;
    /** The rows the result holds; the filter shows some of them. */
    held: number;
    /** The filter as a WHERE to copy, when the engine writes SQL. */
    where: ResultWhere | null;
  }

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
    selected: GridSelection | null;
    /** A query page shows its results only once something ran in it. */
    ran: boolean;
    /** What the last run sent from the editor, to frame the statement of the result shown. */
    ranRequest: RunRequest | null;
    ranRanges: ByteRange[];
  }

  interface Page {
    id: number;
    kind: "query" | "table" | "live";
    /** A query or Live page's number, for its title ("Query 2", "Live 2"). */
    number: number;
    /** A Live page's listening, which goes on while another page is shown (docs/decisions/0020-live-tab.md). */
    live?: LiveSession;
    /** A query page's editor, kept while another page is shown. */
    editor: EditorState | null;
    state: PageState;
    /** The `.sql` file a query page is (docs/decisions/0016-sql-files.md); null for "Query n". */
    file: PageFile | null;
  }

  interface PageFile {
    path: string;
    encoding: TextEncoding;
    lineEnding: LineEnding;
    /** The text as last read from or written to the file: the page is unsaved while it differs. */
    saved: string;
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
  let pages: Page[] = $state.raw([{ id: 1, kind: "query", number: 1, editor: null, state: emptyState(), file: null }]);
  let activePage = $state(1);
  /** Query pages are open with no connection: written first, run once one opens (ADR 0016). */
  let drafting = $state(false);
  const currentPage = $derived(pages.find((page) => page.id === activePage));
  const onTablePage = $derived(currentPage?.kind === "table");
  const onLivePage = $derived(currentPage?.kind === "live");
  /** The page shown has no editor: what goes to the editor opens a query page first. */
  const offQuery = $derived(onTablePage || onLivePage);
  const liveSession = $derived(currentPage?.live ?? null);
  let livePage: LivePage | undefined = $state();

  // --- The connection and the page shown ---------------------------------

  const startSettings = readSettings();
  let settings: Settings = $state(startSettings);
  applyTheme(startSettings.theme);
  applyStyle(startSettings.style);
  if (startSettings.mica) void applyMica(true, startSettings.theme);
  if (startSettings.zoomLevel !== 0) applyZoom(startSettings.zoomLevel);
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

  /** A size the user dragged, as kept, when it is a number in `min`..`max`. */
  function keptSize(name: string, min: number, max: number, fallback: number): number {
    const kept = readPref(name);
    return typeof kept === "number" && kept >= min && kept <= max ? kept : fallback;
  }
  let editorHeight = $state(keptSize("editorHeight", 72, 4000, 188));
  let sidebarWidth = $state(keptSize("sidebarWidth", 180, 480, 260));
  // The splitters' sizes are kept as they settle; the first run of each effect keeps nothing.
  $effect(() => {
    if (editorHeight !== readPref("editorHeight")) writePref("editorHeight", editorHeight);
  });
  $effect(() => {
    if (sidebarWidth !== readPref("sidebarWidth")) writePref("sidebarWidth", sidebarWidth);
  });
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
  let selected = $state<GridSelection | null>(null);
  /** How many rows the selection holds, across every span. */
  const selectedRows = $derived(selected ? selected.rows.reduce((total, span) => total + span.rowCount, 0) : 0);
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
    keepPagesSoon();
    return true;
  }

  function newQueryPage() {
    if (running) {
      note("Wait for the run to end, or stop it, before you open a page");
      return;
    }
    const number = Math.max(0, ...pages.filter((page) => page.kind === "query").map((page) => page.number)) + 1;
    const page: Page = { id: nextPageId++, kind: "query", number, editor: null, state: emptyState(), file: null };
    pages = [...keepCurrent(), page];
    activePage = page.id;
    unstash(page.state);
    editor?.restoreState(null);
    void tick().then(() => editor?.focus());
    keepPagesSoon();
  }

  /** Ctrl+Alt+L: a Live page, for an engine that pushes changes (docs/decisions/0020-live-tab.md). */
  function newLivePage() {
    if (!connection || connection.engine.liveTopics.length === 0) return;
    if (running) {
      note("Wait for the run to end, or stop it, before you open a page");
      return;
    }
    const number = Math.max(0, ...pages.filter((page) => page.kind === "live").map((page) => page.number)) + 1;
    const page: Page = { id: nextPageId++, kind: "live", number, editor: null, state: emptyState(), file: null, live: new LiveSession() };
    pages = [...keepCurrent(), page];
    activePage = page.id;
    unstash(page.state);
    void tick().then(() => livePage?.focusTopics());
  }

  /** F5 on a Live page: listens in the database the toolbar shows. */
  function listenLive() {
    if (connection && liveSession) void liveSession.start(connection.id, database);
  }

  function releaseState(state: PageState) {
    for (const set of state.sets) void ipc.releaseResult(set.result);
  }

  /** Closes page `id`; the last query page closing leaves a fresh empty one, so there is always an editor. */
  function closePage(id: number, unsaved: "ask" | "drop" = "ask") {
    const page = pages.find((candidate) => candidate.id === id);
    if (!page) return;
    if (unsaved === "ask" && page.file && pageDirty(page)) {
      closing = { id, name: fileName(page.file.path) };
      return;
    }
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
    page.live?.close();
    const index = pages.findIndex((candidate) => candidate.id === id);
    let rest = (shown ? pages : keepCurrent()).filter((candidate) => candidate.id !== id);
    const emptied = !rest.some((candidate) => candidate.kind === "query");
    // With no connection, closing the last page goes back to the Welcome screen.
    if (emptied && !connection) drafting = false;
    if (emptied) {
      rest = [
        ...rest,
        { id: nextPageId++, kind: "query", number: page.kind === "query" ? page.number + 1 : 1, editor: null, state: emptyState(), file: null },
      ];
    }
    pages = rest;
    keepPagesSoon();
    if (!shown) return;
    const next = rest[Math.min(index, rest.length - 1)] ?? rest[0];
    if (!next) return;
    activePage = next.id;
    unstash(next.state);
    if (next.kind === "query") editor?.restoreState(next.editor);
  }

  // --- Pages across launches (src/lib/pages.ts) ---------------------------

  /** The connection whose query pages are kept; null with none open. */
  let pagesKey: string | null = null;
  let keepTimer: ReturnType<typeof setTimeout> | undefined;

  /** Keeps the query pages of the connection open now: their numbers, text and cursor. */
  function keepPages() {
    clearTimeout(keepTimer);
    keepTimer = undefined;
    if (!pagesKey) return;
    const kept: KeptPage[] = [];
    let active = 0;
    for (const page of pages) {
      if (page.kind !== "query") continue;
      const state = page.id === activePage ? (editor?.saveState() ?? page.editor) : page.editor;
      if (page.id === activePage) active = kept.length;
      const text = state?.doc.toString() ?? "";
      const file = page.file
        ? { path: page.file.path, encoding: page.file.encoding, lineEnding: page.file.lineEnding, dirty: text !== page.file.saved }
        : undefined;
      kept.push({ number: page.number, text, cursor: state?.selection.main.head ?? 0, file });
    }
    writePages(pagesKey, { pages: kept, active, at: Date.now() });
  }

  /** Keeps the pages a moment after typing stops, so a keystroke never waits on storage. */
  function keepPagesSoon() {
    if (!pagesKey) return;
    clearTimeout(keepTimer);
    keepTimer = setTimeout(keepPages, 400);
  }

  /**
   * Brings back the query pages kept for `key`, in place of the empty one a
   * connection opens with, and adds `carried` (the files opened before it).
   * A file's page reads its file again: a page kept unsaved keeps its text.
   */
  async function restorePages(key: string, carried: Page[] = []) {
    pagesKey = key;
    const kept = await readPages(key);
    if ((!kept && carried.length === 0) || !editor) return;
    const view = editor;
    const restored: Page[] = await Promise.all(
      (kept?.pages ?? []).map(async (page): Promise<Page> => {
        let text = page.text;
        let file: PageFile | null = null;
        if (page.file) {
          const disk = await ipc.readTextFile(page.file.path).catch(() => null);
          // A file gone or unreadable keeps the page's text, unsaved, so a save writes it again.
          file = { path: page.file.path, encoding: disk?.encoding ?? page.file.encoding, lineEnding: disk?.lineEnding ?? page.file.lineEnding, saved: disk?.text ?? "" };
          if (disk && !page.file.dirty) text = disk.text;
        }
        return { id: nextPageId++, kind: "query", number: page.number, editor: view.stateWith(text, page.cursor), state: emptyState(), file };
      }),
    );
    // A carried file already kept for this connection is not opened twice; a carried query
    // takes the numbers after the connection's own, so two pages are never both "Query 1".
    let number = Math.max(0, ...restored.map((page) => page.number));
    const brought = carried
      .filter((page) => !page.file || !restored.some((other) => other.file?.path === page.file?.path))
      .map((page) => (page.file ? page : { ...page, number: ++number }));
    const all = [...restored, ...brought];
    const shown = brought.length > 0 ? all[all.length - 1] : (all[kept?.active ?? 0] ?? all[0]);
    if (!shown) return;
    pages = all;
    activePage = shown.id;
    unstash(emptyState());
    view.restoreState(shown.editor);
    keepPagesSoon();
  }

  // Files opened and queries written with no connection come back at launch
  // (ADR 0016), once the work area's editor mounts to read them.
  const keptWithout = readPagesWithoutConnection();
  if (keptWithout?.pages.some((page) => page.file || page.text.trim() !== "")) {
    drafting = true;
    void tick().then(() => restorePages(NO_CONNECTION));
  }

  // However the window closes (its button, Alt+F4, the taskbar), the last keystrokes are
  // kept and everything not on disk yet is written before it goes.
  void appWindow.onCloseRequested(async () => {
    keepPages();
    await flushStore();
  });

  /** Every page's result sets go, and one empty query page stays; the editor keeps its text. */
  function resetPages() {
    for (const page of pages) if (page.id !== activePage) releaseState(page.state);
    for (const page of pages) page.live?.close();
    clearResults();
    const kept = currentPage?.kind === "query" ? currentPage : null;
    pages = [{ id: nextPageId++, kind: "query", number: 1, editor: null, state: emptyState(), file: null }];
    activePage = pages[0]?.id ?? 1;
    unstash(emptyState());
    if (!kept) editor?.restoreState(null);
  }

  function pageTitle(page: Page): string {
    if (page.file) return fileName(page.file.path);
    if (page.kind === "query") return `Query ${page.number}`;
    if (page.kind === "live") return page.number === 1 ? "Live" : `Live ${page.number}`;
    const tab = page.id === activePage ? tableTab : page.state.tableTab;
    return tab?.table.name ?? "Table";
  }

  function pageCount(page: Page): string {
    if (page.live) return page.live.total > 0 ? numberFormat.format(page.live.total) : "";
    const tab = page.id === activePage ? tableTab : page.state.tableTab;
    return typeof tab?.count === "number" ? numberFormat.format(tab.count) : "";
  }

  const gridCallbacks: GridCallbacks = {
    fetchWindow: ipc.fetchWindow,
    copy: (result, selection, withHeaders) => {
      const rows = selection.rows.reduce((total, span) => total + span.rowCount, 0);
      ipc
        .copyRange(result, selection.rows, selection.firstColumn, selection.columnCount, withHeaders)
        .then((text) => navigator.clipboard.writeText(text))
        .then(
          () => note(`Copied ${numberFormat.format(rows * selection.columnCount)} cells${withHeaders ? " with column names" : ""}`),
          (caught: unknown) => note(`Not copied: ${asCoreError(caught).message}`),
        );
    },
    selectionChanged: (result, focus, selection) => {
      cursor = focus ? { result, row: focus.row, column: focus.column } : null;
      selected = selection;
    },
    sortRequested: (column, additive) => void sortBy(column, additive),
    editRequested: (cell, typed) => void beginEdit(cell, typed),
    deleteRequested: (rows) => markDeleted(rows),
  };

  function anyEdits(): boolean {
    if (hasEdits) return true;
    return pages.some((page) => page.id !== activePage && (page.state.edits.cells.size > 0 || page.state.edits.deleted.size > 0 || page.state.edits.added > 0));
  }

  /** Opens a connection in place of the current one. Resolves to the error, or null. */
  async function openWith(opening: () => Promise<OpenedConnection>, filePath: string | null = null): Promise<CoreError | null> {
    if (anyEdits()) return { code: "config.invalid", message: "Save or discard the table's edits first (Ctrl+S)" };
    connecting = true;
    try {
      const opened = await opening();
      if (connection) void ipc.disconnect(connection.id);
      // Files opened and queries written with no connection go to the one that opens now (ADR 0016).
      const carried = connection ? [] : keepCurrent().filter((page) => page.kind === "query" && (page.file || pageText(page).trim() !== ""));
      drafting = false;
      keepPages();
      if (carried.length > 0) writePages(NO_CONNECTION, { pages: [], active: 0, at: Date.now() });
      pagesKey = null;
      resetPages();
      connection = opened;
      database = opened.database;
      await tick();
      await restorePages(connectionKey(opened, filePath), carried);
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
    connectError = await openWith(() => ipc.connect(config), path);
  }

  function disconnect() {
    if (!connection || running) return;
    if (anyEdits()) {
      note("Save or discard the table's edits first (Ctrl+S)");
      return;
    }
    void ipc.disconnect(connection.id);
    keepPages();
    pagesKey = null;
    drafting = false;
    resetPages();
    connection = null;
    database = null;
  }

  /** The connect dialog: the engine it is for, and the saved connection it edits. */
  let connectDialog: { engine: NetworkEngine; saved: SavedConnection | null } | null = $state(null);
  /** Credential profiles, from the palette or over the connect form; `initial` is the profile shown first. */
  let profilesDialog: { initial: string | null } | null = $state(null);
  /** Goes up when profiles change, so the connect form reads them again. */
  let profilesKey = $state(0);
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
      // Kept: it connects first and is saved only once it has; the page names it as saved.
      const opened = request.keep
        ? await ipc.connectAndSave(request.keep.connection, request.keep.password)
        : await ipc.connect(request.config, request.profileId);
      return { ...opened, title: request.title };
    });
    if (!error) connectDialog = null;
    return error;
  }

  /** A saved connection whose password is not kept asks for it first. */
  let askPassword: SavedConnection | null = $state(null);

  function needsPassword(saved: SavedConnection): boolean {
    const target = saved.config.target;
    if (saved.savePassword || saved.profileId) return false;
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

  /** The extensions Open file… reads as a script; anything else opens as a SQLite database. */
  const SCRIPT_EXTENSIONS = ["sql", "txt"];
  const SQLITE_EXTENSIONS = ["db", "sqlite", "sqlite3", "db3", "s3db"];

  /** Ctrl+O: a `.sql` file opens on a query page, a SQLite file as a connection (ADR 0016). */
  async function openFile() {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "SQL scripts and SQLite databases", extensions: [...SCRIPT_EXTENSIONS, ...SQLITE_EXTENSIONS] },
        { name: "SQL scripts", extensions: SCRIPT_EXTENSIONS },
        { name: "SQLite", extensions: SQLITE_EXTENSIONS },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof path !== "string") return;
    const extension = path.split(".").pop()?.toLowerCase() ?? "";
    if (SCRIPT_EXTENSIONS.includes(extension)) await openScript(path);
    else await connectTo(path);
  }

  // --- .sql files (docs/decisions/0016-sql-files.md) --------------------------

  let recentFiles: string[] = $state(readRecentFiles());

  /** Ctrl+T and New query on the Welcome screen: an empty query page, with or without a connection. */
  async function startQuery() {
    if (workspace) {
      newQueryPage();
      return;
    }
    drafting = true;
    pagesKey ??= NO_CONNECTION;
    await tick();
    editor?.focus();
  }
  /** Goes up as the editor's text changes, so the tabs read whether a file is saved. */
  let editTick = $state(0);
  /** A file's page asked to close while unsaved. */
  let closing: { id: number; name: string } | null = $state(null);

  /** The text a page has now: the editor's for the page shown. */
  function pageText(page: Page): string {
    const state = page.id === activePage ? (editor?.saveState() ?? page.editor) : page.editor;
    return state?.doc.toString() ?? "";
  }

  function pageDirty(page: Page): boolean {
    return page.file !== null && pageText(page) !== page.file.saved;
  }

  function onEditorChange() {
    editTick++;
    keepPagesSoon();
  }

  /** Opens `path` on a query page, or shows the page that already has it. */
  async function openScript(path: string) {
    const same = (other: string | undefined) => other?.toLowerCase() === path.toLowerCase();
    const existing = pages.find((page) => same(page.file?.path));
    if (existing) {
      switchTo(existing.id);
      return;
    }
    if (running) {
      note("Wait for the run to end, or stop it, before you open a page");
      return;
    }
    let read: TextFile;
    try {
      read = await ipc.readTextFile(path);
    } catch (caught) {
      const error = asCoreError(caught);
      note(`${errorTitle(error)}: ${error.message}`);
      return;
    }
    const current = keepCurrent();
    // An empty, untouched "Query n" gives its place to the file, as an editor's new tab does.
    const blank = current.length === 1 && current[0]?.kind === "query" && !current[0].file && pageText(current[0]) === "";
    const number = Math.max(0, ...current.filter((page) => page.kind === "query").map((page) => page.number)) + 1;
    const page: Page = {
      id: nextPageId++,
      kind: "query",
      number,
      editor: null,
      state: emptyState(),
      file: { path, encoding: read.encoding, lineEnding: read.lineEnding, saved: read.text },
    };
    pages = blank ? [page] : [...current, page];
    activePage = page.id;
    unstash(emptyState());
    // With no connection the work area mounts with the first file: wait for its editor.
    await tick();
    if (!editor) return;
    const state = editor.stateWith(read.text, 0);
    pages = pages.map((other) => (other.id === page.id ? { ...other, editor: state } : other));
    editor.restoreState(state);
    pagesKey ??= connection ? null : NO_CONNECTION;
    recentFiles = addRecentFile(path);
    keepPagesSoon();
    void tick().then(() => editor?.focus());
  }

  /** Ctrl+S on a query page: writes its file; a page with none asks where (Save As). */
  async function saveScript(asNew = false) {
    const page = currentPage;
    if (!page || page.kind !== "query") return;
    const text = pageText(page);
    let target = page.file;
    if (!target || asNew) {
      const path = await save({
        defaultPath: page.file?.path ?? `${pageTitle(page)}.sql`,
        filters: [{ name: "SQL scripts", extensions: ["sql"] }],
      });
      if (typeof path !== "string") return;
      // A new file is UTF-8 with CRLF; Save As keeps the encoding of the file it came from.
      target = { path, encoding: page.file?.encoding ?? "utf8", lineEnding: page.file?.lineEnding ?? "crlf", saved: "" };
    }
    try {
      await ipc.writeTextFile(target.path, text, target.encoding, target.lineEnding);
    } catch (caught) {
      const error = asCoreError(caught);
      note(`Not saved: ${errorTitle(error)}: ${error.message}`);
      return false;
    }
    const file: PageFile = { ...target, saved: text };
    pages = pages.map((other) => (other.id === page.id ? { ...other, file } : other));
    pagesKey ??= connection ? null : NO_CONNECTION;
    recentFiles = addRecentFile(file.path);
    keepPagesSoon();
    note(`Saved ${fileName(file.path)}`);
    return true;
  }

  /** The answer to "Save changes to <file>?" as its page closes. */
  async function answerClosing(answer: "save" | "drop" | "cancel") {
    const target = closing;
    closing = null;
    if (!target || answer === "cancel") return;
    if (answer === "save") {
      if (!switchTo(target.id)) return;
      if (!(await saveScript())) return;
    }
    closePage(target.id, "drop");
  }


  function clearResults() {
    for (const set of sets) void ipc.releaseResult(set.result);
    sets = [];
    messages = [];
    activeTab = 0;
    elapsedMs = null;
  }

  /** Ctrl+=, Ctrl+- and Ctrl+0, as in VS Code: the whole window, kept across launches. */
  function zoomBy(step: -1 | 0 | 1) {
    const level = applyZoom(step === 0 ? 0 : settings.zoomLevel + step);
    if (level !== settings.zoomLevel) {
      settings = { ...settings, zoomLevel: level };
      writeSettings(settings);
    }
    note(`Zoom ${zoomPercent(level)} %`);
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
    applyStyle(next.style);
    void applyMica(next.mica, next.theme);
    applyZoom(next.zoomLevel);
    setCompactRows(next.compactRows);
    settingsOpen = false;
    if (density) grid?.reload();
  }

  /** Each result tab's filter as the run began, for the set that takes its place. */
  let carriedFilters: ({ conditions: Condition[]; matchAny: boolean } | null)[] = [];

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
    if (!connection && !running) {
      note("Open a connection to run this script: pick one in Ctrl+K");
      openPalette();
      return;
    }
    if (!connection || running) return;
    if (hasEdits) {
      note("Save or discard the table's edits first (Ctrl+S)");
      return;
    }
    pageRun = overrides.tablePage === true;
    if (!pageRun && offQuery) return;
    carriedFilters = pageRun || keyView ? [] : sets.map((set) => (set.filter ? { conditions: set.filter.conditions, matchAny: set.filter.matchAny } : null));
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
    const keys = nextSortKeys(set.sort, column, additive);

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
      case "setStarted": {
        const carried = carriedFilters[sets.length];
        if (carried) void filterSet(event.result, carried.conditions, carried.matchAny);
        if (pageRun && tableTab) {
          const tab = tableTab;
          const sort = tab.sort.flatMap((key) => {
            const column = event.columns.findIndex((meta) => meta.name === key.column);
            return column < 0 ? [] : [{ column, descending: key.descending }];
          });
          if (tab.columns.length === 0) tableTab = { ...tab, columns: event.columns.map((meta) => meta.name) };
          pendingSort = sort.length > 0 ? { keys: sort, original: null } : null;
        }
        // A sort that ran a statement again makes one set: only that one takes the sort.
        const sorted = sets.length === 0 ? pendingSort : null;
        sets = [
          ...sets,
          {
            result: event.result,
            statement: event.statement,
            part: event.part,
            source: undefined,
            columns: event.columns,
            totalRows: 0,
            finished: false,
            limited: false,
            sort: sorted?.keys ?? [],
            sortedBy: sorted ? "server" : null,
            original: sorted?.original ?? null,
            filter: null,
          },
        ];
        break;
      }
      case "rows":
        if (sets.find((set) => set.result === event.result)?.filter) refreshCounts(event.result);
        else updateSet(event.result, { totalRows: event.total });
        break;
      case "setFinished":
        if (sets.find((set) => set.result === event.result)?.filter) {
          updateSet(event.result, { finished: true, limited: event.limited });
          refreshCounts(event.result);
        } else {
          updateSet(event.result, { totalRows: event.total, finished: true, limited: event.limited });
        }
        break;
      case "partsDropped":
        sets = sets.map((set) => (set.statement === event.statement ? { ...set, part: null } : set));
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

  /** The text a set came from: its statement, or the part of a SQL Server batch that made it. */
  function framedRange(set: ResultSetView, ranges: ByteRange[]): ByteRange | undefined {
    const range = ranges[set.statement];
    if (!range || !set.part) return range;
    return { start: range.start + set.part.start, end: range.start + set.part.end };
  }

  // With two results or more, the text behind the result shown is framed in the editor (D23),
  // when the results came from different text: a batch the core could not read frames nothing.
  $effect(() => {
    const set = activeSet;
    const request = ranRequest;
    const ranges = ranRanges;
    const all = sets;
    untrack(() => {
      const many = new Set(all.map((other) => JSON.stringify(framedRange(other, ranges) ?? null))).size > 1;
      const range = set && many && !onTablePage ? framedRange(set, ranges) : undefined;
      editor?.frame(range ? request : null, range ?? null);
    });
  });

  /** Opens a table or view from the tree on a page of its own; `newTab` always opens another page. */
  async function openObject(
    object: ObjectInfo,
    inDatabase: string | null,
    newTab = false,
    where: { filters: Filter[]; matchAny: boolean } | null = null,
  ) {
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
        if (!switchTo(existing.id)) return;
        // A result's WHERE replaces the filters of the table's page.
        if (where && tableTab) {
          const filters = filtersFor(where.filters, tableTab.columns);
          if (filters) {
            filtersOpen = true;
            void loadPage({ ...tableTab, filters, matchAny: where.matchAny, offset: 0 }, true);
          } else note("Opened the whole table: the statement's WHERE names a column the table does not have");
        }
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
    const names = columns.map((column) => column.name);
    const brought = where ? filtersFor(where.filters, names) : [];
    if (where && !brought) note("Opened the whole table: the statement's WHERE names a column the table does not have");
    const next: TableTab = {
      database: dbName,
      table,
      title,
      columns: names,
      key,
      filters: brought ?? [],
      matchAny: brought && where ? where.matchAny : false,
      sort: [],
      offset: 0,
      limit: pageSize,
      count: null,
      pageSql: "",
      countSql: "",
    };
    const page: Page = { id: nextPageId++, kind: "table", number: 0, editor: null, state: { ...emptyState(), tableTab: next }, file: null };
    pages = [...keepCurrent(), page];
    activePage = page.id;
    unstash(page.state);
    if (brought && brought.length > 0) filtersOpen = true;
    await loadPage(next, true);
  }

  /**
   * A result's filters on the table's own column names, matched without case
   * (the statement may write `city` for `City`); null when one names no column.
   * A table whose columns could not be read keeps the names as written.
   */
  function filtersFor(filters: Filter[], columns: string[]): Filter[] | null {
    if (columns.length === 0) return filters;
    const named: Filter[] = [];
    for (const filter of filters) {
      const column = columns.find((name) => name.toLowerCase() === filter.column.toLowerCase());
      if (!column) return null;
      named.push({ ...filter, column });
    }
    return named;
  }

  /** "Open <table> to edit": the table a query result comes from, on its own page, filtered by the result's WHERE. */
  async function openResultTable(source: ResultSource) {
    await openObject({ schema: source.table.schema, name: source.table.name, kind: "table" }, source.database, false, {
      filters: source.filters,
      matchAny: source.matchAny,
    });
    if (source.whereLeft) note("Opened the whole table: the statement's WHERE is more than the filter row holds");
  }

  // A finished result is asked once which table it comes from.
  $effect(() => {
    const set = activeSet;
    if (!set || !set.finished || set.source !== undefined || keyValue) return;
    const result = set.result;
    untrack(() => {
      updateSet(result, { source: null });
      ipc.resultSource(result).then(
        (source) => updateSet(result, { source }),
        () => updateSet(result, { source: null }),
      );
    });
  });

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

  /** Filters the rows result `result` holds, in the core; the statement does not run again. */
  async function filterSet(result: number, conditions: Condition[], matchAny: boolean) {
    const open = connection;
    if (!open) return;
    const filters: ResultFilter[] = conditions.flatMap((condition) =>
      condition.op === "raw" ? [] : [{ column: condition.column, op: condition.op, value: condition.value, second: condition.second }],
    );
    try {
      const counts = await ipc.filterResult(result, filters, matchAny);
      let where: ResultWhere | null = null;
      if (filters.length > 0 && open.engine.sqlGrammar) {
        where = await ipc.resultWhere(open.id, result, filters, matchAny).catch(() => null);
      }
      updateSet(result, {
        totalRows: counts.shownRows,
        filter: counts.filtered ? { conditions, matchAny, held: counts.totalRows, where } : null,
      });
      if (activeResult === result) grid?.reload();
    } catch (caught) {
      note(`Not filtered: ${asCoreError(caught).message}`);
    }
  }

  function applyResultFilter(conditions: Condition[], matchAny: boolean) {
    const set = activeSet;
    if (set && !running) void filterSet(set.result, conditions, matchAny);
  }

  /** A filtered set streaming in: run events count the rows held, so the rows shown are asked for. */
  function refreshCounts(result: number) {
    ipc.resultCounts(result).then(
      (counts) => {
        const set = sets.find((candidate) => candidate.result === result);
        if (set?.filter) updateSet(result, { totalRows: counts.shownRows, filter: { ...set.filter, held: counts.totalRows } });
      },
      () => {},
    );
  }

  function copyWhere() {
    const where = activeSet?.filter?.where;
    if (where?.sql) copyText(where.sql, "the WHERE");
  }

  /** Reads a key into the results, up to the row cap unless `unlimited`. */
  async function openKey(name: string, unlimited = false) {
    if (!connection || running) return;
    // A key shows under an editor: from a Live page, go to a query page first (there always is one).
    const queryPage = onLivePage ? pages.find((page) => page.kind === "query") : undefined;
    if (queryPage) switchTo(queryPage.id);
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
            part: null,
            source: null,
            columns: read.columns,
            totalRows: read.totalRows,
            finished: true,
            limited: read.limited,
            sort: [],
            sortedBy: null,
            original: null,
            filter: null,
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
    if (!set || !selected) return false;
    let any = false;
    for (const span of selected.rows) {
      const last = Math.min(span.firstRow + span.rowCount, set.totalRows);
      for (let row = span.firstRow; row < last; row++) {
        if (!edits.deleted.has(row)) return false;
        any = true;
      }
    }
    return any;
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
  /** Marks the rows of `spans` for delete, or unmarks them when every one is marked; new rows go at once. */
  function markDeleted(spans: RowSpan[]) {
    const set = pageSet;
    if (!canEdit || !set) return cannotEdit();
    const base = set.totalRows;
    const rows = spans.flatMap((span) => Array.from({ length: span.rowCount }, (_, offset) => span.firstRow + offset));
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
    writePref("pageSize", limit);
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
    if (offQuery) newQueryPage();
    const here = entry.connection === connection.title && connection.engine.hasDatabases;
    if (here && entry.database) database = entry.database;
    editor?.insert(entry.statement);
    void startRun({ sql: entry.statement, cursor: null, origin: 0 }, false, { record: true });
  }

  function runSaved(query: SavedQuery) {
    if (!connection || running) return;
    if (offQuery) newQueryPage();
    editor?.insert(query.sql);
    void startRun({ sql: query.sql, cursor: null, origin: 0 }, false, { record: true });
  }

  function insertSql(sql: string) {
    if (offQuery) newQueryPage();
    editor?.insert(sql);
  }

  /** Ctrl+Shift+D: keeps the editor's text, or its selection, as a named query. */
  function saveFromEditor() {
    if (offQuery) return;
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

  // --- The tree shows every database, or only the one in use ---
  let treeScope: TreeScope = $state(readPref("treeScope") === "current" ? "current" : "all");
  function toggleTreeScope() {
    treeScope = treeScope === "all" ? "current" : "all";
    writePref("treeScope", treeScope);
  }

  // --- The tree menu's actions (docs/decisions/0017-tree-actions.md) ---------

  type ObjectTarget = TreeTarget & { kind: "object" };
  /** Rename, empty, drop, maintain or comment: shown with its statements before they run. */
  let objectAction: { target: ObjectTarget; kind: ActionKind } | null = $state(null);
  /** Show Structure's dialog. */
  let structureOf: ObjectTarget | null = $state(null);

  /** Opens `text` on a new query page, to read, change and run. */
  async function openTextOnPage(text: string) {
    if (running) {
      note("Wait for the run to end, or stop it, before you open a page");
      return;
    }
    newQueryPage();
    await tick();
    if (!editor) return;
    editor.restoreState(editor.stateWith(text, 0));
    keepPagesSoon();
  }

  async function readDdl(target: ObjectTarget): Promise<string | null> {
    if (!connection) return null;
    try {
      return await ipc.objectDdl(connection.id, target.database, target.object);
    } catch (caught) {
      const error = asCoreError(caught);
      note(`${errorTitle(error)}: ${error.message}`);
      return null;
    }
  }

  async function showDdl(target: ObjectTarget) {
    const ddl = await readDdl(target);
    if (ddl !== null) await openTextOnPage(ddl);
  }

  async function copyDdl(target: ObjectTarget) {
    const ddl = await readDdl(target);
    if (ddl !== null) copyText(ddl, "the DDL");
  }

  /** Edit View Definition: the statement that replaces the view, on a query page, to change and run. */
  async function editView(target: ObjectTarget) {
    if (!connection) return;
    try {
      await openTextOnPage(await ipc.viewDefinition(connection.id, target.database, target.object));
    } catch (caught) {
      const error = asCoreError(caught);
      note(`${errorTitle(error)}: ${error.message}`);
    }
  }

  /** Export, Import or Generate Rows on one table (docs/decisions/0018-export-import-generate.md). */
  let dataDialog: {
    kind: "export" | "import" | "generate";
    database: string | null;
    table: TableRef;
    keyColumns: string[];
    filtered: { filters: Filter[]; matchAny: boolean } | null;
  } | null = $state(null);

  /** Opens a data dialog for `table`; an open page of it lends its key and its filter. */
  function openData(kind: "export" | "import" | "generate", table: TableRef, inDatabase: string | null) {
    const tab = (page: Page) => (page.id === activePage ? tableTab : page.state.tableTab);
    const shown = pages.map(tab).find((open) => open && open.table.name === table.name && open.table.schema === table.schema && open.database === inDatabase);
    dataDialog = {
      kind,
      database: inDatabase,
      table,
      keyColumns: shown?.key ?? [],
      filtered: shown && shown.filters.length > 0 ? { filters: shown.filters, matchAny: shown.matchAny } : null,
    };
  }

  function dataDone(message: string) {
    dataDialog = null;
    note(message);
    void refreshObjects();
    // A page of the table shows its new rows.
    if (tableTab) void loadPage(tableTab, true);
  }

  async function copyQualified(target: ObjectTarget) {
    if (!connection) return;
    try {
      copyText(await ipc.qualifiedName(connection.id, target.object), "the qualified name");
    } catch (caught) {
      note(`Not copied: ${asCoreError(caught).message}`);
    }
  }

  function actionDone(message: string) {
    objectAction = null;
    note(message);
    void refreshObjects();
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
    ...(connection && connection.engine.liveTopics.length > 0
      ? [
          {
            id: "new-live",
            group: "Start",
            label: "New Live tab",
            meta: "Listen to channels as events arrive",
            icon: "radio" as const,
            keys: ["Ctrl", "Alt", "L"],
            run: newLivePage,
          },
        ]
      : []),
    { id: "new-connection", group: "Start", label: "New connection…", icon: "plus", keys: ["Ctrl", "N"], run: newConnection },
    { id: "open-file", group: "Start", label: "Open file…", icon: "file", keys: ["Ctrl", "O"], run: () => void openFile() },
    { id: "memory", group: "Start", label: "Open an in-memory database", icon: "database", run: () => void connectTo(":memory:") },
    ...library.map((entry) => ({
      id: `saved:${entry.id}`,
      group: "Saved connections",
      label: entry.name,
      meta: savedWhere(entry),
      dot: entry.color ?? "gray",
      run: () => void openSaved(entry),
    })),
    ...recentFiles.map((path) => ({
      id: `file:${path}`,
      group: "Recent files",
      label: fileName(path),
      meta: path,
      icon: "file" as const,
      run: () => void openScript(path),
    })),
    { id: "settings", group: "Settings", label: "Settings…", icon: "settings", keys: ["Ctrl", ","], run: () => (settingsOpen = true) },
    {
      id: "profiles",
      group: "Settings",
      label: "Credential profiles…",
      meta: "A user and password for many connections",
      icon: "key",
      run: () => (profilesDialog = { initial: null }),
    },
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
    } else if (ctrl && !event.altKey && (key === "=" || key === "+" || event.code === "NumpadAdd")) {
      // Ctrl+= is Ctrl++ without Shift, on every keyboard layout VS Code knows.
      event.preventDefault();
      zoomBy(1);
    } else if (ctrl && !event.altKey && (key === "-" || event.code === "NumpadSubtract")) {
      event.preventDefault();
      zoomBy(-1);
    } else if (ctrl && !event.altKey && (key === "0" || event.code === "Numpad0")) {
      event.preventDefault();
      zoomBy(0);
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
    } else if (ctrl && key === "t" && !event.shiftKey) {
      event.preventDefault();
      void startQuery();
    } else if (ctrl && key === "w" && workspace) {
      event.preventDefault();
      closePage(activePage);
    } else if (ctrl && event.key === "Tab" && workspace && pages.length > 1) {
      event.preventDefault();
      const index = pages.findIndex((page) => page.id === activePage);
      const next = pages[(index + (event.shiftKey ? -1 : 1) + pages.length) % pages.length];
      if (next) switchTo(next.id);
    } else if (ctrl && key === "s" && workspace && currentPage?.kind === "query") {
      // A query page saves its file; Ctrl+Shift+S saves it under another name.
      event.preventDefault();
      void saveScript(event.shiftKey);
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
    } else if (ctrl && event.altKey && key === "l" && workspace) {
      event.preventDefault();
      newLivePage();
    } else if (ctrl && key === "f" && onLivePage) {
      event.preventDefault();
      void livePage?.focusFilter();
    } else if (ctrl && key === "f" && (tableTab || (activeSet && !keyValue && !running))) {
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
      // A Live page listens on F5 and stops on Shift+F5; a query page runs its script.
      if (onLivePage && event.shiftKey) liveSession?.stop();
      else if (onLivePage) listenLive();
      else if (connection && !running && !onTablePage) runFromToolbar("all");
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

  const workRows = $derived(
    onLivePage ? "auto minmax(0, 1fr)" : onTablePage ? "auto auto minmax(0, 1fr)" : ran ? `auto ${editorHeight}px auto minmax(0, 1fr)` : "auto minmax(0, 1fr)",
  );
  const numberFormat = new Intl.NumberFormat();
  const tableError = $derived(onTablePage ? messages.find((message) => message.kind === "error") : undefined);
  /** The database the status bar names: where a Live page's listening started, else the one a run uses. */
  const statusDatabase = $derived(liveSession?.busy ? liveSession.database : database);
  /** A saved connection's colour; none yet (docs/roadmap.md, connection colours), so no stripe shows. */
  const connectionColor: string | null = null;
  /** The saved connection open now, for Edit this connection… */
  const editableSaved = $derived(connection?.savedId ? (library.find((entry) => entry.id === connection?.savedId) ?? null) : null);

  const pageTabs: PageTabItem[] = $derived.by(() => {
    void editTick;
    return pages.map((page) => ({
      id: page.id,
      title: pageTitle(page),
      icon: page.kind === "table" ? "table" : page.kind === "live" ? "radio" : "file",
      count: page.kind === "query" ? undefined : pageCount(page) || undefined,
      dirty: page.file ? pageDirty(page) : undefined,
      tip: page.file?.path,
    }));
  });
  /** Pages to show: a connection's, a query started with none, or `.sql` files opened with none (ADR 0016). */
  const workspace = $derived(connection !== null || drafting || pages.some((page) => page.file !== null));

  const resultTabs: ResultTabItem[] = $derived(
    sets.map((set, index) => ({
      key: set.result,
      label: `Result ${index + 1}`,
      count: numberFormat.format(set.limited ? (set.filter?.held ?? set.totalRows) : set.totalRows),
      limited: set.limited,
      tip: ranRanges[set.statement] && ranRequest ? `From statement ${set.statement + 1} of the run` : undefined,
    })),
  );
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="pb-app" style:grid-template-rows={connectError ? "auto auto minmax(0, 1fr) auto" : undefined}>
  <!-- The title bar says where you are (D14) and holds the window's buttons. -->
  <!-- The stripe shows only for a connection with a colour of its own. -->
  <TitleBar conn={connection ? connectionColor : null}>
    {#snippet left()}
      {#if connection}
        {@const open = connection}
        {@const saved = editableSaved}
        <ConnectionMenu connection={open} color={connectionColor} {running} onEdit={saved ? () => editSaved(saved) : null} onDisconnect={disconnect} />
        {#if open.engine.hasDatabases}
          <DatabasePicker conn={open.id} {database} onPick={useDatabase} onError={note} />
        {/if}
      {/if}
    {/snippet}
    {#snippet center()}
      <PaletteTrigger width={connection ? 340 : 420} open={paletteOpen} onclick={openPalette} />
    {/snippet}
  </TitleBar>

  {#if connectError}
    <InfoBar tone="danger" strip title={errorTitle(connectError)} message={connectError.message} onDismiss={() => (connectError = null)} />
  {/if}

  {#if workspace}
    <div class="pb-mid" style:grid-template-columns={connection ? `${sidebarWidth}px minmax(0, 1fr)` : "minmax(0, 1fr)"}>
      {#if connection}
      {@const open = connection}
      <!-- Sidebar: Objects fills it; Queries and History fold (D19) -->
      <aside class="pb-side" aria-label="Objects, saved queries and history">
        <SidePane title={keyValue ? "Keys" : "Objects"} main>
          {#snippet actions()}
            {#if !keyValue && open.engine.hasDatabases}
              <Button
                variant="ghost"
                class="pb-pane-btn"
                icon="list-tree"
                open={treeScope === "current"}
                label={treeScope === "current" ? "Show every database" : "Show only the database in use"}
                tip={treeScope === "current" ? "Show every database" : `Show only ${database ?? "the database in use"}`}
                tipAt="end"
                onclick={toggleTreeScope}
              />
            {/if}
            <Button
              variant="ghost"
              class="pb-pane-btn"
              icon="refresh"
              spin={treeRefreshing}
              label={keyValue ? "Scan the keys again" : "Refresh objects"}
              tip={keyValue ? "Scan the keys again  Ctrl+R" : "Refresh objects  Ctrl+R"}
              tipAt="end"
              disabled={treeRefreshing}
              onclick={() => void refreshObjects()}
            />
          {/snippet}
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
              scope={treeScope}
              onOpen={(object, inDatabase, newTab) => void openObject(object, inDatabase, newTab)}
              onUseDatabase={useDatabase}
              onCopy={copyText}
              onAction={(target, kind) => (objectAction = { target, kind })}
              onContext={(target, x, y) => (treeMenu = { target, x, y })}
            />
          {/if}
        </SidePane>
        {#if !keyValue}
          <SidePane title="Queries" bind:open={queriesOpen} count={queryCount}>
            <QueryList
              connectionId={open.savedId}
              refreshKey={queriesKey}
              onInsert={insertSql}
              onRun={runSaved}
              onEdit={(query) => (saving = query)}
              onCount={(count) => (queryCount = count)}
            />
          </SidePane>
        {/if}
        <SidePane title="History" bind:open={historyOpen} count={historyCount === null ? null : `${historyCount}${historyCount === 100 ? "+" : ""}`}>
          <HistoryList connection={open.title} refreshKey={historyKey} onInsert={insertSql} onRun={runEntry} onCount={(count) => (historyCount = count)} />
        </SidePane>
      </aside>
      <Splitter orientation="vertical" bind:value={sidebarWidth} min={180} max={480} reset={260} label="Resize the sidebar" style="left: {sidebarWidth - 3}px" />
      {/if}

      <main class="pb-work" style:grid-template-rows={workRows}>
        <!-- Pages: tabs on the left, the open page's tools on the right (D15) -->
        <PageTabs tabs={pageTabs} active={activePage} onSelect={switchTo} onClose={closePage} onNew={newQueryPage}>
          {#snippet tools()}
            {#if onLivePage && liveSession}
              <LiveTools
                session={liveSession}
                {detailOpen}
                onListen={listenLive}
                onToggleFilters={() => {
                  if (liveSession?.filtersOpen) liveSession.filtersOpen = false;
                  else void livePage?.focusFilter();
                }}
                onToggleDetail={() => (detailOpen = !detailOpen)}
              />
            {:else if onTablePage && tableTab}
              <TableTools
                editable={tableTab.key.length > 0}
                {canEdit}
                hasSelection={selected !== null}
                selectionDeleted={selectionDeletes}
                {filtersOpen}
                filterCount={tableTab.filters.length}
                editSummary={hasEdits ? editSummary || "Unsaved changes" : ""}
                {running}
                editing={editing !== null}
                onToggleFilters={() => void toggleFilters()}
                onAddRow={() => void addRow()}
                onDeleteRows={() => selected && markDeleted(selected.rows)}
                onCopySql={copyPageSql}
                onExport={() => tableTab && openData("export", tableTab.table, tableTab.database)}
                onDiscard={discardEdits}
                onPreview={() => void openPreview()}
              />
            {:else}
              <RunControls
                {running}
                onRun={runFromToolbar}
                onStop={stopRun}
                rowCap={settings.rowCap}
                onRowCap={setRowCap}
                {keyValue}
                saveDisabled={saving !== null}
                onSave={saveFromEditor}
              />
            {/if}
          {/snippet}
        </PageTabs>

        <!-- One editor for every query page; a table page and a Live page hide it (D17). -->
        <section class="pb-editor-host" hidden={offQuery} aria-label="Editor">
          <SqlEditor
            bind:this={editor}
            {grammar}
            plain={keyValue}
            onRun={(request) => void startRun(request)}
            complete={completeAt}
            onChange={onEditorChange}
          />
        </section>

        {#if onLivePage && liveSession && connection}
          <!-- A page's own grid and cursor: made again for each Live page shown. -->
          {#key liveSession}
            <LivePage
              bind:this={livePage}
              session={liveSession}
              engine={connection.engine}
              {database}
              {detailOpen}
              onListen={listenLive}
              onCopy={gridCallbacks.copy}
              onNote={note}
            />
          {/key}
        {:else if onTablePage}
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
              <InfoBar tone="danger" strip title={errorTitle(tableError.error)} message={tableError.error.message} />
            {/if}
          </div>
        {:else if ran}
          <div style="position: relative">
            <Splitter orientation="horizontal" bind:value={editorHeight} min={72} max={() => window.innerHeight - 300} reset={188} label="Resize the editor and the results" />
            {#if keyView}
              {@const view = keyView}
              <KeyBar name={view.name} detail={view.detail} busy={running} onReload={() => void openKey(view.name)} onCopy={() => copyText(view.name, "the key name")} />
            {/if}
            <ResultBar tabs={resultTabs} active={activeTab} {errorCount} onSelect={(tab) => (activeTab = tab)}>
              {#snippet actions()}
                {#if activeSet?.source && !keyValue}
                  {@const source = activeSet.source}
                  <Button
                    variant="ghost"
                    size="sm"
                    icon="table"
                    class="is-shrink"
                    disabled={running}
                    tip={source.whereLeft
                      ? "Open the whole table: its WHERE is more than the filter row holds"
                      : source.filters.length > 0
                        ? "Open the table on its own page, filtered by the statement's WHERE"
                        : "Open the table on its own page, to edit its rows"}
                    onclick={() => void openResultTable(source)}><span class="pb-btn-text">Open {source.table.name} to edit</span></Button
                  >
                {/if}
                {#if activeSet && !keyValue}
                  {@const filtered = activeSet.filter}
                  <Button
                    variant="ghost"
                    size="sm"
                    icon="filter"
                    open={filtersOpen}
                    aria-pressed={filtersOpen}
                    disabled={running}
                    tip="Filter the rows held  Ctrl+F"
                    onclick={() => void toggleFilters()}>Filter{#if filtered}<Badge size="sm">{filtered.conditions.length}</Badge>{/if}</Button
                  >
                {/if}
                <Button
                  variant="ghost"
                  size="sm"
                  icon="pane"
                  label="Row detail"
                  open={detailOpen}
                  aria-pressed={detailOpen}
                  tip="Row detail  Ctrl+Alt+I"
                  tipAt="end"
                  onclick={() => (detailOpen = !detailOpen)}
                />
              {/snippet}
            </ResultBar>
            {#if filtersOpen && activeSet && !keyValue}
              {@const set = activeSet}
              {#key set.result}
                <FilterRow
                  bind:this={filterRow}
                  columns={set.columns.map((meta) => meta.name)}
                  conditions={set.filter?.conditions ?? []}
                  matchAny={set.filter?.matchAny ?? false}
                  busy={running}
                  onApply={(conditions, matchAny) => void applyResultFilter(conditions, matchAny)}
                />
              {/key}
              {#if set.filter?.where?.sql}
                <WhereStrip where={set.filter.where} onCopy={copyWhere} />
              {/if}
            {/if}
          </div>
        {/if}

        {#if onTablePage || ran}
          <div class="pb-results">
            {#if activeTab === "messages" && !onTablePage}
              <MessageList {messages} statement={statementLabel} />
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
    <Welcome
      busy={connecting}
      onNew={newConnection}
      onNewQuery={() => void startQuery()}
      onOpenFile={() => void openFile()}
      onMemory={() => void connectTo(":memory:")}
    />
  {/if}

  <!-- Status bar: what the result is; the pager for a table page (D18) -->
  <StatusBar>
    {#if !connection}
      <span>{connecting ? "Connecting…" : "No connection"}</span>
      {#if statusNote}<span class="is-note">{statusNote}</span>{/if}
    {:else}
      {#if running}
        <span class="is-live"><Icon name="loader" size="sm" spin />Running…</span>
      {:else if elapsedMs !== null}
        <span>{numberFormat.format(elapsedMs)} ms</span>
      {/if}
      {#if onLivePage && liveSession}
        {@const live = liveSession}
        {#if live.status === "listening"}
          <span class="is-live"><Icon name="radio" size="sm" />Listening</span>
        {:else if live.status === "starting"}
          <span class="is-live"><Icon name="loader" size="sm" spin />Starting…</span>
        {:else if live.status === "stopping"}
          <span>Stopping…</span>
        {:else if live.result !== null}
          <span>Stopped</span>
        {/if}
        {#if live.result !== null}
          <span
            >{live.filter
              ? `${numberFormat.format(live.shown)} of ${numberFormat.format(live.total)} events`
              : `${numberFormat.format(live.total)} ${live.total === 1 ? "event" : "events"}`}</span
          >
        {/if}
        {#if live.sort.length > 0}<span>sorted in Paboable</span>{/if}
      {/if}
      {#if ranCount && !offQuery}
        <span class:is-note={ranCount.ran < ranCount.total}>
          {ranCount.ran < ranCount.total
            ? `Ran ${ranCount.ran} of ${ranCount.total} statements`
            : `Ran ${ranCount.ran} ${ranCount.ran === 1 ? "statement" : "statements"}`}
        </span>
      {/if}
      {#if connection.engine.hasDatabases && statusDatabase}<span>in {statusDatabase}</span>{/if}
      {#if !ran && !offQuery}<span>No result yet</span>{/if}
      {#if selected && selected.rows.length > 1}
        <span>{numberFormat.format(selectedRows)} rows selected</span>
      {:else if selected && selectedRows * selected.columnCount > 1}
        <span>{numberFormat.format(selectedRows)} × {numberFormat.format(selected.columnCount)} selected</span>
      {/if}
      {#if activeSet}
        {#if onTablePage && tableTab}
          <span
            >{numberFormat.format(activeSet.totalRows)} rows{#if typeof tableTab.count === "number"}&nbsp;of {numberFormat.format(
                tableTab.count,
              )}{:else if tableTab.count === "counting"}&nbsp;of …{:else}&nbsp;<Button variant="link" disabled={running} onclick={() => void countTable()}>Count</Button
              >{/if}</span
          >
        {:else}
          {#if activeSet.filter}
            <span>{numberFormat.format(activeSet.totalRows)} of {numberFormat.format(activeSet.filter.held)} rows held</span>
          {:else}
            <span>{numberFormat.format(activeSet.totalRows)} rows{activeSet.finished ? "" : " so far"}</span>
          {/if}
          {#if activeSet.limited}
            <Badge tone="warning" dot>Stopped at {numberFormat.format(activeSet.filter?.held ?? activeSet.totalRows)} rows</Badge>
            {#if activeSet.filter}<span>the filter covers only the rows loaded</span>{/if}
            <Button variant="link" disabled={running} onclick={fetchAll}>Fetch all</Button>
          {/if}
          {#if activeSet.sortedBy === "server"}<span>sorted by the server</span>{/if}
          {#if activeSet.sortedBy === "core"}<span>sorted in Paboable</span>{/if}
        {/if}
      {/if}
      {#if hasEdits}<Badge dot>Unsaved changes</Badge>{/if}
      {#if statusNote}<span class="is-note">{statusNote}</span>{/if}
    {/if}
    {#snippet end()}
      {#if !connection}
        <span>Paboable</span>
      {:else if onTablePage && tableTab}
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
    {/snippet}
  </StatusBar>
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
    onOpenNew={(target) => void openObject(target.object, target.database, true)}
    onUseDatabase={useDatabase}
    onCopy={copyText}
    onRefresh={() => void refreshObjects()}
    engine={connection?.engine.id ?? ""}
    onStructure={(target) => (structureOf = target)}
    onShowDdl={(target) => void showDdl(target)}
    onCopyDdl={(target) => void copyDdl(target)}
    onEditView={(target) => void editView(target)}
    onAction={(target, kind) => (objectAction = { target, kind })}
    onCopyQualified={(target) => void copyQualified(target)}
    onData={(target, kind) => openData(kind, { schema: target.object.schema, name: target.object.name }, target.database)}
  />
{/if}

{#if dataDialog && connection}
  {@const data = dataDialog}
  {#if data.kind === "export"}
    <ExportDialog
      conn={connection.id}
      database={data.database}
      table={data.table}
      keyColumns={data.keyColumns}
      filtered={data.filtered}
      onDone={dataDone}
      onClose={() => (dataDialog = null)}
    />
  {:else if data.kind === "import"}
    <ImportDialog conn={connection.id} database={data.database} table={data.table} onDone={dataDone} onClose={() => (dataDialog = null)} />
  {:else}
    <GenerateRowsDialog conn={connection.id} database={data.database} table={data.table} onDone={dataDone} onClose={() => (dataDialog = null)} />
  {/if}
{/if}

{#if objectAction && connection}
  {@const action = objectAction}
  <ObjectActionDialog
    conn={connection.id}
    database={action.target.database}
    engine={connection.engine.id}
    object={action.target.object}
    kind={action.kind}
    onDone={actionDone}
    onClose={() => (objectAction = null)}
  />
{/if}

{#if structureOf && connection}
  {@const target = structureOf}
  <StructureDialog
    conn={connection.id}
    database={target.database}
    object={target.object}
    onCopy={copyText}
    onEdit={(ddl) => {
      structureOf = null;
      void openTextOnPage(ddl);
    }}
    onClose={() => (structureOf = null)}
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
    onManageProfiles={(initial) => (profilesDialog = { initial })}
    {profilesKey}
  />
{/if}

{#if profilesDialog}
  <ProfilesDialog
    initial={profilesDialog.initial}
    onChanged={() => {
      profilesKey++;
      savedKey++;
    }}
    onClose={() => (profilesDialog = null)}
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

{#if closing}
  {@const target = closing}
  <UnsavedDialog name={target.name} onAnswer={(answer) => void answerClosing(answer)} />
{/if}

{#if askPassword}
  {@const target = askPassword}
  <PasswordDialog saved={target} onSubmit={(password) => void openSaved(target, password)} onClose={() => (askPassword = null)} />
{/if}

<!-- Last, so it covers everything else until it is answered. -->
{#if termsNeeded}
  <TermsDialog
    onAgree={() => {
      // Which terms, and when: kept on this computer only, as the terms promise.
      writePref("termsAccepted", TERMS_VERSION);
      writePref("termsAcceptedAt", new Date().toISOString());
      termsNeeded = false;
    }}
    onDisagree={() => void appWindow.close()}
  />
{/if}
