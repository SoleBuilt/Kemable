use std::sync::{Arc, Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use paboable_core::DriverError;
use paboable_core::catalog::ColumnMeta;
use paboable_core::format::{hex_preview, write_display};
use paboable_core::sql::ResultFilter;
use paboable_core::value::{Value, ValueKind};

use crate::filter::{RowFilter, Scratch};
use crate::order::{
    KeyBuilder, Keyed, NARROW_KEY, SortCell, WIDE_KEY, compare, key_order, radix_sort, shared_start, skippable,
    wants_wide_key,
};
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
    /// Held while a filter or a sort builds its row order, so two of them
    /// never build from each other's half.
    arranging: Mutex<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreState {
    /// The rows the set holds.
    pub total_rows: u64,
    /// The rows windows show: those a filter keeps, else every row held.
    pub shown_rows: u64,
    /// A filter is on (`ResultStore::filter`).
    pub filtered: bool,
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

impl CellValue {
    /// The value again, as far as a SQL literal needs it: numbers, booleans
    /// and decimals come back as themselves; kinds the store keeps only as text
    /// (UUIDs, dates, times) come back as that text, which a literal writes the
    /// same way (`paboable_core::sql::value_literal`).
    pub fn into_value(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bytes(bytes) => Value::Bytes(bytes),
            Self::Text { kind, text } => match kind {
                ValueKind::Bool => Value::Bool(text == "true"),
                ValueKind::Int => text.parse().map_or(Value::Decimal(text), Value::Int),
                ValueKind::UInt => text.parse().map_or(Value::Decimal(text), Value::UInt),
                ValueKind::Float | ValueKind::Float32 => match text.parse::<f64>() {
                    Ok(number) if number.is_finite() => Value::Float(number),
                    _ => Value::Text(text),
                },
                ValueKind::Decimal => Value::Decimal(text),
                ValueKind::Json => Value::Json(text),
                _ => Value::Text(text),
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct Inner {
    pub(crate) chunks: Vec<Chunk>,
    /// The first row of each chunk.
    pub(crate) starts: Vec<u64>,
    pub(crate) total_rows: u64,
    pub(crate) finished: bool,
    pub(crate) limited: bool,
    /// The rows shown, in order: display row `i` is stored row `order[i]`.
    /// Without a filter, rows that arrived after the sort follow in arrival
    /// order; with one, it lists every row shown, the late ones that match
    /// appended as they arrive.
    pub(crate) order: Option<Vec<u64>>,
    pub(crate) sort_keys: Vec<SortKey>,
    /// Set only together with `order`.
    pub(crate) filter: Option<Arc<RowFilter>>,
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
    /// How many rows windows show.
    pub(crate) fn shown_rows(&self) -> u64 {
        match (&self.filter, &self.order) {
            (Some(_), Some(order)) => order.len() as u64,
            _ => self.total_rows,
        }
    }

    /// The stored row a display row shows: itself, unless the set is sorted
    /// or filtered. `None` past the rows shown.
    pub(crate) fn physical(&self, row: u64) -> Option<u64> {
        if row >= self.shown_rows() {
            return None;
        }
        let ordered = self
            .order
            .as_ref()
            .and_then(|order| usize::try_from(row).ok().and_then(|index| order.get(index)));
        Some(ordered.copied().unwrap_or(row))
    }

    /// The chunk and index that hold display row `row`.
    pub(crate) fn locate_display(&self, row: u64) -> Option<(usize, usize)> {
        self.locate(self.physical(row)?)
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
            arranging: Mutex::new(()),
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
        // Rows arriving under a filter are tested as they come, so a filter
        // set while a result streams keeps applying to it.
        let filter = inner.filter.clone();
        let mut tests = Scratch::default();
        for row in values.chunks_exact(width) {
            if self.row_limit.is_some_and(|limit| inner.total_rows >= limit) {
                inner.limited = true;
                return false;
            }
            let chunk = inner.open_chunk(width);
            for (column, value) in chunk.columns.iter_mut().zip(row) {
                column.push(value, &mut scratch);
            }
            let local = chunk.rows;
            chunk.rows += 1;
            let kept = filter
                .as_ref()
                .is_some_and(|filter| filter.matches(chunk, local, &mut tests));
            let stored = inner.total_rows;
            inner.total_rows += 1;
            if kept && let Some(order) = inner.order.as_mut() {
                order.push(stored);
            }
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
            shown_rows: inner.shown_rows(),
            filtered: inner.filter.is_some(),
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
        let rows = [RowSpan {
            first_row: range.first_row,
            row_count: range.row_count,
        }];
        self.copy_tsv_rows(&rows, range.first_column, range.column_count, headers)
    }

    /// The same columns of several spans of rows, one span after the other in
    /// the order given, as [`copy_tsv`](Self::copy_tsv) writes them: the rows
    /// picked one by one in the grid (Ctrl+click). The column names, with
    /// `headers`, come once. `None` when the spans hold more than
    /// [`MAX_COPY_CELLS`] cells in all.
    pub fn copy_tsv_rows(
        &self,
        rows: &[RowSpan],
        first_column: usize,
        column_count: usize,
        headers: bool,
    ) -> Option<String> {
        let inner = self.read();
        let shown = inner.shown_rows();
        let first_column = first_column.min(self.columns.len());
        let columns = column_count.min(self.columns.len() - first_column);
        let spans: Vec<(u64, u64)> = rows
            .iter()
            .map(|span| {
                let first = span.first_row.min(shown);
                (first, span.row_count.min(shown - first))
            })
            .collect();
        let cells = spans.iter().fold(0u64, |total, (_, count)| {
            total.saturating_add(count.saturating_mul(columns as u64))
        });
        if cells > MAX_COPY_CELLS {
            return None;
        }
        let mut out = String::new();
        if headers {
            let names = self.columns[first_column..first_column + columns]
                .iter()
                .map(|column| Some(column.name.clone()));
            push_tsv_row(&mut out, names);
        }
        for (first, count) in spans {
            for row in first..first + count {
                let cells =
                    (first_column..first_column + columns).map(|column| cell_text_in(&inner, row, column).flatten());
                push_tsv_row(&mut out, cells);
            }
        }
        Some(out)
    }
}

impl ResultStore {
    /// Sorts the rows shown by `keys` (`order` module), or clears the sort
    /// when `keys` is empty. With a filter on, it sorts the rows the filter
    /// keeps. The rows stay where they are: windows read through the new
    /// order. Rows arriving while it sorts are not in the sort: they follow
    /// in arrival order.
    pub fn sort(&self, keys: &[SortKey]) {
        self.arrange(None, Some(keys.to_vec()));
    }

    /// The keys the set is sorted by; empty when it shows arrival order.
    pub fn sort_keys(&self) -> Vec<SortKey> {
        self.read().sort_keys.clone()
    }

    /// Shows only the rows held that match `filters`, all of them or, with
    /// `match_any`, any (`filter` module for how a value compares), then
    /// sorts them by the sort already set. No filters shows every row again,
    /// still sorted. Rows arriving later are tested as they arrive. Refused
    /// with `filter.raw` or `filter.invalid`, the filter in place unchanged
    /// (`paboable_core::sql::ResultFilter::check`).
    pub fn filter(&self, filters: &[ResultFilter], match_any: bool) -> Result<(), DriverError> {
        let filter = RowFilter::compile(filters, match_any, self.columns.len())?;
        self.arrange(Some(filter.map(Arc::new)), None);
        Ok(())
    }

    /// Builds and sets the row order for a filter and a sort; `None` keeps
    /// the one set. The order is built under the read lock, so windows keep
    /// reading meanwhile, and set under the write lock with the rows that
    /// arrived in between tested then.
    fn arrange(&self, filter: Option<Option<Arc<RowFilter>>>, keys: Option<Vec<SortKey>>) {
        let _arranging = self.arranging.lock().unwrap_or_else(PoisonError::into_inner);
        let (filter, keys, mut order, covered) = {
            let inner = self.read();
            let filter = filter.unwrap_or_else(|| inner.filter.clone());
            let keys = keys.unwrap_or_else(|| inner.sort_keys.clone());
            let mut rows = match &filter {
                Some(filter) => kept_rows(&inner, filter),
                None if keys.is_empty() => Vec::new(),
                None => (0..inner.total_rows).collect(),
            };
            sort_rows(&inner, &mut rows, &keys, self.columns.len());
            let order = (filter.is_some() || !keys.is_empty()).then_some(rows);
            (filter, keys, order, inner.total_rows)
        };
        let mut inner = self.write();
        if let (Some(filter), Some(order)) = (&filter, order.as_mut()) {
            let mut scratch = Scratch::default();
            for stored in covered..inner.total_rows {
                if let Some((chunk, local)) = inner.locate(stored)
                    && filter.matches(&inner.chunks[chunk], local, &mut scratch)
                {
                    order.push(stored);
                }
            }
        }
        inner.order = order;
        inner.filter = filter;
        inner.sort_keys = keys;
    }
}

/// The stored rows `filter` keeps, in arrival order.
fn kept_rows(inner: &Inner, filter: &RowFilter) -> Vec<u64> {
    let mut rows = Vec::new();
    let mut scratch = Scratch::default();
    for (chunk, start) in inner.chunks.iter().zip(&inner.starts) {
        for local in 0..chunk.rows {
            if filter.matches(chunk, local, &mut scratch) {
                rows.push(start + local as u64);
            }
        }
    }
    rows
}

/// Sorts stored rows by `keys`, stably; keys past the last column are skipped.
fn sort_rows(inner: &Inner, rows: &mut [u64], keys: &[SortKey], width: usize) {
    let keys: Vec<SortKey> = keys.iter().copied().filter(|key| key.column < width).collect();
    // A column that holds text or bytes anywhere takes the wide key.
    let wide = keys
        .iter()
        .map(|key| {
            inner.chunks.iter().any(|chunk| {
                chunk
                    .columns
                    .get(key.column)
                    .is_some_and(|data| data.kinds.iter().any(|&kind| wants_wide_key(kind)))
            })
        })
        .collect();
    RowSort { inner, keys, wide }.sort(rows, 0, 0);
}

/// Below this many rows a sort compares cells at once: keys would cost more
/// to build than they save.
const FEW_ROWS: usize = 32;
/// From this many rows on, narrow keys are dealt out by the radix sort; fewer
/// are quicker to compare than to deal.
const RADIX_FROM: usize = 1024;

/// A sort of stored rows, one sort column at a time (`order.rs`, Sort keys).
///
/// The rows are sorted by keys of the first column, which reads each cell
/// once. Rows whose keys are the same form a run, and each run is settled by
/// itself: when the cells are equal, by the next sort column, in the same
/// way; when they are texts alike past the key's width, by keys of what
/// follows the start they share. Only what keys cannot say is left to a
/// comparison of the cells.
struct RowSort<'a> {
    inner: &'a Inner,
    keys: Vec<SortKey>,
    /// For each sort column, whether it takes the wide key.
    wide: Vec<bool>,
}

impl<'a> RowSort<'a> {
    fn cell(&self, row: u64, column: usize) -> SortCell<'a> {
        let Some((chunk, local)) = self.inner.locate(row) else {
            return SortCell::Null;
        };
        match self.inner.chunks.get(chunk).and_then(|chunk| chunk.columns.get(column)) {
            Some(data) => SortCell::read(
                data.kinds.get(local).copied().unwrap_or(0),
                data.raw.get(local).copied().unwrap_or(0),
                data.bytes(local),
                data.is_null(local),
            ),
            None => SortCell::Null,
        }
    }

    /// The order of two rows by the sort columns from `level` on.
    fn by_cells(&self, a: u64, b: u64, level: usize) -> std::cmp::Ordering {
        self.keys
            .iter()
            .skip(level)
            .map(|key| {
                let order = compare(self.cell(a, key.column), self.cell(b, key.column));
                if key.descending { order.reverse() } else { order }
            })
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    }

    /// Sorts `rows`, which are equal in every sort column before `level`, by
    /// the columns from `level` on. Their texts in column `level` are alike up
    /// to byte `past`, so keys are read from there.
    fn sort(&self, rows: &mut [u64], level: usize, past: usize) {
        if level >= self.keys.len() || rows.len() < 2 {
            return;
        }
        if rows.len() < FEW_ROWS || u32::try_from(rows.len()).is_err() {
            rows.sort_by(|&a, &b| self.by_cells(a, b, level));
        } else if self.wide.get(level) == Some(&true) {
            self.sort_keyed::<WIDE_KEY>(rows, level, past);
        } else {
            self.sort_keyed::<NARROW_KEY>(rows, level, past);
        }
    }

    fn sort_keyed<const N: usize>(&self, rows: &mut [u64], level: usize, past: usize) {
        let Some(&SortKey { column, descending }) = self.keys.get(level) else {
            return;
        };
        let mut builder = KeyBuilder::default();
        let mut keyed: Vec<Keyed<N>> = rows
            .iter()
            .zip(0u32..)
            .map(|(&row, place)| Keyed {
                key: builder.key(self.cell(row, column), past),
                place,
            })
            .collect();
        for entry in &mut keyed {
            builder.settle(&mut entry.key, descending);
        }
        // By key alone, which reads no cell. Equal keys stay in the order they
        // came: by the radix sort's nature, or by `place`.
        if N == NARROW_KEY && keyed.len() >= RADIX_FROM {
            radix_sort(&mut keyed);
        } else {
            keyed.sort_unstable_by(|a, b| key_order(&a.key, &b.key).then(a.place.cmp(&b.place)));
        }
        let sorted: Vec<u64> = keyed
            .iter()
            .map(|entry| rows.get(entry.place as usize).copied().unwrap_or(0))
            .collect();
        rows.copy_from_slice(&sorted);

        let mut start = 0;
        while let Some(head) = keyed.get(start) {
            let run = keyed[start..].iter().take_while(|entry| entry.key == head.key).count();
            if run > 1
                && let Some(alike) = rows.get_mut(start..start + run)
            {
                self.settle(alike, level, past, &builder);
            }
            start += run;
        }
    }

    /// Orders rows whose keys in column `level` are the same.
    fn settle(&self, alike: &mut [u64], level: usize, past: usize, builder: &KeyBuilder) {
        let (Some(key), Some(&first)) = (self.keys.get(level), alike.first()) else {
            return;
        };
        let head = self.cell(first, key.column);
        if builder.whole(head) {
            // The key was the whole cell, so the cells are equal: the next column orders them.
            return self.sort(alike, level + 1, 0);
        }
        if let SortCell::Text(head) = head {
            let head = head.as_bytes();
            let rest = |text: &'a [u8]| text.get(past..).unwrap_or_default();
            // How far every text of the run is the same, and whether they all are, whole.
            let mut shared = head.len();
            let mut same = true;
            for &row in alike.iter().skip(1) {
                let SortCell::Text(text) = self.cell(row, key.column) else {
                    (shared, same) = (past, false);
                    break;
                };
                let text = text.as_bytes();
                let common = past + shared_start(rest(head), rest(text));
                same &= text.len() == head.len() && common == head.len();
                shared = shared.min(common);
                if !same && shared <= past {
                    break;
                }
            }
            if same {
                return self.sort(alike, level + 1, 0);
            }
            // Alike past the key's width: key them again from what follows the shared start.
            let further = skippable(head, shared, past);
            if further > past {
                return self.sort(alike, level, further);
            }
        }
        // What a key cannot say: numbers of several kinds, bytes, digits past a key's width.
        alike.sort_by(|&a, &b| self.by_cells(a, b, level));
    }
}

/// The most cells one copy takes, so selecting everything in a huge result
/// does not build gigabytes of text.
pub const MAX_COPY_CELLS: u64 = 2_000_000;

/// Rows from the first, in the order rows are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    pub first_row: u64,
    pub row_count: u64,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_comes_back_as_the_value_its_literal_needs() {
        let text = |kind, text: &str| CellValue::Text {
            kind,
            text: text.to_owned(),
        };
        assert_eq!(text(ValueKind::Int, "-7").into_value(), Value::Int(-7));
        assert_eq!(
            text(ValueKind::UInt, "18446744073709551615").into_value(),
            Value::UInt(u64::MAX)
        );
        assert_eq!(text(ValueKind::Bool, "true").into_value(), Value::Bool(true));
        assert_eq!(text(ValueKind::Float, "0.1").into_value(), Value::Float(0.1));
        assert_eq!(text(ValueKind::Float, "NaN").into_value(), Value::Text("NaN".into()));
        assert_eq!(
            text(ValueKind::Decimal, "1.50").into_value(),
            Value::Decimal("1.50".into())
        );
        assert_eq!(text(ValueKind::Uuid, "abc").into_value(), Value::Text("abc".into()));
        assert_eq!(CellValue::Bytes(vec![1]).into_value(), Value::Bytes(vec![1]));
        assert_eq!(CellValue::Null.into_value(), Value::Null);
    }
}
