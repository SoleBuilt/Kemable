// Reads the packed window `fetch_window` returns. The layout is defined once,
// in core/crates/store/src/window.rs; keep this reader in step with it.

const MAGIC = 0x31574250; // "PBW1" little-endian
const FLAG_FINISHED = 1;
const FLAG_LIMITED = 2;

/** Kind bytes, as `kemable_core::value::ValueKind` numbers them. */
export const Kind = {
  Null: 0,
  Bool: 1,
  Int: 2,
  UInt: 3,
  Float: 4,
  Decimal: 5,
  Text: 6,
  Json: 7,
  Bytes: 8,
  Uuid: 9,
  Date: 10,
  Time: 11,
  Timestamp: 12,
  TimestampTz: 13,
  Other: 14,
  Float32: 15,
} as const;

export function isNumericKind(kind: number): boolean {
  return (
    kind === Kind.Int ||
    kind === Kind.UInt ||
    kind === Kind.Float ||
    kind === Kind.Float32 ||
    kind === Kind.Decimal
  );
}

const decoder = new TextDecoder();

export class ResultWindow {
  readonly firstRow: number;
  readonly totalRows: number;
  readonly rowCount: number;
  readonly firstColumn: number;
  readonly columnCount: number;
  readonly finished: boolean;
  readonly limited: boolean;
  readonly columnKinds: Uint8Array;

  private readonly cellKinds: Uint8Array;
  private readonly nulls: Uint8Array;
  private readonly truncated: Uint8Array;
  private readonly offsets: Uint32Array;
  private readonly bytes: Uint8Array;
  // Decoding is lazy: most cells of a window are never on screen.
  private readonly strings: (string | undefined)[];

  constructor(buffer: ArrayBuffer) {
    const view = new DataView(buffer);
    if (buffer.byteLength < 64 || view.getUint32(0, true) !== MAGIC) {
      throw new Error("not a result window");
    }
    this.firstRow = Number(view.getBigUint64(8, true));
    this.totalRows = Number(view.getBigUint64(16, true));
    this.rowCount = view.getUint32(24, true);
    this.firstColumn = view.getUint32(28, true);
    this.columnCount = view.getUint32(32, true);
    const flags = view.getUint32(36, true);
    this.finished = (flags & FLAG_FINISHED) !== 0;
    this.limited = (flags & FLAG_LIMITED) !== 0;

    const cellKindsAt = view.getUint32(40, true);
    const nullsAt = view.getUint32(44, true);
    const truncatedAt = view.getUint32(48, true);
    const offsetsAt = view.getUint32(56, true);
    const textAt = view.getUint32(60, true);
    const cells = this.rowCount * this.columnCount;

    this.columnKinds = new Uint8Array(buffer, 64, this.columnCount);
    this.cellKinds = new Uint8Array(buffer, cellKindsAt, cells);
    this.nulls = new Uint8Array(buffer, nullsAt, Math.ceil(cells / 8));
    this.truncated = new Uint8Array(buffer, truncatedAt, Math.ceil(cells / 8));
    this.offsets = new Uint32Array(buffer, offsetsAt, cells + 1);
    this.bytes = new Uint8Array(buffer, textAt);
    this.strings = new Array<string | undefined>(cells);
  }

  /** Whether the window holds `row` (an absolute row number). */
  has(row: number): boolean {
    return row >= this.firstRow && row < this.firstRow + this.rowCount;
  }

  private cell(row: number, column: number): number {
    return (column - this.firstColumn) * this.rowCount + (row - this.firstRow);
  }

  isNull(row: number, column: number): boolean {
    const cell = this.cell(row, column);
    return ((this.nulls[cell >> 3] ?? 0) & (1 << (cell & 7))) !== 0;
  }

  isTruncated(row: number, column: number): boolean {
    const cell = this.cell(row, column);
    return ((this.truncated[cell >> 3] ?? 0) & (1 << (cell & 7))) !== 0;
  }

  kind(row: number, column: number): number {
    return this.cellKinds[this.cell(row, column)] ?? Kind.Null;
  }

  text(row: number, column: number): string {
    const cell = this.cell(row, column);
    let value = this.strings[cell];
    if (value === undefined) {
      const start = this.offsets[cell] ?? 0;
      const end = this.offsets[cell + 1] ?? start;
      value = decoder.decode(this.bytes.subarray(start, end));
      this.strings[cell] = value;
    }
    return value;
  }
}
