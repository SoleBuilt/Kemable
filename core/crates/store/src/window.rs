//! The packed window a grid reads, version 1.
//!
//! Little-endian, one buffer, every section aligned so a reader can view it
//! as a typed array without copying (`ui/windows/src/lib/grid/window.ts`
//! reads it in TypeScript):
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | 0 | u32 | magic `PBW1` |
//! | 4 | u32 | header length, 64 |
//! | 8 | u64 | first row |
//! | 16 | u64 | rows the set held when the window was read |
//! | 24 | u32 | row count |
//! | 28 | u32 | first column |
//! | 32 | u32 | column count |
//! | 36 | u32 | flags: bit 0 finished, bit 1 limited |
//! | 40 | u32 | offset of the cell kinds |
//! | 44 | u32 | offset of the null bitmap |
//! | 48 | u32 | offset of the truncated bitmap |
//! | 52 | u32 | offset of `raw`, 8-aligned |
//! | 56 | u32 | offset of the text offsets, 4-aligned |
//! | 60 | u32 | offset of the text |
//! | 64 | u8 × columns | column kinds |
//!
//! Cells run column by column: row `r` of column `c` is cell
//! `(c - first column) × row count + (r - first row)`. Its text is
//! `text[offsets[cell]..offsets[cell + 1]]`, at most the request's
//! `max_chars` characters, with control characters made spaces so a cell is
//! one line. Bitmaps hold a bit per cell, lowest bit first. A cell's kind
//! byte says what its `raw` holds (`paboable_core::value::Value::raw`).

use paboable_core::catalog::ColumnMeta;
use paboable_core::format::hex_preview;
use paboable_core::value::ValueKind;

use crate::store::Inner;

pub const MAGIC: u32 = u32::from_le_bytes(*b"PBW1");
pub const HEADER_LEN: usize = 64;
pub const FLAG_FINISHED: u32 = 1;
pub const FLAG_LIMITED: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRequest {
    pub first_row: u64,
    pub row_count: u32,
    pub first_column: u32,
    pub column_count: u32,
    /// The most characters a cell's text carries.
    pub max_chars: u32,
}

pub(crate) fn pack(columns: &[ColumnMeta], inner: &Inner, request: WindowRequest) -> Vec<u8> {
    let total = inner.total_rows;
    let first_row = request.first_row.min(total);
    let rows = usize::try_from((total - first_row).min(u64::from(request.row_count))).unwrap_or(0);
    let first_column = (request.first_column as usize).min(columns.len());
    let column_count = (columns.len() - first_column).min(request.column_count as usize);
    let max_chars = (request.max_chars as usize).max(1);
    let cells = rows * column_count;

    let mut cell_kinds = Vec::with_capacity(cells);
    let mut nulls = vec![0u8; cells.div_ceil(8)];
    let mut truncated = vec![0u8; cells.div_ceil(8)];
    let mut raw = Vec::with_capacity(cells);
    let mut offsets = Vec::with_capacity(cells + 1);
    let mut text = Vec::new();
    let mut scratch = String::new();
    offsets.push(0u32);

    for column in first_column..first_column + column_count {
        let mut row = first_row;
        let end = first_row + rows as u64;
        while row < end {
            let Some((chunk_index, local)) = inner.locate(row) else {
                break;
            };
            let chunk = &inner.chunks[chunk_index];
            let take = (chunk.rows - local).min(usize::try_from(end - row).unwrap_or(usize::MAX));
            let data = &chunk.columns[column];
            for index in local..local + take {
                let cell = cell_kinds.len();
                let kind = data.kinds[index];
                cell_kinds.push(kind);
                raw.push(data.raw[index]);
                if data.is_null(index) {
                    nulls[cell / 8] |= 1 << (cell % 8);
                } else {
                    let bytes = data.bytes(index);
                    let cut = if kind == ValueKind::Bytes.as_u8() {
                        scratch.clear();
                        let cut = hex_preview(bytes, max_chars, &mut scratch);
                        text.extend_from_slice(scratch.as_bytes());
                        cut
                    } else {
                        write_cell_text(bytes, max_chars, &mut text)
                    };
                    if cut {
                        truncated[cell / 8] |= 1 << (cell % 8);
                    }
                }
                offsets.push(u32::try_from(text.len()).unwrap_or(u32::MAX));
            }
            row += take as u64;
        }
    }

    let column_kinds_at = HEADER_LEN;
    let cell_kinds_at = column_kinds_at + column_count;
    let nulls_at = cell_kinds_at + cells;
    let truncated_at = nulls_at + nulls.len();
    let raw_at = (truncated_at + truncated.len()).next_multiple_of(8);
    let offsets_at = raw_at + raw.len() * 8;
    let text_at = offsets_at + offsets.len() * 4;

    let mut flags = 0;
    if inner.finished {
        flags |= FLAG_FINISHED;
    }
    if inner.limited {
        flags |= FLAG_LIMITED;
    }

    let mut out = Vec::with_capacity(text_at + text.len());
    let put32 = |out: &mut Vec<u8>, value: usize| {
        out.extend_from_slice(&u32::try_from(value).unwrap_or(u32::MAX).to_le_bytes())
    };
    out.extend_from_slice(&MAGIC.to_le_bytes());
    put32(&mut out, HEADER_LEN);
    out.extend_from_slice(&first_row.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    put32(&mut out, rows);
    put32(&mut out, first_column);
    put32(&mut out, column_count);
    out.extend_from_slice(&flags.to_le_bytes());
    for offset in [cell_kinds_at, nulls_at, truncated_at, raw_at, offsets_at, text_at] {
        put32(&mut out, offset);
    }
    out.extend(
        columns[first_column..first_column + column_count]
            .iter()
            .map(|c| c.kind.as_u8()),
    );
    out.extend_from_slice(&cell_kinds);
    out.extend_from_slice(&nulls);
    out.extend_from_slice(&truncated);
    out.resize(raw_at, 0);
    for value in &raw {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for offset in &offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&text);
    out
}

/// Writes at most `max_chars` characters of `bytes`, control characters made
/// spaces. Returns whether characters were left out.
fn write_cell_text(bytes: &[u8], max_chars: usize, out: &mut Vec<u8>) -> bool {
    // Most cells are short printable ASCII: copy them as they are.
    if bytes.len() <= max_chars && bytes.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        out.extend_from_slice(bytes);
        return false;
    }
    let text = String::from_utf8_lossy(bytes);
    let mut buffer = [0u8; 4];
    for (count, ch) in text.chars().enumerate() {
        if count == max_chars {
            return true;
        }
        let ch = if ch.is_control() { ' ' } else { ch };
        out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
    }
    false
}

