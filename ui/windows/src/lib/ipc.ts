// The one place the UI calls the Rust side. Every command is listed here with
// its types, so a rename in commands.rs fails `pnpm check` instead of at runtime.

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ConnectionConfig,
  CoreError,
  EngineInfo,
  ObjectInfo,
  OpenedConnection,
  RunEvent,
  SchemaInfo,
} from "./types";

export const ipc = {
  engines: () => invoke<EngineInfo[]>("engines"),

  /** A pasted connection URL as a config for the connect form. */
  parseConnectionUrl: (url: string) => invoke<ConnectionConfig>("parse_connection_url", { url }),

  connect: (config: ConnectionConfig) => invoke<OpenedConnection>("connect", { config }),

  disconnect: (conn: number) => invoke<void>("disconnect", { conn }),

  listSchemas: (conn: number) => invoke<SchemaInfo[]>("list_schemas", { conn }),

  listObjects: (conn: number, schema: string | null) => invoke<ObjectInfo[]>("list_objects", { conn, schema }),

  /**
   * Starts a run and returns its id at once. Events arrive on `onEvent` until
   * one of type `finished`. `cursor` is a UTF-8 byte offset: with it, only the
   * statement under the cursor runs; without it, every statement in `sql`.
   */
  runQuery(conn: number, sql: string, cursor: number | null, onEvent: (event: RunEvent) => void) {
    const channel = new Channel<RunEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("run_query", { conn, sql, cursor, onEvent: channel });
  },

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

  releaseResult: (result: number) => invoke<void>("release_result", { result }),
};

export function asCoreError(error: unknown): CoreError {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    return error as CoreError;
  }
  return { code: "internal", message: String(error) };
}
