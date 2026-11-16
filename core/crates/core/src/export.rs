//! A table's rows written to a file as they arrive, for Export
//! (`docs/decisions/0018-export-import-generate.md`): CSV, JSON or INSERT
//! statements. The writer holds no rows; each batch the driver pushes is
//! written out and forgotten, so a table of millions of rows never sits in
//! memory.

use serde::{Deserialize, Serialize};

use crate::catalog::TableRef;
use crate::format::{hex_preview, write_display};
use crate::sql::{SqlGrammar, quote_name, table_name, value_literal};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Csv,
    Json,
    /// One `INSERT` statement per row, for the engine the rows came from.
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// CSV's separator: `,`, `;` or a tab.
    #[serde(default = "comma")]
    pub delimiter: char,
    /// CSV starts with the column names.
    #[serde(default = "yes")]
    pub header: bool,
    /// The file starts with a UTF-8 byte order mark, so Excel reads it as UTF-8.
    #[serde(default)]
    pub bom: bool,
}

fn comma() -> char {
    ','
}

fn yes() -> bool {
    true
}

/// Writes rows in one format. `start` once, `rows` for each batch, `finish` once.
pub struct ExportWriter {
    options: ExportOptions,
    grammar: SqlGrammar,
    table: TableRef,
    columns: Vec<String>,
    rows: u64,
    cell: String,
}

impl ExportWriter {
    pub fn new(options: ExportOptions, grammar: SqlGrammar, table: TableRef) -> Self {
        Self {
            options,
            grammar,
            table,
            columns: Vec::new(),
            rows: 0,
            cell: String::new(),
        }
    }

    pub fn rows_written(&self) -> u64 {
        self.rows
    }

    /// The file's first bytes: a byte order mark, CSV's header, JSON's `[`.
    pub fn start(&mut self, columns: Vec<String>, out: &mut Vec<u8>) {
        if self.options.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        self.columns = columns;
        match self.options.format {
            ExportFormat::Csv if self.options.header => {
                let names: Vec<String> = self.columns.iter().map(|name| self.csv_field(name)).collect();
                out.extend_from_slice(names.join(&self.options.delimiter.to_string()).as_bytes());
                out.extend_from_slice(b"\r\n");
            }
            ExportFormat::Json => out.push(b'['),
            _ => {}
        }
    }

    /// Writes `values`, row after row, `self.columns.len()` to a row.
    pub fn rows(&mut self, values: &[Value], out: &mut Vec<u8>) {
        let width = self.columns.len().max(1);
        for row in values.chunks(width) {
            match self.options.format {
                ExportFormat::Csv => self.csv_row(row, out),
                ExportFormat::Json => self.json_row(row, out),
                ExportFormat::Insert => self.insert_row(row, out),
            }
            self.rows += 1;
        }
    }

    /// The file's last bytes: JSON's `]`.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        if self.options.format == ExportFormat::Json {
            out.extend_from_slice(if self.rows == 0 { b"]" } else { b"\n]" });
            out.extend_from_slice(b"\n");
        }
    }

    /// The text a CSV field or a JSON string holds. Bytes have no display
    /// text, so they go as `0x` and their hex, every byte of them.
    fn text(&mut self, value: &Value) -> String {
        self.cell.clear();
        match value {
            Value::Bytes(bytes) => {
                hex_preview(bytes, usize::MAX, &mut self.cell);
            }
            other => write_display(other, &mut self.cell),
        }
        self.cell.clone()
    }

    /// RFC 4180: a field holding the separator, a quote or a line break goes
    /// in quotes, its quotes doubled. NULL is an empty field.
    fn csv_field(&self, text: &str) -> String {
        let delimiter = self.options.delimiter;
        if text.contains([delimiter, '"', '\n', '\r']) {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.to_owned()
        }
    }

    fn csv_row(&mut self, row: &[Value], out: &mut Vec<u8>) {
        for (index, value) in row.iter().enumerate() {
            if index > 0 {
                let mut separator = [0; 4];
                out.extend_from_slice(self.options.delimiter.encode_utf8(&mut separator).as_bytes());
            }
            if !matches!(value, Value::Null) {
                let text = self.text(value);
                out.extend_from_slice(self.csv_field(&text).as_bytes());
            }
        }
        out.extend_from_slice(b"\r\n");
    }

    /// An object per row: numbers and booleans as JSON has them, a JSON
    /// column as its own JSON, everything else as the text the core shows.
    fn json_row(&mut self, row: &[Value], out: &mut Vec<u8>) {
        out.extend_from_slice(if self.rows == 0 { b"\n  {" } else { b",\n  {" });
        for (index, value) in row.iter().enumerate() {
            if index > 0 {
                out.extend_from_slice(b", ");
            }
            let name = self.columns.get(index).cloned().unwrap_or_default();
            out.extend_from_slice(json_string(&name).as_bytes());
            out.extend_from_slice(b": ");
            let json = match value {
                Value::Null => "null".to_owned(),
                Value::Bool(flag) => flag.to_string(),
                Value::Int(_) | Value::UInt(_) => self.text(value),
                Value::Float(number) if number.is_finite() => self.text(value),
                Value::Float32(number) if number.is_finite() => self.text(value),
                Value::Decimal(text) if is_json_number(text) => text.clone(),
                Value::Json(text) => text.clone(),
                other => json_string(&self.text(other)),
            };
            out.extend_from_slice(json.as_bytes());
        }
        out.push(b'}');
    }

    fn insert_row(&mut self, row: &[Value], out: &mut Vec<u8>) {
        let names: Vec<String> = self.columns.iter().map(|name| quote_name(self.grammar, name)).collect();
        let values: Vec<String> = row
            .iter()
            .map(|value| value_literal(self.grammar, value).unwrap_or_else(|| "NULL".to_owned()))
            .collect();
        let statement = format!(
            "INSERT INTO {} ({}) VALUES ({});\n",
            table_name(self.grammar, &self.table),
            names.join(", "),
            values.join(", ")
        );
        out.extend_from_slice(statement.as_bytes());
    }
}

