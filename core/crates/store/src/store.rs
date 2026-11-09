use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use paboable_core::catalog::ColumnMeta;
use paboable_core::format::{hex_preview, write_display};
use paboable_core::value::{Value, ValueKind};

use crate::order::{SortCell, compare};
use crate::window::{self, WindowRequest};

/// Rows per chunk. A chunk is the unit a store grows by, so appending never
/// copies rows already held.
pub const CHUNK_ROWS: usize = 65_536;

/// A chunk also closes once one column holds this much text, so a cell's
/// offset always fits a `u32`.
const MAX_CHUNK_BYTES: usize = 1 << 31;

/// One result set. Drivers append through the app's sink while grids read
/// windows; both go through one lock, and a window reads rows and the row
/// count under the same read guard, so it is one snapshot.
pub struct ResultStore {
    columns: Vec<ColumnMeta>,
    row_limit: Option<u64>,
    inner: RwLock<Inner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreState {
    pub total_rows: u64,
    pub finished: bool,
    /// The set stopped at the row limit with rows still unread.
    pub limited: bool,
}

/// A cell's full value, for copy and the detail view. A window carries
/// display text cut at its cap; this is never cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellValue {
    Null,
    Text { kind: ValueKind, text: String },
    Bytes(Vec<u8>),
}

#[derive(Default)]
pub(crate) struct Inner {
    pub(crate) chunks: Vec<Chunk>,
    /// The first row of each chunk.
    pub(crate) starts: Vec<u64>,
    pub(crate) total_rows: u64,
    pub(crate) finished: bool,
    pub(crate) limited: bool,
    /// The sorted order: display row `i` is stored row `order[i]`. Rows that
    /// arrived after the sort follow in arrival order.
    pub(crate) order: Option<Arc<Vec<u64>>>,
    pub(crate) sort_keys: Vec<SortKey>,
}

/// One key of a sort: a column, and its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    pub column: usize,
    pub descending: bool,
}

pub(crate) struct Chunk {
    pub(crate) rows: usize,
    pub(crate) columns: Vec<ColumnChunk>,
}

#[derive(Default)]
pub(crate) struct ColumnChunk {
    pub(crate) kinds: Vec<u8>,
    pub(crate) raw: Vec<i64>,
    pub(crate) nulls: Vec<u64>,
    /// `rows + 1` offsets into `data`.
    pub(crate) offsets: Vec<u32>,
    /// Display text, UTF-8, or a binary value's bytes.
    pub(crate) data: Vec<u8>,
}

impl ColumnChunk {
    fn new() -> Self {
        Self {
            offsets: vec![0],
            ..Self::default()
        }
    }

    fn push(&mut self, value: &Value, scratch: &mut String) {
        let index = self.kinds.len();
        self.kinds.push(value.kind().as_u8());
        self.raw.push(value.raw());
        if index.is_multiple_of(64) {
            self.nulls.push(0);
        }
        match value {
            Value::Null => {
                if let Some(word) = self.nulls.last_mut() {
                    *word |= 1 << (index % 64);
                }
            }
            Value::Bytes(bytes) => self.data.extend_from_slice(bytes),
            Value::Text(text) | Value::Decimal(text) | Value::Json(text) | Value::Other(text) => {
                self.data.extend_from_slice(text.as_bytes());
            }
            other => {
                scratch.clear();
                write_display(other, scratch);
                self.data.extend_from_slice(scratch.as_bytes());
            }
        }
        self.offsets.push(u32::try_from(self.data.len()).unwrap_or(u32::MAX));
    }

    pub(crate) fn is_null(&self, index: usize) -> bool {
        self.nulls
            .get(index / 64)
            .is_some_and(|word| word & (1 << (index % 64)) != 0)
    }

    pub(crate) fn bytes(&self, index: usize) -> &[u8] {
        let start = self.offsets.get(index).copied().unwrap_or(0) as usize;
        let end = self.offsets.get(index + 1).copied().unwrap_or(0) as usize;
        self.data.get(start..end).unwrap_or_default()
    }
}

impl Inner {
    /// The stored row a display row shows: itself, unless the set is sorted.
    pub(crate) fn physical(&self, row: u64) -> u64 {
        self.order
            .as_ref()
            .and_then(|order| usize::try_from(row).ok().and_then(|index| order.get(index)))
            .copied()
            .unwrap_or(row)
    }

    /// The chunk and index that hold display row `row`.
    pub(crate) fn locate_display(&self, row: u64) -> Option<(usize, usize)> {
        self.locate(self.physical(row))
    }

