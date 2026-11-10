//! Which rows a filter on a result keeps, the same for every UI
//! (`docs/decisions/0011-filtering-a-result.md`).
//!
//! - A condition reads one column, by index. The conditions combine with all
//!   (AND) or any (OR).
//! - NULL matches only `isNull` and `isNotNull`: as in SQL, `notEquals` and
//!   `notContains` do not keep a NULL.
//! - `equals` through `lessOrEqual`, `in` and `between` compare by the order of
//!   the rows held (`order` module): numbers exactly across kinds, dates and
//!   timestamps by instant, text by code point with case counting.
//! - The user's value is text, read for each cell's kind (which is the
//!   column's, except in SQLite): a number (`-12.50`, `1e3`, `NaN`); a
//!   boolean (`true`, `t`, `yes`, `1`, and their opposites); a date or a
//!   date and time (`2026-01-31`, `2026-01-31 08:00:00.5`, with `T`, and an
//!   offset such as `+07:00` or `Z`); a time of day (`08:30`); bytes in hex
//!   (`0xcafe`). A timestamp with a zone and a value with no offset compare
//!   in UTC; a date is its midnight. Spaces around the value are ignored for
//!   these kinds; text compares as typed, and a UUID ignores case.
//! - A value that does not read as the cell's kind matches no such cell, for
//!   `notEquals` too: the server would refuse the condition. In `in`, such a
//!   value is skipped and the others still match.
//! - `contains`, `notContains`, `startsWith` and `endsWith` read the cell's
//!   text as the grid shows it (bytes as `0x` hex) and ignore case, by
//!   Unicode lowercase, one character at a time.

use paboable_core::DriverError;
use paboable_core::sql::{FilterOp, ResultFilter, in_items};
use paboable_core::value::ValueKind;

use crate::order::{SortCell, compare, is_decimal};
use crate::store::{Chunk, ColumnChunk, kind_from_u8};

const MICROS_PER_SECOND: i64 = 1_000_000;
const MICROS_PER_DAY: i64 = 86_400 * MICROS_PER_SECOND;

/// A filter read once, so each cell is tested without parsing the user's
/// values again.
#[derive(Debug)]
pub(crate) struct RowFilter {
    conditions: Vec<Condition>,
    match_any: bool,
}

#[derive(Debug)]
struct Condition {
    column: usize,
    test: Test,
}

#[derive(Debug)]
enum Test {
    IsNull,
    IsNotNull,
    Compare(Comparison, Operand),
    Text {
        /// Lowercase.
        needle: String,
        place: Place,
        negated: bool,
    },
    In(Vec<Operand>),
    Between(Operand, Operand),
}

#[derive(Debug, Clone, Copy)]
enum Comparison {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

#[derive(Debug, Clone, Copy)]
enum Place {
    Anywhere,
    Start,
    End,
}

/// Buffers reused from cell to cell, so a test allocates only when a cell
/// needs more room than the ones before it.
#[derive(Default)]
pub(crate) struct Scratch {
    hex: String,
    lower: String,
}

impl RowFilter {
    /// `None` for no conditions: every row is kept. Each condition is checked
    /// against a result of `width` columns first ([`ResultFilter::check`]).
    pub(crate) fn compile(
        filters: &[ResultFilter],
        match_any: bool,
        width: usize,
    ) -> Result<Option<Self>, DriverError> {
        if filters.is_empty() {
            return Ok(None);
        }
        let mut conditions = Vec::with_capacity(filters.len());
        for filter in filters {
            filter.check(width)?;
            let compare = |comparison| Test::Compare(comparison, Operand::parse(&filter.value));
            let text = |place, negated| Test::Text {
                needle: lowercase(&filter.value),
                place,
                negated,
            };
            let test = match filter.op {
                FilterOp::Equals => compare(Comparison::Equal),
                FilterOp::NotEquals => compare(Comparison::NotEqual),
                FilterOp::Greater => compare(Comparison::Greater),
                FilterOp::GreaterOrEqual => compare(Comparison::GreaterOrEqual),
                FilterOp::Less => compare(Comparison::Less),
                FilterOp::LessOrEqual => compare(Comparison::LessOrEqual),
                FilterOp::Contains => text(Place::Anywhere, false),
                FilterOp::NotContains => text(Place::Anywhere, true),
                FilterOp::StartsWith => text(Place::Start, false),
                FilterOp::EndsWith => text(Place::End, false),
                FilterOp::In => Test::In(in_items(&filter.value).map(Operand::parse).collect()),
                FilterOp::Between => Test::Between(Operand::parse(&filter.value), Operand::parse(&filter.second)),
                FilterOp::IsNull => Test::IsNull,
                FilterOp::IsNotNull => Test::IsNotNull,
                // `check` refused it above.
                FilterOp::Raw => Test::IsNull,
            };
            conditions.push(Condition {
                column: filter.column,
                test,
            });
        }
        Ok(Some(Self { conditions, match_any }))
    }

