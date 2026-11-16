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

/// The bytes a hex text names: `0x` (as the grid shows bytes) or `\x` (as
/// PostgreSQL does) and two hex digits a byte. `None` for anything else.
pub fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .or_else(|| text.strip_prefix("\\x"))?;
    if digits.len() % 2 != 0 {
        return None;
    }
    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            u8::try_from(high * 16 + low).ok()
        })
        .collect()
}

/// A date, or a date and time, written `YYYY-MM-DD[ hh:mm[:ss[.f]]][offset]`
/// (as the core shows them, or as a person types them), in the form SQL
/// Server reads the same under every language and `DATEFORMAT`: `YYYYMMDD`
/// for a date alone, `YYYY-MM-DDThh:mm:ss[.f][offset]` with a time. The form
/// with a space is read day first by a `datetime` under British, French or
/// German. `None` for any other text.
pub fn sql_server_datetime(text: &str) -> Option<String> {
    let text = text.trim();
    let bytes = text.as_bytes();
    let digits =
        |range: std::ops::Range<usize>| bytes.get(range).is_some_and(|part| part.iter().all(u8::is_ascii_digit));
    if !(digits(0..4) && bytes.get(4) == Some(&b'-') && digits(5..7) && bytes.get(7) == Some(&b'-') && digits(8..10)) {
        return None;
    }
    // The first ten bytes are ASCII, so every index below is a char boundary.
    let (year, month, day) = (&text[0..4], &text[5..7], &text[8..10]);
    let rest = &text[10..];
    if rest.is_empty() {
        return Some(format!("{year}{month}{day}"));
    }
    let rest = rest.strip_prefix([' ', 'T'])?;
    let rest_bytes = rest.as_bytes();
    let at = |index: usize| rest_bytes.get(index).copied();
    let two = |index: usize| {
        at(index).is_some_and(|b| b.is_ascii_digit()) && at(index + 1).is_some_and(|b| b.is_ascii_digit())
    };
    if !(two(0) && at(2) == Some(b':') && two(3)) {
        return None;
    }
    let mut end = 5;
    let mut seconds = "00";
    if at(5) == Some(b':') {
        if !two(6) {
            return None;
        }
        seconds = &rest[6..8];
        end = 8;
    }
    let mut fraction = "";
    if end == 8 && at(8) == Some(b'.') {
        let count = rest_bytes[9..].iter().take_while(|b| b.is_ascii_digit()).count();
        if count == 0 {
            return None;
        }
        fraction = &rest[8..9 + count];
        end = 9 + count;
    }
    let offset = rest[end..].trim_start();
    let offset_ok = offset.is_empty()
        || offset == "Z"
        || (offset.len() == 6
            && offset.starts_with(['+', '-'])
            && offset.as_bytes()[1..3].iter().all(u8::is_ascii_digit)
            && offset.as_bytes()[3] == b':'
            && offset.as_bytes()[4..6].iter().all(u8::is_ascii_digit));
    if !offset_ok {
        return None;
    }
    Some(format!(
        "{year}-{month}-{day}T{}:{}:{seconds}{fraction}{offset}",
        &rest[0..2],
        &rest[3..5]
    ))
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
        Value::Date(_) | Value::Timestamp(_) | Value::TimestampTz { .. } if grammar == SqlGrammar::SqlServer => {
            let text = display(value);
            quoted(grammar, &sql_server_datetime(&text).unwrap_or(text))
        }
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
            (Value::Date(0), "'1970-01-01'", "N'19700101'", "'1970-01-01'"),
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
    fn hex_text_reads_as_its_bytes_with_its_prefix_only() {
        assert_eq!(parse_hex("0x00fF10"), Some(vec![0x00, 0xff, 0x10]));
        assert_eq!(parse_hex(" 0X0a "), Some(vec![0x0a]));
        assert_eq!(parse_hex("\\x0a"), Some(vec![0x0a]));
        assert_eq!(parse_hex("0x"), Some(Vec::new()));
        assert_eq!(parse_hex("00ff"), None, "no prefix: text, not bytes");
        assert_eq!(parse_hex("0x0"), None, "half a byte");
        assert_eq!(parse_hex("0xzz"), None);
        assert_eq!(parse_hex("0xé1"), None);
    }

    #[test]
    fn sql_server_dates_and_times_are_written_so_every_language_reads_them_alike() {
        let stamp = |text: &str| sql_server_datetime(text);
        assert_eq!(stamp("2024-01-02").as_deref(), Some("20240102"));
        assert_eq!(stamp("2024-01-02 10:00").as_deref(), Some("2024-01-02T10:00:00"));
        assert_eq!(stamp(" 2024-01-02T10:00:05 ").as_deref(), Some("2024-01-02T10:00:05"));
        assert_eq!(
            stamp("2024-01-02 10:00:05.1234567").as_deref(),
            Some("2024-01-02T10:00:05.1234567")
        );
        assert_eq!(
            stamp("2024-01-02 10:00:05+07:00").as_deref(),
            Some("2024-01-02T10:00:05+07:00")
        );
        assert_eq!(
            stamp("2024-01-02 10:00:05 -05:30").as_deref(),
            Some("2024-01-02T10:00:05-05:30")
        );
        assert_eq!(stamp("2024-01-02 10:00:05Z").as_deref(), Some("2024-01-02T10:00:05Z"));
        for other in [
            "02/01/2024",
            "2024-1-2",
            "2024-01-02 10",
            "2024-01-02 10:00:05.",
            "2024-01-02x",
            "hôm nay",
            "",
        ] {
            assert_eq!(stamp(other), None, "{other}");
        }
        let micros = (19_724 * 86_400 + 36_000) * 1_000_000 + 123_000;
        assert_eq!(
            value_literal(SqlGrammar::SqlServer, &Value::Timestamp(micros)).as_deref(),
            Some("N'2024-01-02T10:00:00.123'")
        );
        assert_eq!(
            value_literal(
                SqlGrammar::SqlServer,
                &Value::TimestampTz {
                    micros,
                    offset_seconds: 7 * 3600
                }
            )
            .as_deref(),
            Some("N'2024-01-02T17:00:00.123+07:00'")
        );
        assert_eq!(
            value_literal(SqlGrammar::Postgres, &Value::Timestamp(micros)).as_deref(),
            Some("'2024-01-02 10:00:00.123'")
        );
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
