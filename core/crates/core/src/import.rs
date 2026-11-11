//! CSV into a table, for Import (`docs/decisions/0018-export-import-generate.md`):
//! the reader (RFC 4180: quoted fields, doubled quotes, line breaks inside
//! quotes), the separator a file uses, the types a new table takes from its
//! values, and the `INSERT` statements that carry the rows, a batch at a time.

use serde::{Deserialize, Serialize};

use crate::catalog::{ColumnInfo, TableRef};
use crate::sql::{SqlGrammar, quote_name, table_name};
use crate::{DriverError, Result};

/// Rows in one `INSERT`: SQL Server takes at most 1,000 in a VALUES list.
pub const ROWS_PER_INSERT: usize = 500;

/// Reads CSV records one at a time from text the caller holds.
pub struct CsvReader<'a> {
    text: &'a str,
    at: usize,
    delimiter: char,
}

impl<'a> CsvReader<'a> {
    pub fn new(text: &'a str, delimiter: char) -> Self {
        // A byte order mark is not part of the first field.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        Self { text, at: 0, delimiter }
    }
}

impl Iterator for CsvReader<'_> {
    type Item = Vec<String>;

    fn next(&mut self) -> Option<Vec<String>> {
        let rest = self.text.get(self.at..)?;
        if rest.is_empty() {
            return None;
        }
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = rest.char_indices().peekable();
        while let Some((offset, ch)) = chars.next() {
            if quoted {
                if ch == '"' {
                    if chars.peek().is_some_and(|(_, next)| *next == '"') {
                        field.push('"');
                        chars.next();
                    } else {
                        quoted = false;
                    }
                } else {
                    field.push(ch);
                }
            } else if ch == '"' && field.is_empty() {
                quoted = true;
            } else if ch == self.delimiter {
                fields.push(std::mem::take(&mut field));
            } else if ch == '\r' || ch == '\n' {
                // \r\n, \n or a lone \r ends the record.
                let mut end = offset + ch.len_utf8();
                if ch == '\r' && rest[end..].starts_with('\n') {
                    end += 1;
                }
                self.at += end;
                fields.push(field);
                return Some(fields);
            } else {
                field.push(ch);
            }
        }
        // The last record, with no line break after it (or a quote left open, read to the end).
        self.at = self.text.len();
        fields.push(field);
        Some(fields)
    }
}

/// The separator a CSV file uses: the one of `,` `;` tab `|` that splits
/// its first lines into the same, largest number of fields.
pub fn detect_delimiter(text: &str) -> char {
    let sample: String = text.lines().take(20).collect::<Vec<_>>().join("\n");
    let mut best = (',', 0usize);
    for candidate in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = CsvReader::new(&sample, candidate)
            .take(20)
            .map(|record| record.len())
            .filter(|count| *count > 1)
            .collect();
        let Some(first) = counts.first().copied() else { continue };
        let steady = counts.iter().filter(|count| **count == first).count();
        if steady * first > best.1 {
            best = (candidate, steady * first);
        }
    }
    best.0
}

/// What a column's values look like, for a new table's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ValueShape {
    Integer,
    Decimal,
    Boolean,
    Date,
    Timestamp,
    Text,
}

/// The narrowest shape every non-empty value fits.
pub fn infer_shape<'a>(values: impl IntoIterator<Item = &'a str>) -> ValueShape {
    let mut shape: Option<ValueShape> = None;
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let this = shape_of(value);
        shape = Some(match (shape, this) {
            (None, this) => this,
            (Some(before), this) if before == this => before,
            (Some(ValueShape::Integer), ValueShape::Decimal) | (Some(ValueShape::Decimal), ValueShape::Integer) => {
                ValueShape::Decimal
            }
            (Some(ValueShape::Date), ValueShape::Timestamp) | (Some(ValueShape::Timestamp), ValueShape::Date) => {
                ValueShape::Timestamp
            }
            _ => ValueShape::Text,
        });
        if shape == Some(ValueShape::Text) {
            break;
        }
    }
    shape.unwrap_or(ValueShape::Text)
}

fn shape_of(value: &str) -> ValueShape {
    let digits = value.strip_prefix('-').unwrap_or(value);
    // A leading zero is a code (01121212), not a number: it stays text.
    let leading_zero = digits.len() > 1 && digits.starts_with('0') && !digits.starts_with("0.");
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) && !leading_zero && digits.len() <= 18 {
        return ValueShape::Integer;
    }
    if let Some((whole, fraction)) = digits.split_once('.')
        && !whole.is_empty()
        && !fraction.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.bytes().all(|byte| byte.is_ascii_digit())
        && !leading_zero
    {
        return ValueShape::Decimal;
    }
    if ["true", "false"].iter().any(|word| value.eq_ignore_ascii_case(word)) {
        return ValueShape::Boolean;
    }
    let bytes = value.as_bytes();
    let date = bytes.len() >= 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit);
    if date && bytes.len() == 10 {
        return ValueShape::Date;
    }
    if date && matches!(bytes[10], b' ' | b'T') && bytes.len() >= 16 && bytes[13] == b':' {
        return ValueShape::Timestamp;
    }
    ValueShape::Text
}