    /// The chunk that holds stored row `row`, and the row's index inside it.
    pub(crate) fn locate(&self, row: u64) -> Option<(usize, usize)> {
        if row >= self.total_rows {
            return None;
        }
        let chunk = self.starts.partition_point(|&start| start <= row).checked_sub(1)?;
        let local = usize::try_from(row - self.starts[chunk]).ok()?;
        Some((chunk, local))
    }

    fn open_chunk(&mut self, width: usize) -> &mut Chunk {
        let full = self.chunks.last().is_none_or(|chunk| {
            chunk.rows >= CHUNK_ROWS || chunk.columns.iter().any(|c| c.data.len() >= MAX_CHUNK_BYTES)
        });
        if full {
            self.starts.push(self.total_rows);
            self.chunks.push(Chunk {
                rows: 0,
                columns: (0..width).map(|_| ColumnChunk::new()).collect(),
            });
        }
        let last = self.chunks.len() - 1;
        &mut self.chunks[last]
    }
}

impl ResultStore {
    pub fn new(columns: Vec<ColumnMeta>, row_limit: Option<u64>) -> Self {
        Self {
            columns,
            row_limit,
            inner: RwLock::new(Inner::default()),
        }
    }

    pub fn columns(&self) -> &[ColumnMeta] {
        &self.columns
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        // A panic while appending leaves whole rows behind, so a poisoned
        // store is still readable.
        self.inner.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Appends whole rows, row after row. Returns `false` when a row past the
    /// limit was offered or the set is finished, and the driver stops reading.
    pub fn push_rows(&self, values: &[Value]) -> bool {
        let width = self.columns.len();
        if width == 0 {
            return true;
        }
        let mut inner = self.write();
        if inner.finished || inner.limited {
            return false;
        }
        let mut scratch = String::new();
        for row in values.chunks_exact(width) {
            if self.row_limit.is_some_and(|limit| inner.total_rows >= limit) {
                inner.limited = true;
                return false;
            }
            let chunk = inner.open_chunk(width);
            for (column, value) in chunk.columns.iter_mut().zip(row) {
                column.push(value, &mut scratch);
            }
            chunk.rows += 1;
            inner.total_rows += 1;
        }
        true
    }

    pub fn finish(&self) {
        self.write().finished = true;
    }

    pub fn state(&self) -> StoreState {
        let inner = self.read();
        StoreState {
            total_rows: inner.total_rows,
            finished: inner.finished,
            limited: inner.limited,
        }
    }

    /// Packs one window of display text (`window` module docs for the layout).
    pub fn fetch_window(&self, request: WindowRequest) -> Vec<u8> {
        let inner = self.read();
        window::pack(&self.columns, &inner, request)
    }

    pub fn cell(&self, row: u64, column: usize) -> Option<CellValue> {
        let inner = self.read();
        let (chunk, local) = inner.locate_display(row)?;
        let column = inner.chunks[chunk].columns.get(column)?;
        if column.is_null(local) {
            return Some(CellValue::Null);
        }
        let kind = kind_from_u8(column.kinds.get(local).copied().unwrap_or(0));
        let bytes = column.bytes(local);
        Some(if kind == ValueKind::Bytes {
            CellValue::Bytes(bytes.to_vec())
        } else {
            CellValue::Text {
                kind,
                text: String::from_utf8_lossy(bytes).into_owned(),
            }
        })
    }

    /// The text copy writes for a cell: the full value, `None` for null, and
    /// `0x` hex for bytes.
    pub fn cell_text(&self, row: u64, column: usize) -> Option<Option<String>> {
        cell_text_in(&self.read(), row, column)
    }

    /// Every value of `row`, as `cell_text` gives them, for the detail pane.
    pub fn row_texts(&self, row: u64) -> Option<Vec<Option<String>>> {
        let inner = self.read();
        (0..self.columns.len())
            .map(|column| cell_text_in(&inner, row, column))
            .collect()
    }

    /// The cells of `range` as tab-separated text, as spreadsheets paste it:
    /// rows end with CRLF, a NULL is an empty field, and a field holding a
    /// tab, a line break or a quote is quoted with its quotes doubled. With
    /// `headers`, the column names come first. `None` when the range holds
    /// more than [`MAX_COPY_CELLS`] cells.
    pub fn copy_tsv(&self, range: CellRange, headers: bool) -> Option<String> {
        let inner = self.read();
        let first_row = range.first_row.min(inner.total_rows);
        let rows = range.row_count.min(inner.total_rows - first_row);
        let first_column = range.first_column.min(self.columns.len());
        let columns = range.column_count.min(self.columns.len() - first_column);
        if rows.saturating_mul(columns as u64) > MAX_COPY_CELLS {
            return None;
        }
        let mut out = String::new();
        if headers {
            let names = self.columns[first_column..first_column + columns]
                .iter()
                .map(|column| Some(column.name.clone()));
            push_tsv_row(&mut out, names);
        }
        for row in first_row..first_row + rows {
            let cells =
                (first_column..first_column + columns).map(|column| cell_text_in(&inner, row, column).flatten());
            push_tsv_row(&mut out, cells);
        }
        Some(out)
    }
}

impl ResultStore {
    /// Sorts the rows held by `keys` (`order` module), or clears the sort
    /// when `keys` is empty. The rows stay where they are: windows read
    /// through the new order. Holds the read lock while it sorts, so rows
    /// arriving meanwhile wait, and are not in the order.
    pub fn sort(&self, keys: &[SortKey]) {
        let order = if keys.is_empty() {
            None
        } else {
            let inner = self.read();
            let keys: Vec<SortKey> = keys
                .iter()
                .copied()
                .filter(|key| key.column < self.columns.len())
                .collect();
            let cell = |row: u64, column: usize| -> SortCell<'_> {
                match inner.locate(row) {
                    Some((chunk, local)) => {
                        let data = &inner.chunks[chunk].columns[column];
                        SortCell::read(
                            data.kinds[local],
                            data.raw[local],
                            data.bytes(local),
                            data.is_null(local),
                        )
                    }
                    None => SortCell::Null,
                }
            };
            let mut rows: Vec<u64> = (0..inner.total_rows).collect();
            rows.sort_by(|&a, &b| {
                keys.iter()
                    .map(|key| {
                        let order = compare(cell(a, key.column), cell(b, key.column));
                        if key.descending { order.reverse() } else { order }
                    })
                    .find(|order| order.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            Some(Arc::new(rows))
        };
        let mut inner = self.write();
        inner.sort_keys = if order.is_some() { keys.to_vec() } else { Vec::new() };
        inner.order = order;
    }

    /// The keys the set is sorted by; empty when it shows arrival order.
    pub fn sort_keys(&self) -> Vec<SortKey> {
        self.read().sort_keys.clone()
    }
}

/// The most cells one copy takes, so selecting everything in a huge result
/// does not build gigabytes of text.
pub const MAX_COPY_CELLS: u64 = 2_000_000;

/// A rectangle of cells, rows and columns from their first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    pub first_row: u64,
    pub row_count: u64,
    pub first_column: usize,
    pub column_count: usize,
}

fn cell_text_in(inner: &Inner, row: u64, column: usize) -> Option<Option<String>> {
    let (chunk, local) = inner.locate_display(row)?;
    let data = inner.chunks[chunk].columns.get(column)?;
    if data.is_null(local) {
        return Some(None);
    }
    let bytes = data.bytes(local);
    if kind_from_u8(data.kinds.get(local).copied().unwrap_or(0)) == ValueKind::Bytes {
        let mut out = String::with_capacity(bytes.len() * 2 + 2);
        hex_preview(bytes, usize::MAX, &mut out);
        Some(Some(out))
    } else {
        Some(Some(String::from_utf8_lossy(bytes).into_owned()))
    }
}

fn push_tsv_row(out: &mut String, cells: impl Iterator<Item = Option<String>>) {
    for (index, cell) in cells.enumerate() {
        if index > 0 {
            out.push('\t');
        }
        let Some(text) = cell else { continue };
        if text.contains(['\t', '\n', '\r', '"']) {
            out.push('"');
            out.push_str(&text.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(&text);
        }
    }
    out.push_str("\r\n");
}

pub(crate) fn kind_from_u8(byte: u8) -> ValueKind {
    match byte {
        1 => ValueKind::Bool,
        2 => ValueKind::Int,
        3 => ValueKind::UInt,
        4 => ValueKind::Float,
        5 => ValueKind::Decimal,
        6 => ValueKind::Text,
        7 => ValueKind::Json,
        8 => ValueKind::Bytes,
        9 => ValueKind::Uuid,
        10 => ValueKind::Date,
        11 => ValueKind::Time,
        12 => ValueKind::Timestamp,
        13 => ValueKind::TimestampTz,
        14 => ValueKind::Other,
        15 => ValueKind::Float32,
        _ => ValueKind::Null,
    }
}
