// The data grid: one canvas over a native scroll container.
//
// The scroller holds an empty spacer as tall and wide as the data, so the
// browser does the scrolling (momentum, scrollbars, wheel) and the canvas only
// paints the rows and columns on screen. Rows come from the core as packed
// windows of BLOCK_ROWS rows; nothing per cell lives in the DOM
// (.claude/rules/data-grid.md).

import type { ColumnMeta } from "../types";
import { isNumericKind, ResultWindow } from "./window";

export const ROW_HEIGHT = 24;
export const HEADER_HEIGHT = 28;
const BLOCK_ROWS = 128;
const MAX_CHARS = 300;
const MAX_BLOCKS = 96;
const CELL_PADDING = 8;
const MIN_WIDTH = 48;
const MAX_AUTO_WIDTH = 360;
const RESIZE_HANDLE = 4;
// Chromium stops growing an element near 2^25 px; past this the scroll
// position is scaled onto the rows.
const MAX_SCROLL_PIXELS = 16_000_000;

const FONT = '12px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
const HEADER_FONT = '600 12px "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';

export interface GridSource {
  result: number;
  columns: ColumnMeta[];
}

export type FetchWindow = (
  result: number,
  firstRow: number,
  rowCount: number,
  firstColumn: number,
  columnCount: number,
  maxChars: number,
) => Promise<ArrayBuffer>;

/** A rectangle of cells, from its first row and column. */
export interface CellRect {
  firstRow: number;
  rowCount: number;
  firstColumn: number;
  columnCount: number;
}

export interface GridCallbacks {
  fetchWindow: FetchWindow;
  /** Ctrl+C copies the selection; Ctrl+Shift+C adds the column names. */
  copy: (result: number, rect: CellRect, withHeaders: boolean) => void;
  /** The cell cursor or the selection moved; null when nothing is selected. */
  selectionChanged: (result: number, focus: Cell | null, rect: CellRect | null) => void;
  /** A plain header click: sort by `column`; with Shift, as one more key. */
  sortRequested: (column: number, additive: boolean) => void;
}

/** One key of the sort the header shows. */
export interface SortKey {
  column: number;
  descending: boolean;
}

/** What a selection spans: a rectangle of cells, whole rows, whole columns, or everything. */
type SelectionMode = "cells" | "rows" | "columns" | "all";

interface Selection {
  /** Where the selection started; Shift extends from here. */
  anchor: Cell;
  /** The cell cursor: the end that moves, drawn with a border. */
  focus: Cell;
  mode: SelectionMode;
}

interface Theme {
  surface: string;
  surface2: string;
  border: string;
  text: string;
  muted: string;
  accent: string;
  selection: string;
  line: string;
  stripe: string;
}

export interface Cell {
  row: number;
  column: number;
}

type Hit =
  | { area: "header"; column: number; edge: number | null }
  | { area: "gutter"; row: number }
  | { area: "cell"; row: number; column: number }
  | { area: "corner" }
  | { area: "none" };

export class GridEngine {
  private readonly context: CanvasRenderingContext2D;
  private source: GridSource | null = null;
  private totalRows = 0;
  private generation = 0;
  private readonly blocks = new Map<number, ResultWindow>();
  private readonly pending = new Set<number>();
  private widths: number[] = [];
  private lefts: number[] = [0];
  private readonly userSized = new Set<number>();
  private autoSized = false;
  private gutterWidth = 40;
  private viewWidth = 0;
  private viewHeight = 0;
  private ratio = 1;
  private frame = 0;
  private theme: Theme;
  private selection: Selection | null = null;
  private sortKeys: SortKey[] = [];
  /** What a pointer drag is extending, while the button is down. */
  private dragging: SelectionMode | null = null;
  private resizing: { column: number; startX: number; startWidth: number } | null = null;
  private readonly colorScheme = window.matchMedia("(prefers-color-scheme: dark)");

