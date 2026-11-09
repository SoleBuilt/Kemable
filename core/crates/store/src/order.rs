//! The order the core sorts a result's rows in, the same for every UI
//! (`docs/decisions/0005-sorting-a-result.md`, Order of the rows held).
//!
//! - NULL first; descending is the comparison reversed, so NULL last.
//! - A column holding several kinds (SQLite) orders by kind first: NULL,
//!   booleans, numbers, dates and times, text, bytes.
//! - Numbers compare exactly across integer and decimal kinds; a float
//!   compares by value, NaN after every number.
//! - Dates and timestamps compare by instant; a date is its midnight.
//! - Text compares by code point, with runs of digits by their value
//!   (`item2` before `item10`), case counting. UUIDs order as their text.
//! - Bytes compare by content.
//!
//! The sort is stable: equal rows keep their arrival order.

use std::cmp::Ordering;

use paboable_core::value::ValueKind;

use crate::store::kind_from_u8;

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// One cell, as the order reads it, borrowing the store's bytes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SortCell<'a> {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Decimal(&'a str),
    /// Microseconds: a date at midnight, a time of day, a timestamp.
    Instant(i64),
    Text(&'a str),
    Bytes(&'a [u8]),
}

impl<'a> SortCell<'a> {
    pub(crate) fn read(kind: u8, raw: i64, bytes: &'a [u8], null: bool) -> Self {
        if null {
            return Self::Null;
        }
        let text = || std::str::from_utf8(bytes).unwrap_or("");
        match kind_from_u8(kind) {
            ValueKind::Null => Self::Null,
            ValueKind::Bool => Self::Bool(raw != 0),
            ValueKind::Int => Self::Int(raw),
            ValueKind::UInt => Self::UInt(u64::from_ne_bytes(raw.to_ne_bytes())),
            ValueKind::Float => Self::Float(f64::from_bits(u64::from_ne_bytes(raw.to_ne_bytes()))),
            ValueKind::Float32 => Self::Float(f64::from(f32::from_bits(u32::try_from(raw).unwrap_or(0)))),
            ValueKind::Decimal => Self::Decimal(text()),
            ValueKind::Date => Self::Instant(raw.saturating_mul(MICROS_PER_DAY)),
            ValueKind::Time | ValueKind::Timestamp | ValueKind::TimestampTz => Self::Instant(raw),
            ValueKind::Bytes => Self::Bytes(bytes),
            ValueKind::Text | ValueKind::Json | ValueKind::Uuid | ValueKind::Other => Self::Text(text()),
        }
    }

    fn class(self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Int(_) | Self::UInt(_) | Self::Float(_) | Self::Decimal(_) => 2,
            Self::Instant(_) => 3,
            Self::Text(_) => 4,
            Self::Bytes(_) => 5,
        }
    }
}

pub(crate) fn compare(a: SortCell<'_>, b: SortCell<'_>) -> Ordering {
    use SortCell::{Bool, Bytes, Decimal, Float, Instant, Int, Text, UInt};
    match (a, b) {
        (Bool(x), Bool(y)) => x.cmp(&y),
        (Int(x), Int(y)) => x.cmp(&y),
        (UInt(x), UInt(y)) => x.cmp(&y),
        (Int(x), UInt(y)) => i128::from(x).cmp(&i128::from(y)),
        (UInt(x), Int(y)) => i128::from(x).cmp(&i128::from(y)),
        (Instant(x), Instant(y)) => x.cmp(&y),
        (Text(x), Text(y)) => natural(x, y),
        (Bytes(x), Bytes(y)) => x.cmp(y),
        (Decimal(x), Decimal(y)) => decimal_text(x, y),
        (Decimal(x), Int(y)) => decimal_text(x, &y.to_string()),
        (Int(x), Decimal(y)) => decimal_text(&x.to_string(), y),
        (Decimal(x), UInt(y)) => decimal_text(x, &y.to_string()),
        (UInt(x), Decimal(y)) => decimal_text(&x.to_string(), y),
        (Float(_), _) | (_, Float(_)) if a.class() == 2 && b.class() == 2 => float(as_f64(a), as_f64(b)),
        _ => a.class().cmp(&b.class()),
    }
}

fn as_f64(cell: SortCell<'_>) -> f64 {
    match cell {
        SortCell::Int(value) => value as f64,
        SortCell::UInt(value) => value as f64,
        SortCell::Float(value) => value,
        SortCell::Decimal(text) => decimal_f64(text),
        _ => f64::NAN,
    }
}