/// The type a new table's column takes for `shape`.
pub fn column_type(grammar: SqlGrammar, shape: ValueShape) -> &'static str {
    match (grammar, shape) {
        (SqlGrammar::Postgres, ValueShape::Integer) => "bigint",
        (SqlGrammar::Postgres, ValueShape::Decimal) => "numeric",
        (SqlGrammar::Postgres, ValueShape::Boolean) => "boolean",
        (SqlGrammar::Postgres, ValueShape::Date) => "date",
        (SqlGrammar::Postgres, ValueShape::Timestamp) => "timestamp",
        (SqlGrammar::Postgres, ValueShape::Text) => "text",
        (SqlGrammar::SqlServer, ValueShape::Integer) => "bigint",
        (SqlGrammar::SqlServer, ValueShape::Decimal) => "decimal(38, 10)",
        (SqlGrammar::SqlServer, ValueShape::Boolean) => "bit",
        (SqlGrammar::SqlServer, ValueShape::Date) => "date",
        (SqlGrammar::SqlServer, ValueShape::Timestamp) => "datetime2",
        (SqlGrammar::SqlServer, ValueShape::Text) => "nvarchar(max)",
        (SqlGrammar::Sqlite, ValueShape::Integer | ValueShape::Boolean) => "INTEGER",
        (SqlGrammar::Sqlite, ValueShape::Decimal) => "REAL",
        (SqlGrammar::Sqlite, ValueShape::Date | ValueShape::Timestamp | ValueShape::Text) => "TEXT",
    }
}

/// A new table's column: its name from the header and its type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewColumn {
    pub name: String,
    pub type_name: String,
}

/// The `CREATE TABLE` a new table runs before its rows.
pub fn create_statement(grammar: SqlGrammar, table: &TableRef, columns: &[NewColumn]) -> Result<String> {
    if columns.is_empty() {
        return Err(DriverError::config("a new table needs a column"));
    }
    let lines: Vec<String> = columns
        .iter()
        .map(|column| format!("    {} {}", quote_name(grammar, &column.name), column.type_name))
        .collect();
    Ok(format!(
        "CREATE TABLE {} (\n{}\n)",
        table_name(grammar, table),
        lines.join(",\n")
    ))
}

/// Where each CSV field goes: the table's column it matches by name (case
/// does not matter), or `None` for a field the table has no column for.
pub fn match_columns(header: &[String], columns: &[ColumnInfo]) -> Vec<Option<usize>> {
    header
        .iter()
        .map(|name| {
            let name = name.trim();
            columns
                .iter()
                .position(|column| !column.generated && column.name.eq_ignore_ascii_case(name))
        })
        .collect()
}

/// Whether a column type holds numbers, so a numeric value goes in bare.
fn numeric_type(type_name: &str) -> bool {
    let lower = type_name.to_ascii_lowercase();
    [
        "int",
        "integer",
        "bigint",
        "smallint",
        "tinyint",
        "decimal",
        "numeric",
        "real",
        "float",
        "double",
        "money",
        "smallmoney",
        "number",
    ]
    .iter()
    .any(|word| lower == *word || lower.starts_with(&format!("{word}(")) || lower.starts_with(&format!("{word} ")))
}

/// A CSV field as a literal for a column of `type_name`: bare for a number
/// in a numeric column, quoted otherwise (the server reads dates, booleans
/// and text from quotes), NULL for an empty field when `empty_is_null`.
pub fn field_literal(grammar: SqlGrammar, type_name: &str, value: &str, empty_is_null: bool) -> String {
    if value.is_empty() && empty_is_null {
        return "NULL".to_owned();
    }
    let trimmed = value.trim();
    let number = !trimmed.is_empty()
        && trimmed
            .strip_prefix('-')
            .unwrap_or(trimmed)
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && trimmed.bytes().filter(|byte| *byte == b'.').count() <= 1
        && !trimmed.ends_with('.')
        && !trimmed.starts_with('.');
    if number && numeric_type(type_name) {
        return trimmed.to_owned();
    }
    let mut out = String::with_capacity(value.len() + 3);
    if grammar == SqlGrammar::SqlServer {
        out.push('N');
    }
    out.push('\'');
    out.push_str(&value.replace('\'', "''"));
    out.push('\'');
    out
}

/// One `INSERT` for a batch of rows: `names` are the target columns, each
/// row holds their literals already written.
pub fn insert_statement(grammar: SqlGrammar, table: &TableRef, names: &[String], rows: &[Vec<String>]) -> String {
    let columns: Vec<String> = names.iter().map(|name| quote_name(grammar, name)).collect();
    let values: Vec<String> = rows.iter().map(|row| format!("({})", row.join(", "))).collect();
    format!(
        "INSERT INTO {} ({}) VALUES\n{}",
        table_name(grammar, table),
        columns.join(", "),
        values.join(",\n")
    )
}

