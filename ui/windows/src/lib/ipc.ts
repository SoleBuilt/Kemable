// The one place the UI calls the Rust side. Every command is listed here with
// its types, so a rename in commands.rs fails `pnpm check` instead of at runtime.

import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  CredentialProfile,
  BrowseQuery,
  BrowseStatements,
  ColumnInfo,
  CompletionReply,
  ConnectionConfig,
  CoreError,
  EditRequest,
  DatabaseInfo,
  EngineInfo,
  HistoryEntry,
  KeyPage,
  KeyRead,
  KeyScan,
  ObjectInfo,
  OpenedConnection,
  ResultCounts,
  ResultFilter,
  ResultWhere,
  RowSpan,
  RunEvent,
  RunStarted,
  SavedConnection,
  SavedQuery,
  SchemaInfo,
  SortOutcome,
  TableRef,
  TestedConnection,
} from "./types";

export const ipc = {
  engines: () => invoke<EngineInfo[]>("engines"),

  /** A pasted connection URL as a config for the connect form. */
  parseConnectionUrl: (url: string) => invoke<ConnectionConfig>("parse_connection_url", { url }),

  /** Opens a connection as typed; `profileId` signs it in with a credential profile instead. */
  connect: (config: ConnectionConfig, profileId: string | null = null) => invoke<OpenedConnection>("connect", { config, profileId }),

  /** Connects, reads the version and the databases, and closes. `savedId` lends a kept password to a blank field. */
  testConnection: (config: ConnectionConfig, savedId: string | null, profileId: string | null = null) =>
    invoke<TestedConnection>("test_connection", { config, savedId, profileId }),
  /** The databases the connect form's login may see: connects as Test does, lists them, closes. */
  listFormDatabases: (config: ConnectionConfig, savedId: string | null, profileId: string | null) =>
    invoke<string[]>("list_form_databases", { config, savedId, profileId }),

  /** The saved connections, in the order they were added. */
  listSaved: () => invoke<SavedConnection[]>("list_saved"),

  /** Saves a connection; `password`, when given, replaces the kept one. */
  saveConnection: (connection: SavedConnection, password: string | null) =>
    invoke<SavedConnection>("save_connection", { connection, password }),

  deleteSaved: (id: string) => invoke<void>("delete_saved", { id }),

  /** The credential profiles, by name; their passwords stay in Windows Credential Manager. */
  listProfiles: () => invoke<CredentialProfile[]>("list_profiles"),
  /** Saves a profile; `password`, when given, replaces the kept one. */
  saveProfile: (profile: CredentialProfile, password: string | null) =>
    invoke<CredentialProfile>("save_profile", { profile, password }),
  /** Deletes a profile; resolves to how many saved connections used it. */
  deleteProfile: (id: string) => invoke<number>("delete_profile", { id }),

  /** Opens a saved connection; `password` is for one saved without it. */
  connectSaved: (id: string, password: string | null) =>
    invoke<OpenedConnection>("connect_saved", { id, password }),

  disconnect: (conn: number) => invoke<void>("disconnect", { conn }),

  /** The databases this login may open, for an engine with databases. */
  listDatabases: (conn: number) => invoke<DatabaseInfo[]>("list_databases", { conn }),

  listSchemas: (conn: number, database: string | null) => invoke<SchemaInfo[]>("list_schemas", { conn, database }),

  listObjects: (conn: number, database: string | null, schema: string | null) =>
    invoke<ObjectInfo[]>("list_objects", { conn, database, schema }),

  /** A table's columns, with its primary key. */
  fetchColumns: (conn: number, database: string | null, table: TableRef) =>
    invoke<ColumnInfo[]>("fetch_columns", { conn, database, table }),

  /** The page and count statements a table tab runs. */
  browseStatements: (conn: number, query: BrowseQuery) => invoke<BrowseStatements>("browse_statements", { conn, query }),

  /** The names to offer at UTF-8 byte `cursor` of `text`; null inside a string or a comment. */
  complete: (conn: number, database: string | null, text: string, cursor: number) =>
    invoke<CompletionReply | null>("complete", { conn, database, text, cursor }),

  /** Forgets what completion read of a server's catalog, after a change to it. */
  forgetCatalog: (conn: number) => invoke<void>("forget_catalog", { conn }),

  /** One step of a scan over a key-value server's keys. */
  scanKeys: (conn: number, database: string | null, scan: KeyScan) =>
    invoke<KeyPage>("scan_keys", { conn, database, scan }),

  /** Reads a key into a result set, up to `rowLimit` rows. */
  readKey: (conn: number, database: string | null, key: string, rowLimit: number | null) =>
    invoke<KeyRead>("read_key", { conn, database, key, rowLimit }),

  /** The statements saving `request` would run, for the preview. */
  editPreview: (conn: number, request: EditRequest) => invoke<string[]>("edit_preview", { conn, request }),

  /** Runs previewed edit statements in one transaction, all or none. */
  applyEdits: (conn: number, database: string | null, statements: string[]) =>
    invoke<void>("apply_edits", { conn, database, statements }),

  /** Runs a count statement; gives up after 30 s. */
  countRows: (conn: number, database: string | null, sql: string) =>
    invoke<number>("count_rows", { conn, database, sql }),

  /**
   * Starts a run in `database` (null: the one the connection opened on) and
   * returns at once. Events arrive on `onEvent` until one of type `finished`.
   * `cursor` is a UTF-8 byte offset: with it, only the statement under the
   * cursor runs; without it, every statement in `sql`. With
   * `followCreatedDatabase`, the statements after `CREATE DATABASE x` run in x.
   * `rowLimit` caps each result set; null keeps every row.
   */
  runQuery(
    conn: number,
    database: string | null,
    sql: string,
    cursor: number | null,
    followCreatedDatabase: boolean,
    rowLimit: number | null,
    recordHistory: boolean,
    onEvent: (event: RunEvent) => void,
  ) {
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<RunStarted>("run_query", {
      conn,
      database,
      sql,
      cursor,
      followCreatedDatabase,
      rowLimit,
      recordHistory,
      onEvent: channel,
    });
  },

  /** The query history, newest first. */
  historySearch: (query: { text: string; connection: string | null; before: number | null; limit: number | null }) =>
    invoke<HistoryEntry[]>("history_search", { query }),

  /** Deletes one history entry, or every one when `id` is null. */
  historyDelete: (id: number | null) => invoke<void>("history_delete", { id }),

  /** The saved queries a connection shows: its own and the global ones. */
  listQueries: (connectionId: string | null) => invoke<SavedQuery[]>("list_queries", { connectionId }),

  saveQuery: (query: SavedQuery) => invoke<SavedQuery>("save_query", { query }),

  deleteQuery: (id: string) => invoke<void>("delete_query", { id }),

  /** Sorts a result: by running it again with ORDER BY, or in the core. No keys clears the core's sort. */
  sortResult: (result: number, keys: { column: number; descending: boolean }[]) =>
    invoke<SortOutcome>("sort_result", { result, keys }),

  cancelRun: (run: number) => invoke<void>("cancel_run", { run }),

  /** One packed window of display text (lib/grid/window.ts reads it). */
  fetchWindow: (
    result: number,
    firstRow: number,
    rowCount: number,
    firstColumn: number,
    columnCount: number,
    maxChars: number,
  ) => invoke<ArrayBuffer>("fetch_window", { result, firstRow, rowCount, firstColumn, columnCount, maxChars }),

  /** A cell's full value for copy, `null` for SQL NULL. */
  cellText: (result: number, row: number, column: number) => invoke<string | null>("cell_text", { result, row, column }),

  /** The same columns of several row spans, one after the other, as tab-separated text spreadsheets paste. */
  copyRange: (result: number, rows: RowSpan[], firstColumn: number, columnCount: number, withHeaders: boolean) =>
    invoke<string>("copy_range", { result, rows, firstColumn, columnCount, withHeaders }),

  /** Every value of one row, null for SQL NULL. */
  rowValues: (result: number, row: number) => invoke<(string | null)[]>("row_values", { result, row }),

  releaseResult: (result: number) => invoke<void>("release_result", { result }),

  /**
   * Filters the rows a result holds, in the core; the statement never runs
   * again. No filters clears it; a core sort stays. Windows, copies and row
   * reads then go through the filter: row numbers are rows shown.
   */
  filterResult: (result: number, filters: ResultFilter[], matchAny: boolean) =>
    invoke<ResultCounts>("filter_result", { result, filters, matchAny }),

  /** The rows a result shows and holds; run events count only the rows held. */
  resultCounts: (result: number) => invoke<ResultCounts>("result_counts", { result }),

  /** A result's filter as a WHERE for the connection's engine, to copy. */
  resultWhere: (conn: number, result: number, filters: ResultFilter[], matchAny: boolean) =>
    invoke<ResultWhere>("result_where", { conn, result, filters, matchAny }),
};

/** Work that must happen before the window closes from its close button, such as keeping the pages. */
export const beforeClose = new Set<() => void>();

/**
 * The app's window. It has no system title bar: the toolbar draws the
 * minimize, maximize and close buttons (docs/decisions/0014-ui-components.md).
 */
export const appWindow = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  /** Runs the `beforeClose` work (keeping the pages), then closes. */
  close: () => {
    for (const work of beforeClose) work();
    return getCurrentWindow().close();
  },
  isMaximized: () => getCurrentWindow().isMaximized(),
  /** Calls `handler` after every resize; resolves to the function that stops it. */
  onResized: (handler: () => void) => getCurrentWindow().onResized(handler),
  /** Puts Mica behind the window or takes it away; resolves to whether Mica is on (never on Windows 10). */
  setMica: (on: boolean, dark: boolean) => invoke<boolean>("set_mica", { on, dark }),
};

export function asCoreError(error: unknown): CoreError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    return error as CoreError;
  }
  return { code: "internal", message: String(error) };
}