/// Text as a JSON string, with its quotes, backslashes and control characters escaped.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Whether decimal text is a number JSON reads as it is (no `+`, no bare `.5`).
fn is_json_number(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = body.split_once('.').unwrap_or((body, "1"));
    !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && !fraction.is_empty()
        && fraction.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(format: ExportFormat, bom: bool) -> String {
        let options = ExportOptions {
            format,
            delimiter: ',',
            header: true,
            bom,
        };
        let table = TableRef {
            schema: Some("dbo".into()),
            name: "Khách hàng".into(),
        };
        let mut writer = ExportWriter::new(options, SqlGrammar::SqlServer, table);
        let mut out = Vec::new();
        writer.start(
            vec!["id".into(), "tên, \"đầy đủ\"".into(), "số dư".into(), "ghi chú".into()],
            &mut out,
        );
        // Two batches, as a driver pushes them.
        writer.rows(
            &[
                Value::Int(1),
                Value::Text("Nguyễn \"An\"".into()),
                Value::Decimal("-12.50".into()),
                Value::Null,
            ],
            &mut out,
        );
        writer.rows(
            &[
                Value::Int(2),
                Value::Text("dòng 1\ndòng 2".into()),
                Value::Decimal("0".into()),
                Value::Text("a,b".into()),
            ],
            &mut out,
        );
        writer.finish(&mut out);
        assert_eq!(writer.rows_written(), 2);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn csv_quotes_what_needs_quoting_and_leaves_null_empty() {
        let csv = write(ExportFormat::Csv, true);
        assert!(csv.starts_with('\u{feff}'), "a BOM for Excel");
        assert_eq!(
            csv.trim_start_matches('\u{feff}'),
            "id,\"tên, \"\"đầy đủ\"\"\",số dư,ghi chú\r\n\
             1,\"Nguyễn \"\"An\"\"\",-12.50,\r\n\
             2,\"dòng 1\ndòng 2\",0,\"a,b\"\r\n"
        );
        assert!(!write(ExportFormat::Csv, false).starts_with('\u{feff}'));
    }

    #[test]
    fn json_is_an_array_of_objects_with_numbers_as_numbers() {
        let json = write(ExportFormat::Json, false);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["tên, \"đầy đủ\""], "Nguyễn \"An\"");
        // The exact decimal text, as the engine gave it: a JSON reader may round it, the file does not.
        assert!(json.contains("\"số dư\": -12.50"), "{json}");
        assert!(parsed[0]["ghi chú"].is_null());
        assert_eq!(parsed[1]["tên, \"đầy đủ\""], "dòng 1\ndòng 2");
        assert_eq!(parsed.as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn insert_writes_one_statement_per_row_for_the_engine() {
        let sql = write(ExportFormat::Insert, false);
        let lines: Vec<&str> = sql.lines().collect();
        assert_eq!(
            lines[0],
            "INSERT INTO [dbo].[Khách hàng] ([id], [tên, \"đầy đủ\"], [số dư], [ghi chú]) VALUES (1, N'Nguyễn \"An\"', -12.50, NULL);"
        );
        assert_eq!(lines.len(), 3, "the second row's text holds a line break");
    }

    fn write_bytes(format: ExportFormat) -> String {
        let options = ExportOptions {
            format,
            delimiter: ',',
            header: true,
            bom: false,
        };
        let table = TableRef {
            schema: None,
            name: "t".into(),
        };
        let mut writer = ExportWriter::new(options, SqlGrammar::Postgres, table);
        let mut out = Vec::new();
        writer.start(vec!["id".into(), "data".into()], &mut out);
        writer.rows(
            &[
                Value::Int(1),
                Value::Bytes(vec![0x00, 0xff, 0x10]),
                Value::Int(2),
                Value::Bytes(Vec::new()),
                Value::Int(3),
                Value::Null,
            ],
            &mut out,
        );
        writer.finish(&mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn csv_writes_bytes_as_hex_with_a_0x_prefix() {
        assert_eq!(
            write_bytes(ExportFormat::Csv),
            "id,data\r\n1,0x00ff10\r\n2,0x\r\n3,\r\n"
        );
    }

    #[test]
    fn json_writes_bytes_as_a_hex_string_with_a_0x_prefix() {
        let json = write_bytes(ExportFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["data"], "0x00ff10");
        assert_eq!(parsed[1]["data"], "0x");
        assert!(parsed[2]["data"].is_null());
    }

    #[test]
    fn an_empty_table_is_a_header_or_an_empty_array() {
        let options = ExportOptions {
            format: ExportFormat::Json,
            delimiter: ',',
            header: true,
            bom: false,
        };
        let mut writer = ExportWriter::new(
            options,
            SqlGrammar::Postgres,
            TableRef {
                schema: None,
                name: "t".into(),
            },
        );
        let mut out = Vec::new();
        writer.start(vec!["a".into()], &mut out);
        writer.finish(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "[]\n");
    }
}