/// The statements that begin, keep and undo a transaction, as the engine spells them.
pub fn transaction(grammar: SqlGrammar) -> (&'static str, &'static str, &'static str) {
    match grammar {
        SqlGrammar::SqlServer => ("BEGIN TRANSACTION", "COMMIT TRANSACTION", "ROLLBACK TRANSACTION"),
        SqlGrammar::Postgres | SqlGrammar::Sqlite => ("BEGIN", "COMMIT", "ROLLBACK"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(text: &str, delimiter: char) -> Vec<Vec<String>> {
        CsvReader::new(text, delimiter).collect()
    }

    #[test]
    fn csv_reads_quotes_doubled_quotes_and_line_breaks_inside_quotes() {
        let text =
            "\u{feff}mã,tên,ghi chú\r\n01121212,\"Nguyễn \"\"An\"\"\",\"dòng 1\r\ndòng 2\"\n2,Bình,\r3,\"a,b\",x";
        assert_eq!(
            records(text, ','),
            [
                vec!["mã", "tên", "ghi chú"],
                vec!["01121212", "Nguyễn \"An\"", "dòng 1\r\ndòng 2"],
                vec!["2", "Bình", ""],
                vec!["3", "a,b", "x"],
            ]
        );
        assert_eq!(records("a;b\n1;2\n", ';'), [vec!["a", "b"], vec!["1", "2"]]);
    }

    #[test]
    fn the_separator_is_the_one_that_splits_the_lines_alike() {
        assert_eq!(detect_delimiter("a;b;c\n1;2,5;3\n4;5;6"), ';');
        assert_eq!(detect_delimiter("a,b\n1,2"), ',');
        assert_eq!(detect_delimiter("a\tb\tc\n1\t2\t3"), '\t');
        assert_eq!(detect_delimiter("one column\nonly"), ',');
    }

    #[test]
    fn a_new_tables_types_come_from_its_values() {
        assert_eq!(infer_shape(["1", "-20", ""]), ValueShape::Integer);
        assert_eq!(infer_shape(["1", "2.50"]), ValueShape::Decimal);
        // A code with a leading zero keeps it: text, not a number.
        assert_eq!(infer_shape(["01121212", "20000"]), ValueShape::Text);
        assert_eq!(
            infer_shape(["2026-11-12", "2026-01-01 08:30:00"]),
            ValueShape::Timestamp
        );
        assert_eq!(infer_shape(["TRUE", "false"]), ValueShape::Boolean);
        assert_eq!(infer_shape(["", ""]), ValueShape::Text);
        assert_eq!(column_type(SqlGrammar::SqlServer, ValueShape::Text), "nvarchar(max)");
        let create = create_statement(
            SqlGrammar::Postgres,
            &TableRef {
                schema: Some("public".into()),
                name: "Khách hàng".into(),
            },
            &[
                NewColumn {
                    name: "mã".into(),
                    type_name: "text".into(),
                },
                NewColumn {
                    name: "số dư".into(),
                    type_name: "numeric".into(),
                },
            ],
        )
        .unwrap();
        assert_eq!(
            create,
            "CREATE TABLE \"public\".\"Khách hàng\" (\n    \"mã\" text,\n    \"số dư\" numeric\n)"
        );
    }

    #[test]
    fn fields_become_literals_for_their_columns_type() {
        assert_eq!(
            field_literal(SqlGrammar::SqlServer, "nvarchar(20)", "01121212", true),
            "N'01121212'"
        );
        assert_eq!(
            field_literal(SqlGrammar::SqlServer, "decimal(38,20)", "-12.50", true),
            "-12.50"
        );
        assert_eq!(field_literal(SqlGrammar::Postgres, "integer", "", true), "NULL");
        assert_eq!(field_literal(SqlGrammar::Postgres, "text", "", false), "''");
        assert_eq!(
            field_literal(SqlGrammar::Postgres, "text", "O'Brien", true),
            "'O''Brien'"
        );
        assert_eq!(
            field_literal(SqlGrammar::Postgres, "date", "2026-11-12", true),
            "'2026-11-12'"
        );
        assert_eq!(field_literal(SqlGrammar::Sqlite, "INTEGER", "1e5", true), "'1e5'");
    }

    #[test]
    fn headers_match_columns_by_name_without_case_and_skip_generated_ones() {
        let column = |name: &str, generated: bool| ColumnInfo {
            ordinal: 0,
            name: name.into(),
            type_name: "int".into(),
            nullable: true,
            default: None,
            primary_key: None,
            generated,
        };
        let columns = [column("Id", true), column("City", false), column("Name", false)];
        let header = [
            "id".to_owned(),
            "NAME".to_owned(),
            "phone".to_owned(),
            " city ".to_owned(),
        ];
        assert_eq!(match_columns(&header, &columns), [None, Some(2), None, Some(1)]);
        let insert = insert_statement(
            SqlGrammar::SqlServer,
            &TableRef {
                schema: Some("dbo".into()),
                name: "t".into(),
            },
            &["Name".into(), "City".into()],
            &[
                vec!["N'An'".into(), "NULL".into()],
                vec!["N'Bình'".into(), "N'Huế'".into()],
            ],
        );
        assert_eq!(
            insert,
            "INSERT INTO [dbo].[t] ([Name], [City]) VALUES\n(N'An', NULL),\n(N'Bình', N'Huế')"
        );
    }
}