  constructor(
    private readonly scroller: HTMLDivElement,
    private readonly spacer: HTMLDivElement,
    private readonly canvas: HTMLCanvasElement,
    private readonly callbacks: GridCallbacks,
  ) {
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) throw new Error("no 2D canvas");
    this.context = context;
    this.theme = readTheme();
    this.colorScheme.addEventListener("change", this.onThemeChange);
    scroller.addEventListener("scroll", this.scheduleDraw, { passive: true });
  }

  destroy(): void {
    cancelAnimationFrame(this.frame);
    this.colorScheme.removeEventListener("change", this.onThemeChange);
    this.scroller.removeEventListener("scroll", this.scheduleDraw);
    this.generation++;
  }

  setSource(source: GridSource | null): void {
    this.generation++;
    this.source = source;
    this.blocks.clear();
    this.pending.clear();
    this.userSized.clear();
    this.autoSized = false;
    this.selection = null;
    this.totalRows = 0;
    this.context.font = HEADER_FONT;
    this.widths = (source?.columns ?? []).map((column) =>
      clamp(this.context.measureText(column.name).width + CELL_PADDING * 2 + 8, 80, MAX_AUTO_WIDTH),
    );
    this.computeLefts();
    this.scroller.scrollTop = 0;
    this.scroller.scrollLeft = 0;
    this.updateGutter();
    this.updateSpacer();
    this.scheduleDraw();
  }

  setTotalRows(totalRows: number): void {
    if (totalRows === this.totalRows) return;
    this.totalRows = totalRows;
    this.updateGutter();
    this.updateSpacer();
    this.scheduleDraw();
  }

  resize(width: number, height: number): void {
    this.ratio = window.devicePixelRatio || 1;
    this.viewWidth = width;
    this.viewHeight = height;
    this.canvas.width = Math.round(width * this.ratio);
    this.canvas.height = Math.round(height * this.ratio);
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;
    this.updateSpacer();
    this.draw();
  }

  readonly scheduleDraw = (): void => {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.draw();
    });
  };

  private readonly onThemeChange = (): void => {
    this.theme = readTheme();
    this.scheduleDraw();
  };

  // --- geometry -----------------------------------------------------------

  private computeLefts(): void {
    const lefts = [0];
    let x = 0;
    for (const width of this.widths) {
      x += width;
      lefts.push(x);
    }
    this.lefts = lefts;
  }

  private get dataWidth(): number {
    return this.lefts[this.lefts.length - 1] ?? 0;
  }

  private get bodyHeight(): number {
    return Math.max(0, this.viewHeight - HEADER_HEIGHT);
  }

  private get rowsPixels(): number {
    return this.totalRows * ROW_HEIGHT;
  }

  private get scrollPixels(): number {
    return Math.min(this.rowsPixels, MAX_SCROLL_PIXELS);
  }

  private updateGutter(): void {
    this.context.font = FONT;
    const digits = String(Math.max(this.totalRows, 1)).length;
    this.gutterWidth = Math.max(40, Math.ceil(this.context.measureText("9".repeat(digits)).width) + CELL_PADDING * 2);
  }

  private updateSpacer(): void {
    this.spacer.style.width = `${this.gutterWidth + this.dataWidth}px`;
    this.spacer.style.height = `${HEADER_HEIGHT + this.scrollPixels}px`;
  }

  /** The pixel offset into the rows that the scroll position shows. */
  private rowsTop(): number {
    const scrollTop = this.scroller.scrollTop;
    if (this.rowsPixels <= MAX_SCROLL_PIXELS) return scrollTop;
    const range = this.scrollPixels - this.bodyHeight;
    return range > 0 ? (scrollTop * (this.rowsPixels - this.bodyHeight)) / range : 0;
  }

  private setRowsTop(pixels: number): void {
    if (this.rowsPixels <= MAX_SCROLL_PIXELS) {
      this.scroller.scrollTop = pixels;
      return;
    }
    const range = this.rowsPixels - this.bodyHeight;
    this.scroller.scrollTop = range > 0 ? (pixels * (this.scrollPixels - this.bodyHeight)) / range : 0;
  }

  /** The column at `x`, measured from the start of the data area. */
  private columnAt(x: number): number {
    let low = 0;
    let high = this.widths.length - 1;
    while (low < high) {
      const mid = (low + high + 1) >> 1;
      if ((this.lefts[mid] ?? 0) <= x) low = mid;
      else high = mid - 1;
    }
    return low;
  }

  private hit(clientX: number, clientY: number): Hit {
    const rect = this.scroller.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    if (x < 0 || y < 0 || x > this.viewWidth || y > this.viewHeight || !this.source) return { area: "none" };
    if (y < HEADER_HEIGHT && x < this.gutterWidth) return { area: "corner" };
    const dataX = x - this.gutterWidth + this.scroller.scrollLeft;
    if (y < HEADER_HEIGHT) {
      if (dataX > this.dataWidth + RESIZE_HANDLE) return { area: "none" };
      const column = this.columnAt(dataX);
      const right = this.lefts[column + 1] ?? 0;
      const left = this.lefts[column] ?? 0;
      let edge: number | null = null;
      if (Math.abs(right - dataX) <= RESIZE_HANDLE) edge = column;
      else if (column > 0 && Math.abs(dataX - left) <= RESIZE_HANDLE) edge = column - 1;
      return { area: "header", column, edge };
    }
    const row = Math.floor((this.rowsTop() + y - HEADER_HEIGHT) / ROW_HEIGHT);
    if (row >= this.totalRows) return { area: "none" };
    if (x < this.gutterWidth) return { area: "gutter", row };
    if (dataX >= this.dataWidth) return { area: "none" };
    return { area: "cell", row, column: this.columnAt(dataX) };
  }

  // --- data ---------------------------------------------------------------

  private ensureBlocks(firstRow: number, endRow: number): void {
    const source = this.source;
    if (!source || this.totalRows === 0) return;
    // One block of read-ahead each way, so a scroll rarely shows an empty row.
    const first = Math.max(0, Math.floor(firstRow / BLOCK_ROWS) - 1);
    const last = Math.floor(Math.max(endRow - 1, 0) / BLOCK_ROWS) + 1;
    for (let block = first; block <= last; block++) {
      const start = block * BLOCK_ROWS;
      if (start >= this.totalRows) break;
      const window = this.blocks.get(block);
      if (window) {
        // Re-insert to mark it recently used.
        this.blocks.delete(block);
        this.blocks.set(block, window);
      }
      const wanted = Math.min(BLOCK_ROWS, this.totalRows - start);
      // A block read while rows were still arriving is read again once more came.
      if ((!window || window.rowCount < wanted) && !this.pending.has(block)) this.request(source, block);
    }
  }

  private request(source: GridSource, block: number): void {
    const generation = this.generation;
    this.pending.add(block);
    this.callbacks
      .fetchWindow(source.result, block * BLOCK_ROWS, BLOCK_ROWS, 0, source.columns.length, MAX_CHARS)
      .then((buffer) => {
        if (generation !== this.generation) return;
        this.pending.delete(block);
        const window = new ResultWindow(buffer);
        this.blocks.set(block, window);
        while (this.blocks.size > MAX_BLOCKS) {
          const oldest = this.blocks.keys().next().value;
          if (oldest === undefined) break;
          this.blocks.delete(oldest);
        }
        if (!this.autoSized && block === 0) this.autoSize(window);
        this.scheduleDraw();
      })
      .catch(() => {
        if (generation === this.generation) this.pending.delete(block);
      });
  }

  /** Widens each column to the first rows' text, until the user sizes it. */
  private autoSize(window: ResultWindow): void {
    if (!this.source) return;
    this.autoSized = true;
    const context = this.context;
    this.source.columns.forEach((column, index) => {
      if (this.userSized.has(index)) return;
      context.font = HEADER_FONT;
      let width = context.measureText(column.name).width + CELL_PADDING * 2 + 8;
      context.font = FONT;
      const rows = Math.min(window.rowCount, 64);
      for (let row = 0; row < rows; row++) {
        const absolute = window.firstRow + row;
        if (window.isNull(absolute, index)) continue;
        width = Math.max(width, context.measureText(window.text(absolute, index)).width + CELL_PADDING * 2);
      }
      this.widths[index] = clamp(Math.ceil(width), MIN_WIDTH, MAX_AUTO_WIDTH);
    });
    this.computeLefts();
    this.updateSpacer();
  }

  private windowFor(row: number): ResultWindow | undefined {
    const window = this.blocks.get(Math.floor(row / BLOCK_ROWS));
    return window?.has(row) ? window : undefined;
  }

  // --- painting -----------------------------------------------------------

  private draw(): void {
    const context = this.context;
    const theme = this.theme;
    const width = this.viewWidth;
    const height = this.viewHeight;
    context.setTransform(this.ratio, 0, 0, this.ratio, 0, 0);
    context.fillStyle = theme.surface;
    context.fillRect(0, 0, width, height);
    const source = this.source;
    if (!source || width === 0) return;

    const rowsTop = this.rowsTop();
    const firstRow = Math.floor(rowsTop / ROW_HEIGHT);
    const firstY = HEADER_HEIGHT - (rowsTop - firstRow * ROW_HEIGHT);
    const endRow = Math.min(this.totalRows, firstRow + Math.ceil(this.bodyHeight / ROW_HEIGHT) + 1);
    const scrollLeft = this.scroller.scrollLeft;
    const originX = this.gutterWidth - scrollLeft;
    const firstColumn = this.columnAt(scrollLeft);
    let endColumn = firstColumn;
    while (endColumn < this.widths.length && (this.lefts[endColumn] ?? 0) - scrollLeft < width - this.gutterWidth) {
      endColumn++;
    }
    this.ensureBlocks(firstRow, endRow);

    // Stripes.
    context.fillStyle = theme.stripe;
    for (let row = firstRow; row < endRow; row++) {
      if (row % 2 === 1) context.fillRect(this.gutterWidth, firstY + (row - firstRow) * ROW_HEIGHT, width, ROW_HEIGHT);
    }

    // The selection, clipped to the body: only its visible part is filled.
    const rect = this.selectionRect();
    if (rect) {
      const top = Math.max(rect.firstRow, firstRow);
      const bottom = Math.min(rect.firstRow + rect.rowCount, endRow);
      if (bottom > top) {
        const left = Math.max(originX + (this.lefts[rect.firstColumn] ?? 0), this.gutterWidth);
        const right = originX + (this.lefts[rect.firstColumn + rect.columnCount] ?? 0);
        context.fillStyle = theme.selection;
        context.fillRect(left, firstY + (top - firstRow) * ROW_HEIGHT, right - left, (bottom - top) * ROW_HEIGHT);
      }
    }

    // Cells, one clip per column.
    context.font = FONT;
    context.textBaseline = "middle";
    for (let column = firstColumn; column < endColumn; column++) {
      const x = originX + (this.lefts[column] ?? 0);
      const columnWidth = this.widths[column] ?? 0;
      context.save();
      context.beginPath();
      const clipLeft = Math.max(x, this.gutterWidth);
      context.rect(clipLeft, HEADER_HEIGHT, x + columnWidth - CELL_PADDING / 2 - clipLeft, this.bodyHeight);
      context.clip();
      for (let row = firstRow; row < endRow; row++) {
        const window = this.windowFor(row);
        if (!window) continue;
        const y = firstY + (row - firstRow) * ROW_HEIGHT + ROW_HEIGHT / 2;
        if (window.isNull(row, column)) {
          context.fillStyle = theme.muted;
          context.textAlign = "left";
          context.fillText("NULL", x + CELL_PADDING, y);
          continue;
        }
        context.fillStyle = theme.text;
        if (isNumericKind(window.kind(row, column))) {
          context.textAlign = "right";
          context.fillText(window.text(row, column), x + columnWidth - CELL_PADDING, y);
        } else {
          context.textAlign = "left";
          context.fillText(window.text(row, column), x + CELL_PADDING, y);
        }
      }
      context.restore();
    }

    // Grid lines.
    context.strokeStyle = theme.line;
    context.lineWidth = 1;
    context.beginPath();
    for (let column = firstColumn; column < endColumn; column++) {
      const x = Math.round(originX + (this.lefts[column + 1] ?? 0)) - 0.5;
      if (x > this.gutterWidth) {
        context.moveTo(x, HEADER_HEIGHT);
        context.lineTo(x, height);
      }
    }
    const rightEdge = Math.min(width, originX + this.dataWidth);
    for (let row = firstRow; row < endRow; row++) {
      const y = Math.round(firstY + (row - firstRow + 1) * ROW_HEIGHT) - 0.5;
      context.moveTo(this.gutterWidth, y);
      context.lineTo(rightEdge, y);
    }
    context.stroke();

    // The cell cursor.
    const focus = this.selection?.focus;
    if (focus && focus.row >= firstRow && focus.row < endRow) {
      const x = originX + (this.lefts[focus.column] ?? 0);
      const y = firstY + (focus.row - firstRow) * ROW_HEIGHT;
      context.save();
      context.beginPath();
      context.rect(this.gutterWidth, HEADER_HEIGHT, width, this.bodyHeight);
      context.clip();
      context.strokeStyle = theme.accent;
      context.lineWidth = 2;
      context.strokeRect(x + 1, y + 1, (this.widths[focus.column] ?? 0) - 2, ROW_HEIGHT - 2);
      context.restore();
    }

    // Row numbers, marked where the selection spans the row.
    context.fillStyle = theme.surface2;
    context.fillRect(0, HEADER_HEIGHT, this.gutterWidth, height);
    context.textAlign = "right";
    for (let row = firstRow; row < endRow; row++) {
      const y = firstY + (row - firstRow) * ROW_HEIGHT;
      const selected = rect !== null && row >= rect.firstRow && row < rect.firstRow + rect.rowCount;
      if (selected) {
        context.fillStyle = theme.selection;
        context.fillRect(0, y, this.gutterWidth, ROW_HEIGHT);
      }
      if (y + ROW_HEIGHT / 2 < HEADER_HEIGHT) continue;
      context.fillStyle = selected ? theme.text : theme.muted;
      context.fillText(String(row + 1), this.gutterWidth - CELL_PADDING, y + ROW_HEIGHT / 2);
    }

    // Header, marked where the selection spans the column.
    context.fillStyle = theme.surface2;
    context.fillRect(0, 0, width, HEADER_HEIGHT);
    context.font = HEADER_FONT;
    context.textAlign = "left";
    for (let column = firstColumn; column < endColumn; column++) {
      const x = originX + (this.lefts[column] ?? 0);
      const columnWidth = this.widths[column] ?? 0;
      context.save();
      context.beginPath();
      const clipLeft = Math.max(x, this.gutterWidth);
      context.rect(clipLeft, 0, x + columnWidth - clipLeft, HEADER_HEIGHT);
      context.clip();
      if (rect && column >= rect.firstColumn && column < rect.firstColumn + rect.columnCount) {
        context.fillStyle = theme.selection;
        context.fillRect(x, 0, columnWidth, HEADER_HEIGHT);
      }
      context.beginPath();
      context.rect(clipLeft, 0, x + columnWidth - CELL_PADDING / 2 - clipLeft, HEADER_HEIGHT);
      context.clip();
      context.fillStyle = theme.text;
      context.fillText(source.columns[column]?.name ?? "", x + CELL_PADDING, HEADER_HEIGHT / 2);
      const keyIndex = this.sortKeys.findIndex((key) => key.column === column);
      const key = this.sortKeys[keyIndex];
      if (key) {
        const label = `${key.descending ? "▼" : "▲"}${this.sortKeys.length > 1 ? keyIndex + 1 : ""}`;
        const labelWidth = context.measureText(label).width + CELL_PADDING;
        context.fillStyle = theme.surface2;
        context.fillRect(x + columnWidth - labelWidth - CELL_PADDING / 2, 1, labelWidth + CELL_PADDING / 2, HEADER_HEIGHT - 2);
        context.fillStyle = theme.accent;
        context.textAlign = "right";
        context.fillText(label, x + columnWidth - CELL_PADDING, HEADER_HEIGHT / 2);
        context.textAlign = "left";
      }
      context.restore();
    }
    context.strokeStyle = theme.border;
    context.beginPath();
    for (let column = firstColumn; column < endColumn; column++) {
      const x = Math.round(originX + (this.lefts[column + 1] ?? 0)) - 0.5;
      if (x > this.gutterWidth) {
        context.moveTo(x, 6);
        context.lineTo(x, HEADER_HEIGHT - 6);
      }
    }
    context.moveTo(0, HEADER_HEIGHT - 0.5);
    context.lineTo(width, HEADER_HEIGHT - 0.5);
    context.moveTo(this.gutterWidth - 0.5, 0);
    context.lineTo(this.gutterWidth - 0.5, height);
    context.stroke();
  }

  // --- input --------------------------------------------------------------

  pointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    const hit = this.hit(event.clientX, event.clientY);
    if (hit.area === "header" && hit.edge !== null) {
      this.resizing = { column: hit.edge, startX: event.clientX, startWidth: this.widths[hit.edge] ?? 0 };
      this.scroller.setPointerCapture(event.pointerId);
      event.preventDefault();
      return;
    }
    const extend = event.shiftKey && this.selection !== null;
    const ctrl = event.ctrlKey || event.metaKey;
    let mode: SelectionMode | null = null;
    let cell: Cell | null = null;
    if (hit.area === "cell") {
      mode = "cells";
      cell = { row: hit.row, column: hit.column };
    } else if (hit.area === "gutter") {
      mode = "rows";
      cell = { row: hit.row, column: this.selection?.focus.column ?? 0 };
    } else if (hit.area === "header" && ctrl) {
      mode = "columns";
      cell = { row: this.selection?.focus.row ?? 0, column: hit.column };
    } else if (hit.area === "header" && this.source) {
      // docs/decisions/0005-sorting-a-result.md: the owner decides how.
      this.callbacks.sortRequested(hit.column, event.shiftKey);
      return;
    } else if (hit.area === "corner") {
      this.select({ anchor: { row: 0, column: 0 }, focus: { row: 0, column: 0 }, mode: "all" });
      return;
    }
    if (mode === null || cell === null || this.totalRows === 0 && mode !== "columns") return;
    const anchor = extend && this.selection ? this.selection.anchor : cell;
    this.select({ anchor, focus: cell, mode });
    this.dragging = mode;
    this.scroller.setPointerCapture(event.pointerId);
  }

  pointerMove(event: PointerEvent): void {
    if (this.resizing) {
      const { column, startX, startWidth } = this.resizing;
      this.widths[column] = Math.max(MIN_WIDTH, startWidth + event.clientX - startX);
      this.userSized.add(column);
      this.computeLefts();
      this.updateSpacer();
      this.scheduleDraw();
      return;
    }
    if (this.dragging && this.selection) {
      this.autoScroll(event.clientX, event.clientY);
      const cell = this.cellNear(event.clientX, event.clientY);
      const focus =
        this.dragging === "rows"
          ? { row: cell.row, column: this.selection.focus.column }
          : this.dragging === "columns"
            ? { row: this.selection.focus.row, column: cell.column }
            : cell;
      if (focus.row !== this.selection.focus.row || focus.column !== this.selection.focus.column) {
        this.select({ ...this.selection, focus });
      }
      return;
    }
    const hit = this.hit(event.clientX, event.clientY);
    this.scroller.style.cursor = hit.area === "header" && hit.edge !== null ? "col-resize" : "";
  }

  pointerUp(event: PointerEvent): void {
    if (!this.resizing && !this.dragging) return;
    this.resizing = null;
    this.dragging = null;
    this.scroller.releasePointerCapture(event.pointerId);
  }

  /** The rectangle the selection covers, cut to the data. */
  private selectionRect(): CellRect | null {
    const selection = this.selection;
    const columns = this.source?.columns.length ?? 0;
    if (!selection || columns === 0) return null;
    const { anchor, focus, mode } = selection;
    const rows = mode === "columns" || mode === "all" ? { first: 0, count: this.totalRows } : span(anchor.row, focus.row);
    const cols = mode === "rows" || mode === "all" ? { first: 0, count: columns } : span(anchor.column, focus.column);
    const lastRow = Math.min(rows.first + rows.count, this.totalRows);
    const lastColumn = Math.min(cols.first + cols.count, columns);
    if (lastRow <= rows.first || lastColumn <= cols.first) return null;
    return {
      firstRow: rows.first,
      rowCount: lastRow - rows.first,
      firstColumn: cols.first,
      columnCount: lastColumn - cols.first,
    };
  }

  private select(selection: Selection | null): void {
    this.selection = selection;
    if (this.source) this.callbacks.selectionChanged(this.source.result, selection?.focus ?? null, this.selectionRect());
    this.scheduleDraw();
  }

  /** The cell under a point, or the nearest one when the point is outside the data. */
  private cellNear(clientX: number, clientY: number): Cell {
    const rect = this.scroller.getBoundingClientRect();
    const dataX = clientX - rect.left - this.gutterWidth + this.scroller.scrollLeft;
    const row = Math.floor((this.rowsTop() + clientY - rect.top - HEADER_HEIGHT) / ROW_HEIGHT);
    return {
      row: clamp(row, 0, Math.max(0, this.totalRows - 1)),
      column: this.columnAt(clamp(dataX, 0, Math.max(0, this.dataWidth - 1))),
    };
  }

  /** Scrolls while a drag holds the pointer past an edge of the body. */
  private autoScroll(clientX: number, clientY: number): void {
    const rect = this.scroller.getBoundingClientRect();
    const below = clientY - (rect.top + this.viewHeight);
    const above = rect.top + HEADER_HEIGHT - clientY;
    if (below > 0) this.setRowsTop(this.rowsTop() + Math.min(below, ROW_HEIGHT * 3));
    else if (above > 0) this.setRowsTop(Math.max(0, this.rowsTop() - Math.min(above, ROW_HEIGHT * 3)));
    const right = clientX - (rect.left + this.viewWidth);
    const left = rect.left + this.gutterWidth - clientX;
    if (right > 0) this.scroller.scrollLeft += Math.min(right, 60);
    else if (left > 0) this.scroller.scrollLeft -= Math.min(left, 60);
  }

  keyDown(event: KeyboardEvent): void {
    const source = this.source;
    if (!source || this.totalRows === 0 || source.columns.length === 0) return;
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();
    if (ctrl && key === "c") {
      const rect = this.selectionRect();
      if (rect) this.callbacks.copy(source.result, rect, event.shiftKey);
      event.preventDefault();
      return;
    }
    if (ctrl && key === "a") {
      const focus = this.selection?.focus ?? { row: 0, column: 0 };
      this.select({ anchor: { row: 0, column: 0 }, focus, mode: "all" });
      event.preventDefault();
      return;
    }
    if (event.key === "Escape" && this.selection) {
      const focus = this.selection.focus;
      this.select({ anchor: focus, focus, mode: "cells" });
      event.preventDefault();
      return;
    }
    const page = Math.max(1, Math.floor(this.bodyHeight / ROW_HEIGHT) - 1);
    const lastRow = this.totalRows - 1;
    const lastColumn = source.columns.length - 1;
    const current = this.selection?.focus ?? { row: 0, column: 0 };
    let { row, column } = current;
    switch (event.key) {
      case "ArrowDown":
        row = ctrl ? lastRow : row + 1;
        break;
      case "ArrowUp":
        row = ctrl ? 0 : row - 1;
        break;
      case "ArrowRight":
        column = ctrl ? lastColumn : column + 1;
        break;
      case "ArrowLeft":
        column = ctrl ? 0 : column - 1;
        break;
      case "Tab":
        column = event.shiftKey ? column - 1 : column + 1;
        break;
      case "PageDown":
        row += page;
        break;
      case "PageUp":
        row -= page;
        break;
      case "Home":
        if (ctrl) row = 0;
        column = 0;
        break;
      case "End":
        if (ctrl) row = lastRow;
        column = lastColumn;
        break;
      default:
        return;
    }
    event.preventDefault();
    const focus = { row: clamp(row, 0, lastRow), column: clamp(column, 0, lastColumn) };
    // Shift with an arrow or page key extends; Tab and a plain move collapse to the cursor.
    if (event.shiftKey && event.key !== "Tab" && this.selection) {
      const mode = this.selection.mode === "all" ? "cells" : this.selection.mode;
      this.select({ anchor: this.selection.anchor, focus, mode });
    } else {
      this.select({ anchor: focus, focus, mode: "cells" });
    }
    this.reveal(focus);
  }

  /** The sort the header shows; drawing only, the rows come in order from the core. */
  setSortKeys(keys: SortKey[]): void {
    this.sortKeys = keys;
    this.scheduleDraw();
  }

  /** Reads the rows again (after a sort in the core), keeping widths, scroll and selection. */
  reload(): void {
    this.generation++;
    this.blocks.clear();
    this.pending.clear();
    this.scheduleDraw();
  }

  /** Moves the cell cursor to `cell`, as a click there would, and scrolls it into view. */
  focusCell(cell: Cell): void {
    if (!this.source || this.totalRows === 0) return;
    const target = {
      row: clamp(cell.row, 0, this.totalRows - 1),
      column: clamp(cell.column, 0, this.source.columns.length - 1),
    };
    this.select({ anchor: target, focus: target, mode: "cells" });
    this.reveal(target);
  }

  private reveal(cell: Cell): void {
    const top = cell.row * ROW_HEIGHT;
    const rowsTop = this.rowsTop();
    if (top < rowsTop) this.setRowsTop(top);
    else if (top + ROW_HEIGHT > rowsTop + this.bodyHeight) this.setRowsTop(top + ROW_HEIGHT - this.bodyHeight);

    const left = this.lefts[cell.column] ?? 0;
    const right = this.lefts[cell.column + 1] ?? left;
    const visible = this.viewWidth - this.gutterWidth;
    if (left < this.scroller.scrollLeft) this.scroller.scrollLeft = left;
    else if (right > this.scroller.scrollLeft + visible) this.scroller.scrollLeft = right - visible;
  }
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

function readTheme(): Theme {
  const style = getComputedStyle(document.documentElement);
  const read = (name: string) => style.getPropertyValue(name).trim();
  return {
    surface: read("--surface"),
    surface2: read("--surface-2"),
    border: read("--border"),
    text: read("--text"),
    muted: read("--text-muted"),
    accent: read("--accent"),
    selection: read("--selection"),
    line: read("--grid-line"),
    stripe: read("--grid-stripe"),
  };
}

/** The rows or columns between two ends, in either order. */
function span(a: number, b: number): { first: number; count: number } {
  return { first: Math.min(a, b), count: Math.abs(a - b) + 1 };
}
