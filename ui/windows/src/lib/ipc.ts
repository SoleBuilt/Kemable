// The one place the UI calls the Rust side. Every command is listed here with
// its types, so a rename in commands.rs fails `pnpm check` instead of at runtime.

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ConnectionConfig,
  CoreError,
  DatabaseInfo,
  EngineInfo,
  HistoryEntry,
  ObjectInfo,
  OpenedConnection,
  RunEvent,
  RunStarted,
  SavedConnection,
  SavedQuery,
  SchemaInfo,
  SortOutcome,
} from "./types";

export const ipc = {
  engines: () => invoke<EngineInfo[]>("engines"),

  /** A pasted connection URL as a config for the connect form. */
  parseConnectionUrl: (url: string) => invoke<ConnectionConfig>("parse_connection_url", { url }),

  connect: (config: ConnectionConfig) => invoke<OpenedConnection>("connect", { config }),

  /** The saved connections, in the order they were added. */
  listSaved: () => invoke<SavedConnection[]>("list_saved"),

  /** Saves a connection; `password`, when given, replaces the kept one. */
  saveConnection: (connection: SavedConnection, password: string | null) =>
    invoke<SavedConnection>("save_connection", { connection, password }),

  deleteSaved: (id: string) => invoke<void>("delete_saved", { id }),

  /** Opens a saved connection; `password` is for one saved without it. */
  connectSaved: (id: string, password: string | null) =>
    invoke<OpenedConnection>("connect_saved", { id, password }),

  disconnect: (conn: number) => invoke<void>("disconnect", { conn }),

  /** The databases this login may open, for an engine with databases. */
  listDatabases: (conn: number) => invoke<DatabaseInfo[]>("list_databases", { conn }),

  listSchemas: (conn: number, database: string | null) => invoke<SchemaInfo[]>("list_schemas", { conn, database }),

  listObjects: (conn: number, database: string | null, schema: string | null) =>
    invoke<ObjectInfo[]>("list_objects", { conn, database, schema }),

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

  /** A rectangle of cells as tab-separated text, as spreadsheets paste it. */
  copyRange: (
    result: number,
    firstRow: number,
    rowCount: number,
    firstColumn: number,
    columnCount: number,
    withHeaders: boolean,
  ) => invoke<string>("copy_range", { result, firstRow, rowCount, firstColumn, columnCount, withHeaders }),

  /** Every value of one row, null for SQL NULL. */
  rowValues: (result: number, row: number) => invoke<(string | null)[]>("row_values", { result, row }),

  releaseResult: (result: number) => invoke<void>("release_result", { result }),
};

export function asCoreError(error: unknown): CoreError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    return error as CoreError;
  }
  return { code: "internal", message: String(error) };
}