/// Reads a packed window back. The TypeScript reader is the one the app
/// uses; this one serves tests and any Rust UI.
pub struct WindowReader<'a> {
    bytes: &'a [u8],
    pub first_row: u64,
    pub total_rows: u64,
    pub row_count: usize,
    pub first_column: usize,
    pub column_count: usize,
    pub flags: u32,
    sections: [usize; 6],
}

impl<'a> WindowReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Option<Self> {
        let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize);
        let u64_at = |at: usize| Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?));
        if u32_at(0)? != MAGIC as usize {
            return None;
        }
        let mut sections = [0; 6];
        for (index, section) in sections.iter_mut().enumerate() {
            *section = u32_at(40 + index * 4)?;
        }
        Some(Self {
            bytes,
            first_row: u64_at(8)?,
            total_rows: u64_at(16)?,
            row_count: u32_at(24)?,
            first_column: u32_at(28)?,
            column_count: u32_at(32)?,
            flags: u32::try_from(u32_at(36)?).ok()?,
            sections,
        })
    }

    fn cell(&self, row: usize, column: usize) -> usize {
        column * self.row_count + row
    }

    fn bit(&self, section: usize, cell: usize) -> bool {
        self.bytes
            .get(self.sections[section] + cell / 8)
            .is_some_and(|byte| byte & (1 << (cell % 8)) != 0)
    }

    pub fn is_null(&self, row: usize, column: usize) -> bool {
        self.bit(1, self.cell(row, column))
    }

    pub fn is_truncated(&self, row: usize, column: usize) -> bool {
        self.bit(2, self.cell(row, column))
    }

    pub fn kind(&self, row: usize, column: usize) -> u8 {
        self.bytes
            .get(self.sections[0] + self.cell(row, column))
            .copied()
            .unwrap_or(0)
    }

    pub fn raw(&self, row: usize, column: usize) -> i64 {
        let at = self.sections[3] + self.cell(row, column) * 8;
        self.bytes
            .get(at..at + 8)
            .and_then(|b| b.try_into().ok())
            .map_or(0, i64::from_le_bytes)
    }

    pub fn text(&self, row: usize, column: usize) -> &'a str {
        let cell = self.cell(row, column);
        let offset = |index: usize| {
            let at = self.sections[4] + index * 4;
            self.bytes
                .get(at..at + 4)
                .and_then(|b| b.try_into().ok())
                .map_or(0, u32::from_le_bytes) as usize
        };
        let base = self.sections[5];
        self.bytes
            .get(base + offset(cell)..base + offset(cell + 1))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use paboable_core::value::Value;

    use super::*;
    use crate::{CHUNK_ROWS, CellValue, ResultStore};

    fn column(name: &str, kind: ValueKind) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            type_name: String::new(),
            kind,
        }
    }

    fn request(first_row: u64, row_count: u32) -> WindowRequest {
        WindowRequest {
            first_row,
            row_count,
            first_column: 0,
            column_count: u32::MAX,
            max_chars: 100,
        }
    }

    #[test]
    fn a_window_holds_the_rows_it_asked_for_column_by_column() {
        let store = ResultStore::new(
            vec![column("id", ValueKind::Int), column("name", ValueKind::Text)],
            None,
        );
        let rows = [
            Value::Int(1),
            Value::Text("a".into()),
            Value::Int(2),
            Value::Null,
            Value::Int(3),
            Value::Text("c".into()),
        ];
        assert!(store.push_rows(&rows));
        store.finish();

        let bytes = store.fetch_window(request(1, 10));
        let window = WindowReader::new(&bytes).expect("a valid window");
        assert_eq!(
            (
                window.first_row,
                window.total_rows,
                window.row_count,
                window.column_count
            ),
            (1, 3, 2, 2)
        );
        assert_eq!(window.flags & FLAG_FINISHED, FLAG_FINISHED);
        assert_eq!(window.text(0, 0), "2");
        assert_eq!(window.raw(1, 0), 3);
        assert!(window.is_null(0, 1));
        assert_eq!(window.text(1, 1), "c");
        assert_eq!(window.kind(1, 1), ValueKind::Text.as_u8());
    }

    #[test]
    fn a_window_past_the_end_is_empty() {
        let store = ResultStore::new(vec![column("id", ValueKind::Int)], None);
        assert!(store.push_rows(&[Value::Int(1)]));
        let bytes = store.fetch_window(request(5, 10));
        let window = WindowReader::new(&bytes).expect("a valid window");
        assert_eq!((window.first_row, window.row_count), (1, 0));
    }

    #[test]
    fn a_window_reads_across_chunks() {
        let store = ResultStore::new(vec![column("n", ValueKind::Int)], None);
        let values: Vec<Value> = (0..CHUNK_ROWS as i64 + 10).map(Value::Int).collect();
        assert!(store.push_rows(&values));
        let first = CHUNK_ROWS as u64 - 2;
        let bytes = store.fetch_window(request(first, 4));
        let window = WindowReader::new(&bytes).expect("a valid window");
        let texts: Vec<&str> = (0..4).map(|row| window.text(row, 0)).collect();
        let expected: Vec<String> = (first..first + 4).map(|n| n.to_string()).collect();
        assert_eq!(texts, expected);
    }

    #[test]
    fn the_row_limit_stops_only_when_a_row_past_it_is_offered() {
        let store = ResultStore::new(vec![column("n", ValueKind::Int)], Some(2));
        assert!(store.push_rows(&[Value::Int(1), Value::Int(2)]));
        assert!(!store.state().limited);
        assert!(!store.push_rows(&[Value::Int(3)]));
        let state = store.state();
        assert_eq!((state.total_rows, state.limited), (2, true));
    }

    #[test]
    fn long_text_is_cut_at_the_cap_and_control_characters_become_spaces() {
        let store = ResultStore::new(vec![column("t", ValueKind::Text)], None);
        assert!(store.push_rows(&[Value::Text("a\nb\tc".into()), Value::Text("Việt Nam".into())]));
        let bytes = store.fetch_window(WindowRequest {
            max_chars: 4,
            ..request(0, 2)
        });
        let window = WindowReader::new(&bytes).expect("a valid window");
        assert_eq!(window.text(0, 0), "a b ");
        assert!(window.is_truncated(0, 0));
        assert_eq!(window.text(1, 0), "Việt");
        // The full value is never cut.
        assert_eq!(store.cell_text(0, 0), Some(Some("a\nb\tc".into())));
    }

    #[test]
    fn bytes_show_as_hex_and_copy_in_full() {
        let store = ResultStore::new(vec![column("b", ValueKind::Bytes)], None);
        assert!(store.push_rows(&[Value::Bytes(vec![0xca, 0xfe, 0xba, 0xbe])]));
        let bytes = store.fetch_window(WindowRequest {
            max_chars: 6,
            ..request(0, 1)
        });
        let window = WindowReader::new(&bytes).expect("a valid window");
        assert_eq!(window.text(0, 0), "0xcafe");
        assert!(window.is_truncated(0, 0));
        assert_eq!(window.raw(0, 0), 4);
        assert_eq!(store.cell_text(0, 0), Some(Some("0xcafebabe".into())));
        assert_eq!(store.cell(0, 0), Some(CellValue::Bytes(vec![0xca, 0xfe, 0xba, 0xbe])));
    }

    #[test]
    fn sections_are_aligned_for_typed_array_views() {
        let store = ResultStore::new(vec![column("a", ValueKind::Int), column("b", ValueKind::Text)], None);
        assert!(store.push_rows(&[Value::Int(1), Value::Text("x".into())]));
        let bytes = store.fetch_window(request(0, 1));
        let window = WindowReader::new(&bytes).expect("a valid window");
        assert_eq!(window.sections[3] % 8, 0);
        assert_eq!(window.sections[4] % 4, 0);
    }
}