fn decimal_f64(text: &str) -> f64 {
    match text {
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        other => other.parse().unwrap_or(f64::NAN),
    }
}

/// By value, NaN after every number.
fn float(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

/// Two decimal texts compared exactly, at any length. `NaN` and the
/// infinities compare as floats do.
pub(crate) fn decimal_text(a: &str, b: &str) -> Ordering {
    let (Some(x), Some(y)) = (Decimal::parse(a), Decimal::parse(b)) else {
        return float(decimal_f64(a), decimal_f64(b));
    };
    match (x.negative, y.negative) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) => x.magnitude(&y),
        (true, true) => y.magnitude(&x),
    }
}

struct Decimal<'a> {
    negative: bool,
    /// Without leading zeros.
    whole: &'a str,
    /// Without trailing zeros.
    fraction: &'a str,
}

impl<'a> Decimal<'a> {
    fn parse(text: &'a str) -> Option<Self> {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        if whole.is_empty() && fraction.is_empty() || !whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let whole = whole.trim_start_matches('0');
        let fraction = fraction.trim_end_matches('0');
        // -0 and 0 are the same number.
        let negative = negative && !(whole.is_empty() && fraction.is_empty());
        Some(Self {
            negative,
            whole,
            fraction,
        })
    }

    fn magnitude(&self, other: &Self) -> Ordering {
        self.whole
            .len()
            .cmp(&other.whole.len())
            .then_with(|| self.whole.cmp(other.whole))
            .then_with(|| self.fraction.cmp(other.fraction))
    }
}

/// Code points, with each run of ASCII digits compared by its value.
pub(crate) fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run_a = a.iter().take_while(|b| b.is_ascii_digit()).count();
                let run_b = b.iter().take_while(|b| b.is_ascii_digit()).count();
                let (digits_a, digits_b) = (&a[..run_a], &b[..run_b]);
                let trimmed_a = trim_zeros(digits_a);
                let trimmed_b = trim_zeros(digits_b);
                let order = trimmed_a
                    .len()
                    .cmp(&trimmed_b.len())
                    .then_with(|| trimmed_a.cmp(trimmed_b))
                    // Equal values: fewer leading zeros first, so the order is total.
                    .then_with(|| run_a.cmp(&run_b));
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[run_a..];
                b = &b[run_b..];
            }
            // UTF-8 bytes order as their code points do.
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn trim_zeros(digits: &[u8]) -> &[u8] {
    let start = digits.iter().position(|&b| b != b'0').unwrap_or(digits.len());
    &digits[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_runs_compare_by_value() {
        let mut words = vec!["item10", "item2", "Item1", "item02", "item"];
        words.sort_by(|a, b| natural(a, b));
        assert_eq!(words, ["Item1", "item", "item2", "item02", "item10"]);
    }

    #[test]
    fn decimals_compare_exactly_past_f64() {
        assert_eq!(
            decimal_text("12345678901234567890.1", "12345678901234567890.01"),
            Ordering::Greater
        );
        assert_eq!(decimal_text("-1.5", "-1.25"), Ordering::Less);
        assert_eq!(decimal_text("0.50", "0.5"), Ordering::Equal);
        assert_eq!(decimal_text("-0", "0"), Ordering::Equal);
        assert_eq!(decimal_text("NaN", "1e308"), Ordering::Greater);
    }

    #[test]
    fn kinds_mix_by_class_then_value() {
        let cells = [
            SortCell::Text("b"),
            SortCell::Int(10),
            SortCell::Null,
            SortCell::Float(f64::NAN),
            SortCell::Decimal("9.5"),
            SortCell::Bytes(b"\x00"),
            SortCell::Float(-1.0),
            SortCell::Bool(true),
        ];
        let mut sorted = cells.to_vec();
        sorted.sort_by(|a, b| compare(*a, *b));
        let classes: Vec<u8> = sorted.iter().map(|cell| cell.class()).collect();
        assert_eq!(classes, [0, 1, 2, 2, 2, 2, 4, 5]);
        assert!(matches!(sorted[2], SortCell::Float(value) if value == -1.0));
        assert!(matches!(sorted[3], SortCell::Decimal("9.5")));
        assert!(matches!(sorted[4], SortCell::Int(10)));
        assert!(matches!(sorted[5], SortCell::Float(value) if value.is_nan()));
    }

    #[test]
    fn a_date_is_its_midnight_among_timestamps() {
        let date = SortCell::Instant(MICROS_PER_DAY);
        let earlier = SortCell::Instant(MICROS_PER_DAY - 1);
        assert_eq!(compare(earlier, date), Ordering::Less);
    }
}