    /// Whether row `local` of `chunk` is kept.
    pub(crate) fn matches(&self, chunk: &Chunk, local: usize, scratch: &mut Scratch) -> bool {
        let holds = |condition: &Condition, scratch: &mut Scratch| {
            chunk
                .columns
                .get(condition.column)
                .is_some_and(|data| condition.test.holds(data, local, scratch))
        };
        if self.match_any {
            self.conditions.iter().any(|condition| holds(condition, scratch))
        } else {
            self.conditions.iter().all(|condition| holds(condition, scratch))
        }
    }
}

impl Test {
    fn holds(&self, data: &ColumnChunk, local: usize, scratch: &mut Scratch) -> bool {
        let Some(&kind) = data.kinds.get(local) else {
            return false;
        };
        let null = data.is_null(local) || kind == ValueKind::Null.as_u8();
        match self {
            Self::IsNull => null,
            Self::IsNotNull => !null,
            _ if null => false,
            Self::Compare(comparison, operand) => {
                compared(data, local, kind, operand).is_some_and(|order| comparison.holds(order))
            }
            Self::In(items) => items
                .iter()
                .any(|item| compared(data, local, kind, item).is_some_and(std::cmp::Ordering::is_eq)),
            Self::Between(low, high) => {
                compared(data, local, kind, low).is_some_and(std::cmp::Ordering::is_ge)
                    && compared(data, local, kind, high).is_some_and(std::cmp::Ordering::is_le)
            }
            Self::Text { needle, place, negated } => {
                let bytes = data.bytes(local);
                let text = if kind == ValueKind::Bytes.as_u8() {
                    scratch.hex.clear();
                    paboable_core::format::hex_preview(bytes, usize::MAX, &mut scratch.hex);
                    scratch.hex.as_str()
                } else {
                    std::str::from_utf8(bytes).unwrap_or("")
                };
                holds_text(text, needle, *place, &mut scratch.lower) != *negated
            }
        }
    }
}

/// How the cell compares with `operand`; `None` when the operand does not
/// read as the cell's kind.
fn compared(data: &ColumnChunk, local: usize, kind: u8, operand: &Operand) -> Option<std::cmp::Ordering> {
    let value = operand.as_cell(kind_from_u8(kind))?;
    let cell = SortCell::read(kind, data.raw[local], data.bytes(local), false);
    Some(compare(cell, value))
}

impl Comparison {
    fn holds(self, order: std::cmp::Ordering) -> bool {
        match self {
            Self::Equal => order.is_eq(),
            Self::NotEqual => order.is_ne(),
            Self::Greater => order.is_gt(),
            Self::GreaterOrEqual => order.is_ge(),
            Self::Less => order.is_lt(),
            Self::LessOrEqual => order.is_le(),
        }
    }
}

/// Whether `text`, lowercased, holds `needle` (already lowercase) at `place`.
fn holds_text(text: &str, needle: &str, place: Place, lower: &mut String) -> bool {
    if text.is_ascii() {
        // Most cells are ASCII: compare them in place. A needle with other
        // characters cannot be in ASCII text once both are lowercase.
        let (text, needle) = (text.as_bytes(), needle.as_bytes());
        let same = |part: &[u8]| part.iter().zip(needle).all(|(a, b)| a.to_ascii_lowercase() == *b);
        if needle.len() > text.len() {
            return false;
        }
        return match place {
            Place::Start => same(&text[..needle.len()]),
            Place::End => same(&text[text.len() - needle.len()..]),
            Place::Anywhere => needle.is_empty() || text.windows(needle.len()).any(same),
        };
    }
    lower.clear();
    lower.extend(text.chars().flat_map(char::to_lowercase));
    match place {
        Place::Start => lower.starts_with(needle),
        Place::End => lower.ends_with(needle),
        Place::Anywhere => lower.contains(needle),
    }
}

/// Lowercase one character at a time, as `holds_text` lowercases a cell, so
/// a word-final sigma reads the same on both sides.
fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// The user's value, read once for each kind a cell can have.
#[derive(Debug)]
struct Operand {
    text: String,
    lower: String,
    number: Option<Number>,
    boolean: Option<bool>,
    /// Microseconds of a date or date and time as written, and the offset
    /// written with it, in seconds.
    date_time: Option<(i64, Option<i64>)>,
    time: Option<i64>,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug)]
enum Number {
    Int(i64),
    UInt(u64),
    Decimal(String),
    Float(f64),
}

impl Operand {
    fn parse(text: &str) -> Self {
        let trimmed = text.trim();
        Self {
            text: text.to_owned(),
            lower: lowercase(text),
            number: number(trimmed),
            boolean: boolean(trimmed),
            date_time: date_time(trimmed),
            time: time_only(trimmed),
            bytes: hex_bytes(trimmed),
        }
    }

