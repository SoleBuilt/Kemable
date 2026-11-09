use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use paboable_core::catalog::ColumnMeta;
use paboable_core::format::{hex_preview, write_display};
use paboable_core::value::{Value, ValueKind};

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
    /// The chunk that holds `row`, and the row's index inside it.
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
        let (chunk, local) = inner.locate(row)?;
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
        Some(match self.cell(row, column)? {
            CellValue::Null => None,
            CellValue::Text { text, .. } => Some(text),
            CellValue::Bytes(bytes) => {
                let mut out = String::with_capacity(bytes.len() * 2 + 2);
                hex_preview(&bytes, usize::MAX, &mut out);
                Some(out)
            }
        })
    }
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
