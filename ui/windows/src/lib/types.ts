// Mirrors of the Rust types the commands send (ui/windows/src-tauri/src/commands.rs).
// Field names are camelCase on both sides through serde's rename_all.

export type ValueKind =
  | "null"
  | "bool"
  | "int"
  | "uInt"
  | "float"
  | "float32"
  | "decimal"
  | "text"
  | "json"
  | "bytes"
  | "uuid"
  | "date"
  | "time"
  | "timestamp"
  | "timestampTz"
  | "other";

export type DataModel = "sql" | "keyValue" | "document" | "queue";
export type TargetKind = "file" | "network" | "http";
export type SqlGrammar = "postgres" | "sqlServer" | "sqlite";
export type TlsMode = "disable" | "prefer" | "require" | "verifyFull";

export interface EngineInfo {
  id: string;
  displayName: string;
  dataModel: DataModel;
  targets: TargetKind[];
  defaultPort: number | null;
  sqlGrammar: SqlGrammar | null;
  live: boolean;
  /** The server holds databases the tree lists and a run picks between. */
  hasDatabases: boolean;
}

export type Target =
  | { kind: "file"; path: string; readOnly?: boolean }
  | {
      kind: "network";
      host: string;
      port?: number | null;
      user?: string | null;
      password?: string;
      database?: string | null;
      tls?: TlsMode;
    }
  | { kind: "http"; endpoint: string; token?: string };

export interface ConnectionConfig {
  engine: string;
  target: Target;
  options?: Record<string, string>;
}

export interface OpenedConnection {
  id: number;
  engine: EngineInfo;
  serverVersion: string | null;
  /** The database the connection opened on, for an engine with databases. */
  database: string | null;
  title: string;
}

export interface DatabaseInfo {
  name: string;
  isCurrent: boolean;
}

export interface ByteRange {
  start: number;
  end: number;
}

export interface RunStarted {
  run: number;
  /** The UTF-8 byte ranges of the statements the run sends. */
  ranges: ByteRange[];
  /** How many statements the whole text holds. */
  scriptStatements: number;
}

export interface SchemaInfo {
  name: string;
  isDefault: boolean;
}

export type ObjectKind =
  | "table"
  | "view"
  | "materializedView"
  | "index"
  | "trigger"
  | "function"
  | "procedure"
  | "sequence"
  | "collection"
  | "key"
  | "queue"
  | "exchange"
  | "other";

export interface ObjectInfo {
  schema: string | null;
  name: string;
  kind: ObjectKind;
}

export interface ColumnMeta {
  name: string;
  typeName: string;
  kind: ValueKind;
}

/** A failed command. `code` is stable; the UI words it (lib/errors.ts). */
export interface CoreError {
  code: string;
  message: string;
  engineCode?: string;
  position?: { byteOffset: number };
}

export type RunEvent =
  | { type: "statementStarted"; index: number; text: string }
  | { type: "setStarted"; result: number; statement: number; columns: ColumnMeta[] }
  | { type: "rows"; result: number; total: number }
  | { type: "setFinished"; result: number; total: number; limited: boolean }
  | { type: "commandDone"; statement: number; rowsAffected: number | null }
  | { type: "notice"; statement: number; text: string }
  | { type: "databaseChanged"; statement: number; database: string }
  | { type: "statementFailed"; statement: number; error: CoreError }
  | { type: "finished"; elapsedMs: number; cancelled: boolean; failed: boolean };

/** A connection in the library. Its config never carries the secret. */
export interface SavedConnection {
  /** Empty for one not saved yet. */
  id: string;
  name: string;
  config: ConnectionConfig;
  /** The password is kept in Windows Credential Manager. */
  savePassword: boolean;
  /** Milliseconds since the Unix epoch. */
  lastUsed: number | null;
}

/** How `sort_result` sorted (docs/decisions/0005-sorting-a-result.md). */
export type SortOutcome =
  | {
      /** Run `sql` for the sorted result; `statement` is the one it was written from. */
      kind: "rerun";
      sql: string;
      statement: string;
      database: string | null;
      rowLimit: number | null;
    }
  | { kind: "sorted"; limited: boolean };