    fn as_cell(&self, kind: ValueKind) -> Option<SortCell<'_>> {
        match kind {
            ValueKind::Null => None,
            ValueKind::Int | ValueKind::UInt | ValueKind::Float | ValueKind::Float32 | ValueKind::Decimal => {
                self.number.as_ref().map(|number| match number {
                    Number::Int(value) => SortCell::Int(*value),
                    Number::UInt(value) => SortCell::UInt(*value),
                    Number::Decimal(text) => SortCell::Decimal(text),
                    Number::Float(value) => SortCell::Float(*value),
                })
            }
            ValueKind::Bool => self.boolean.map(SortCell::Bool),
            ValueKind::Date | ValueKind::Timestamp => self.date_time.map(|(micros, _)| SortCell::Instant(micros)),
            ValueKind::TimestampTz => self.date_time.and_then(|(micros, offset)| {
                micros
                    .checked_sub(offset.unwrap_or(0).checked_mul(MICROS_PER_SECOND)?)
                    .map(SortCell::Instant)
            }),
            ValueKind::Time => self.time.map(SortCell::Instant),
            ValueKind::Bytes => self.bytes.as_deref().map(SortCell::Bytes),
            ValueKind::Uuid => Some(SortCell::Text(&self.lower)),
            ValueKind::Text | ValueKind::Json | ValueKind::Other => Some(SortCell::Text(&self.text)),
        }
    }
}

fn number(text: &str) -> Option<Number> {
    if let Ok(value) = text.parse::<i64>() {
        return Some(Number::Int(value));
    }
    if let Ok(value) = text.parse::<u64>() {
        return Some(Number::UInt(value));
    }
    if is_decimal(text) {
        return Some(Number::Decimal(text.to_owned()));
    }
    // An exponent, NaN or an infinity: what a float column can hold.
    text.parse::<f64>().ok().map(Number::Float)
}

fn boolean(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "1" => Some(true),
        "false" | "f" | "no" | "n" | "0" => Some(false),
        _ => None,
    }
}

fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .or_else(|| text.strip_prefix("\\x"))?;
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect()
}

/// Reads digits, signs and separators from the front of a value.
struct Cursor<'a> {
    rest: &'a [u8],
}

impl Cursor<'_> {
    fn eat(&mut self, byte: u8) -> bool {
        match self.rest.split_first() {
            Some((first, rest)) if *first == byte => {
                self.rest = rest;
                true
            }
            _ => false,
        }
    }

    /// Between `min` and `max` ASCII digits as a number.
    fn digits(&mut self, min: usize, max: usize) -> Option<i64> {
        let count = self.rest.iter().take(max).take_while(|b| b.is_ascii_digit()).count();
        if count < min {
            return None;
        }
        let (digits, rest) = self.rest.split_at(count);
        self.rest = rest;
        digits.iter().try_fold(0i64, |sum, digit| {
            sum.checked_mul(10)?.checked_add(i64::from(digit - b'0'))
        })
    }

    /// `HH:MM[:SS[.fraction]]` as microseconds since midnight.
    fn time(&mut self) -> Option<i64> {
        let hour = self.digits(2, 2)?;
        if !self.eat(b':') {
            return None;
        }
        let minute = self.digits(2, 2)?;
        let (mut second, mut micros) = (0, 0);
        if self.eat(b':') {
            second = self.digits(2, 2)?;
            if self.eat(b'.') {
                let count = self.rest.iter().take_while(|b| b.is_ascii_digit()).count();
                if count == 0 {
                    return None;
                }
                // Microseconds are what the store keeps; digits past them are dropped.
                let (digits, rest) = self.rest.split_at(count);
                self.rest = rest;
                for place in 0..6 {
                    micros = micros * 10 + digits.get(place).map_or(0, |digit| i64::from(digit - b'0'));
                }
            }
        }
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some(((hour * 60 + minute) * 60 + second) * MICROS_PER_SECOND + micros)
    }

    /// Nothing, `Z`, or `±HH[:MM[:SS]]` (the colons optional), in seconds.
    fn offset(&mut self) -> Option<Option<i64>> {
        if self.rest.is_empty() {
            return Some(None);
        }
        if self.eat(b'Z') || self.eat(b'z') {
            return Some(Some(0));
        }
        let sign = if self.eat(b'+') {
            1
        } else if self.eat(b'-') {
            -1
        } else {
            return None;
        };
        let hours = self.digits(2, 2)?;
        let mut seconds = hours * 3600;
        for unit in [60, 1] {
            self.eat(b':');
            if self.rest.is_empty() {
                break;
            }
            let part = self.digits(2, 2)?;
            if part > 59 {
                return None;
            }
            seconds += part * unit;
        }
        if hours > 15 {
            return None;
        }
        Some(Some(sign * seconds))
    }
}

