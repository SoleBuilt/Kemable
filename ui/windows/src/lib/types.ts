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
  /** The saved connection it was opened from, if any. */
  savedId: string | null;
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

export type HistoryOutcome = "succeeded" | "failed" | "cancelled";

/** One statement run from the editor. */
export interface HistoryEntry {
  id: number;
  ranAt: number;
  connection: string;
  engine: string;
  database: string | null;
  statement: string;
  durationMs: number;
  rows: number | null;
  outcome: HistoryOutcome;
  error: string | null;
}

/** Named SQL; `connectionId` null shows it for every connection. */
export interface SavedQuery {
  id: string;
  name: string;
  sql: string;
  connectionId: string | null;
  updatedAt: number;
}

export interface TableRef {
  schema: string | null;
  name: string;
}

/** A table's column as the catalog describes it. */
export interface ColumnInfo {
  ordinal: number;
  name: string;
  typeName: string;
  nullable: boolean;
  default: string | null;
  /** Position in the primary key, from 1. */
  primaryKey: number | null;
}

export type FilterOp =
  | "equals"
  | "notEquals"
  | "contains"
  | "notContains"
  | "startsWith"
  | "endsWith"
  | "greater"
  | "greaterOrEqual"
  | "less"
  | "lessOrEqual"
  | "in"
  | "between"
  | "isNull"
  | "isNotNull"
  | "raw";

/** One condition of a table tab; `raw` takes `value` as SQL and ignores `column`. */
export interface Filter {
  column: string;
  op: FilterOp;
  value: string;
  /** The upper end of `between`. */
  second: string;
}

/** One page of a table (docs/decisions/0006-table-browsing.md). */
export interface BrowseQuery {
  table: TableRef;
  filters: Filter[];
  matchAny: boolean;
  sort: { column: string; descending: boolean }[];
  /** The primary key's columns, which break ties. */
  key: string[];
  offset: number;
  limit: number;
}

export interface BrowseStatements {
  page: string;
  count: string;
}

export type CompletionKind = "schema" | "table" | "view" | "column" | "alias";

/** A name the editor offers (docs/decisions/0007-autocomplete.md). */
export interface CompletionItem {
  label: string;
  /** What replaces the word typed, quoted when the name needs it. */
  apply: string;
  kind: CompletionKind;
  detail: string;
  boost: number;
}

export interface CompletionReply {
  /** The UTF-8 byte offset where the word the items replace starts. */
  from: number;
  items: CompletionItem[];
}
