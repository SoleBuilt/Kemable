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

/** Rows from the first, in the order the grid shows them. */
export interface RowSpan {
  firstRow: number;
  rowCount: number;
}

/** What Test in the connect form learned; nothing stays open after it. */
export interface TestedConnection {
  serverVersion: string | null;
  /** How many databases the login may see, for an engine with databases. */
  databases: number | null;
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
  | "tableFunction"
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
  | {
      type: "setStarted";
      result: number;
      statement: number;
      columns: ColumnMeta[];
      /** The statement inside a SQL Server batch that made the set, as UTF-8 byte offsets into the statement's text. */
      part: ByteRange | null;
    }
  /** The sets of `statement` did not match how its batch was read: drop their parts. */
  | { type: "partsDropped"; statement: number }
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
  /** The credential profile it signs in with; its user is copied into the config. */
  profileId: string | null;
  /** A `--conn-*` token's name ("red", "teal"), drawn as a dot beside its name; null is grey. */
  color: string | null;
}

/** The one table a query result comes from (docs/decisions/0011-filtering-a-result.md). */
export interface ResultSource {
  /** The database it is in: the run's, or a SQL Server three-part name's. */
  database: string | null;
  table: TableRef;
  /** The statement's WHERE as the table tab's filters, when they can hold it. */
  filters: Filter[];
  matchAny: boolean;
  /** The statement had a WHERE the filters could not hold: the table opens whole. */
  whereLeft: boolean;
}

/** How a script file's bytes are written (docs/decisions/0016-sql-files.md). */
export type TextEncoding = "utf8" | "utf8Bom" | "utf16Le" | "utf16Be";
export type LineEnding = "crlf" | "lf";

/** A script file's text with `
` line endings, and how a save writes it back. */
export interface TextFile {
  text: string;
  encoding: TextEncoding;
  lineEnding: LineEnding;
}

/** A user and a password kept under a name, for many connections to sign in with. */
export interface CredentialProfile {
  /** Empty for one not saved yet. */
  id: string;
  name: string;
  user: string;
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

/** What a Redis key holds (docs/decisions/0008-key-value-engines.md). */
export type KeyKind = "string" | "list" | "set" | "sortedSet" | "hash" | "stream" | "other";

export interface KeyInfo {
  name: string;
  kind: KeyKind;
  /** Milliseconds before it expires; null when it does not. */
  ttlMs: number | null;
}

export interface KeyScan {
  pattern: string;
  /** Where the last step stopped; null to start. */
  cursor: string | null;
  /** How many keys the server looks at in a step: a hint. */
  count: number;
  kind: KeyKind | null;
}

export interface KeyPage {
  keys: KeyInfo[];
  /** Null once the scan has seen every key. */
  cursor: string | null;
}

export interface KeyDetail {
  kind: KeyKind;
  ttlMs: number | null;
  /** Elements in a collection, bytes in a string. */
  length: number;
}

/** A key read into a result set. */
export interface KeyRead {
  result: number | null;
  columns: ColumnMeta[];
  detail: KeyDetail;
  totalRows: number;
  limited: boolean;
  notices: string[];
}

/** What a cell becomes when edits are saved (docs/decisions/0010-editing-rows.md). */
export type NewValue = { kind: "null" } | { kind: "default" } | { kind: "text"; text: string };

export interface CellEdit {
  column: number;
  value: NewValue;
}

/** A table tab's pending edits; rows are rows of `result`, the page it shows. */
export interface EditRequest {
  result: number;
  table: TableRef;
  key: string[];
  updates: { row: number; cells: CellEdit[] }[];
  deletes: number[];
  inserts: CellEdit[][];
}

/** A condition on a query result's rows (docs/decisions/0011-filtering-a-result.md): the table tab's, without `raw`. */
export type ResultFilterOp = Exclude<FilterOp, "raw">;

/** One condition on a query result. `column` is the column's index: two columns can share a name. */
export interface ResultFilter {
  column: number;
  op: ResultFilterOp;
  value: string;
  /** The upper end of `between`. */
  second: string;
}

/** What a result shows under its filter. */
export interface ResultCounts {
  /** The rows the filter keeps; every row held when no filter is on. */
  shownRows: number;
  /** The rows the result holds. */
  totalRows: number;
  filtered: boolean;
  /** The result stopped at its row limit: the filter covers only the rows loaded. */
  limited: boolean;
}

/** A result's filter as a WHERE clause to copy, never run. */
export interface ResultWhere {
  /** `WHERE ...` without a trailing `;`; empty when no condition could be written. */
  sql: string;
  /** Indexes into the filters of the conditions left out: their column's name is empty or shared. */
  leftOut: number[];
}