/// `[±]YYYY-MM-DD`, optionally followed by a space or `T`, a time and an
/// offset: the date at midnight, or the date and time, as written.
fn date_time(text: &str) -> Option<(i64, Option<i64>)> {
    let mut cursor = Cursor { rest: text.as_bytes() };
    let negative = cursor.eat(b'-');
    if !negative {
        cursor.eat(b'+');
    }
    let year = cursor.digits(4, 6)?;
    let year = if negative { -year } else { year };
    if !cursor.eat(b'-') {
        return None;
    }
    let month = cursor.digits(2, 2)?;
    if !cursor.eat(b'-') {
        return None;
    }
    let day = cursor.digits(2, 2)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let midnight = days_from_civil(year, month, day).checked_mul(MICROS_PER_DAY)?;
    if cursor.rest.is_empty() {
        return Some((midnight, None));
    }
    if !(cursor.eat(b' ') || cursor.eat(b'T') || cursor.eat(b't')) {
        return None;
    }
    let micros = midnight.checked_add(cursor.time()?)?;
    cursor.eat(b' ');
    let offset = cursor.offset()?;
    cursor.rest.is_empty().then_some((micros, offset))
}

/// A time of day alone, for a time column.
fn time_only(text: &str) -> Option<i64> {
    let mut cursor = Cursor { rest: text.as_bytes() };
    let micros = cursor.time()?;
    cursor.rest.is_empty().then_some(micros)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date, from Howard
/// Hinnant's `days_from_civil`; the inverse of `format::civil_from_days`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_reads_back_as_the_day_the_core_shows() {
        for days in [-719_529, -1, 0, 19_723, 19_782, 2_932_897] {
            let (year, month, day) = paboable_core::format::civil_from_days(days);
            assert_eq!(days_from_civil(year, i64::from(month), i64::from(day)), days);
        }
        assert_eq!(date_time("2024-02-29"), Some((19_782 * MICROS_PER_DAY, None)));
        assert_eq!(date_time("2023-02-29"), None);
        assert_eq!(date_time("+10000-01-01"), Some((2_932_897 * MICROS_PER_DAY, None)));
    }

    #[test]
    fn a_date_and_time_reads_with_its_fraction_and_offset() {
        assert_eq!(
            date_time("2023-11-15 05:13:20+07:00"),
            Some((1_700_025_200 * MICROS_PER_SECOND, Some(7 * 3600)))
        );
        assert_eq!(date_time("1970-01-01T00:00:01.5Z"), Some((1_500_000, Some(0))));
        assert_eq!(
            date_time("1970-01-01 00:00:00.1234567 -0530"),
            Some((123_456, Some(-19_800)))
        );
        assert_eq!(date_time("1970-01-01 24:00"), None);
        assert_eq!(date_time("1970-01-01 10"), None);
        assert_eq!(date_time("1970-01-01 10:00 junk"), None);
        assert_eq!(time_only("08:30"), Some(510 * 60 * MICROS_PER_SECOND));
        assert_eq!(time_only("8:30"), None);
    }

    #[test]
    fn the_other_kinds_read_as_the_cell_needs_them() {
        assert!(matches!(number(" -12 ".trim()), Some(Number::Int(-12))));
        assert!(matches!(number("18446744073709551615"), Some(Number::UInt(u64::MAX))));
        assert!(matches!(number("1.50"), Some(Number::Decimal(_))));
        assert!(matches!(number("1e3"), Some(Number::Float(value)) if value == 1000.0));
        assert!(number("12abc").is_none());
        assert_eq!(boolean("Yes"), Some(true));
        assert_eq!(boolean("0"), Some(false));
        assert_eq!(boolean("maybe"), None);
        assert_eq!(hex_bytes("0xCAfe"), Some(vec![0xca, 0xfe]));
        assert_eq!(hex_bytes("0xcaf"), None);
        assert_eq!(hex_bytes("cafe"), None);
    }

    #[test]
    fn text_matches_ignoring_case_in_ascii_and_beyond() {
        let mut lower = String::new();
        assert!(holds_text("Hello World", "o w", Place::Anywhere, &mut lower));
        assert!(holds_text("Hello", "", Place::Anywhere, &mut lower));
        assert!(holds_text("Hello", "he", Place::Start, &mut lower));
        assert!(!holds_text("Hello", "lo!", Place::End, &mut lower));
        assert!(holds_text("XIN CHÀO", &lowercase("chÀo"), Place::End, &mut lower));
        assert!(!holds_text("abc", &lowercase("À"), Place::Anywhere, &mut lower));
        assert!(holds_text("ΟΔΟΣ", &lowercase("οδοσ"), Place::Anywhere, &mut lower));
    }
}
