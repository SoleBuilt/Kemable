//! Values as SQL literals, for the statements the core writes: a table tab's
//! filters (`browse.rs`) and its edits (`edit.rs`).
//!
//! - `typed`: what a person typed, as text the server converts to the column's
//!   type. Quotes are doubled, so no value can end its literal.
//! - `value_literal`: a value read from the database, written so it compares
//!   equal to itself: numbers bare, bytes in the engine's own form, the rest
//!   as quoted text in the form the core displays it.

use super::SqlGrammar;
use crate::format::display;
use crate::value::Value;

/// A typed value as a literal. SQLite compares a column with the literal's
/// own type, so a number goes bare there; PostgreSQL and SQL Server convert a
/// quoted literal to the column's type, and refuse one that does not fit.
pub fn typed(grammar: SqlGrammar, value: &str) -> String {
    if grammar == SqlGrammar::Sqlite && is_number(value) {
        return value.to_owned();
    }
    quoted(grammar, value)
}

/// Text between single quotes, its quotes doubled; `N'...'` on SQL Server, so
/// text past the server's code page survives.
pub fn quoted(grammar: SqlGrammar, value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 3);
    if grammar == SqlGrammar::SqlServer {
        out.push('N');
    }
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

pub fn is_number(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let mut parts = digits.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        // A leading zero is text, as a code or a phone number is.
        && (whole == "0" || !whole.starts_with('0'))
}

/// A value read from the database as the literal that names it; `None` for NULL.
pub fn value_literal(grammar: SqlGrammar, value: &Value) -> Option<String> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(value) => match (grammar, value) {
            (SqlGrammar::Postgres, true) => "TRUE".to_owned(),
            (SqlGrammar::Postgres, false) => "FALSE".to_owned(),
            (_, true) => "1".to_owned(),
            (_, false) => "0".to_owned(),
        },
        Value::Int(_) | Value::UInt(_) => display(value),
        Value::Float(number) if number.is_finite() => display(value),
        Value::Float32(number) if number.is_finite() => display(value),
        Value::Decimal(text) if is_decimal(text) => text.clone(),
        Value::Bytes(bytes) => {
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            match grammar {
                SqlGrammar::Postgres => format!("'\\x{hex}'::bytea"),
                SqlGrammar::SqlServer => format!("0x{hex}"),
                SqlGrammar::Sqlite => format!("X'{hex}'"),
            }
        }
        // Text, JSON, UUIDs, dates and times, NaN and infinities: the text the
        // core shows, which each engine reads back as the same value.
        other => quoted(grammar, &display(other)),
    })
}

fn is_decimal(text: &str) -> bool {
    let body = text.strip_prefix(['-', '+']).unwrap_or(text);
    !body.is_empty()
        && body.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.')
        && body.bytes().filter(|byte| *byte == b'.').count() <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_read_is_written_back_as_the_same_value() {
        let cases: [(Value, &str, &str, &str); 12] = [
            (Value::Int(-42), "-42", "-42", "-42"),
            (
                Value::UInt(u64::MAX),
                "18446744073709551615",
                "18446744073709551615",
                "18446744073709551615",
            ),
            (Value::Bool(true), "TRUE", "1", "1"),
            (Value::Float(0.1), "0.1", "0.1", "0.1"),
            (Value::Float(f64::INFINITY), "'Infinity'", "N'Infinity'", "'Infinity'"),
            (Value::Decimal("-12.3400".into()), "-12.3400", "-12.3400", "-12.3400"),
            (Value::Decimal("NaN".into()), "'NaN'", "N'NaN'", "'NaN'"),
            (Value::Text("O'Brien".into()), "'O''Brien'", "N'O''Brien'", "'O''Brien'"),
            (Value::Bytes(vec![0, 0xff]), "'\\x00ff'::bytea", "0x00ff", "X'00ff'"),
            (Value::Bytes(Vec::new()), "'\\x'::bytea", "0x", "X''"),
            (
                Value::Uuid([0x12; 16]),
                "'12121212-1212-1212-1212-121212121212'",
                "N'12121212-1212-1212-1212-121212121212'",
                "'12121212-1212-1212-1212-121212121212'",
            ),
            (Value::Date(0), "'1970-01-01'", "N'1970-01-01'", "'1970-01-01'"),
        ];
        for (value, postgres, mssql, sqlite) in cases {
            assert_eq!(
                value_literal(SqlGrammar::Postgres, &value).as_deref(),
                Some(postgres),
                "{value:?}"
            );
            assert_eq!(
                value_literal(SqlGrammar::SqlServer, &value).as_deref(),
                Some(mssql),
                "{value:?}"
            );
            assert_eq!(
                value_literal(SqlGrammar::Sqlite, &value).as_deref(),
                Some(sqlite),
                "{value:?}"
            );
        }
        assert_eq!(value_literal(SqlGrammar::Postgres, &Value::Null), None);
    }

    #[test]
    fn typed_text_is_quoted_and_sqlite_takes_a_plain_number_bare() {
        assert_eq!(typed(SqlGrammar::Postgres, "it's"), "'it''s'");
        assert_eq!(typed(SqlGrammar::SqlServer, "xin chào"), "N'xin chào'");
        assert_eq!(typed(SqlGrammar::Sqlite, "12.5"), "12.5");
        assert_eq!(typed(SqlGrammar::Sqlite, "0012"), "'0012'");
        assert_eq!(typed(SqlGrammar::Postgres, "12"), "'12'");
    }
}
