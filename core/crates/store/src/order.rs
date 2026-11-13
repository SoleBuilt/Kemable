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

use kemable_core::value::ValueKind;

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
        (Decimal(x), Int(y)) => integer_text(i128::from(y), |y| decimal_text(x, y)),
        (Int(x), Decimal(y)) => integer_text(i128::from(x), |x| decimal_text(x, y)),
        (Decimal(x), UInt(y)) => integer_text(i128::from(y), |y| decimal_text(x, y)),
        (UInt(x), Decimal(y)) => integer_text(i128::from(x), |x| decimal_text(x, y)),
        (Float(_), _) | (_, Float(_)) if a.class() == 2 && b.class() == 2 => float(as_f64(a), as_f64(b)),
        _ => a.class().cmp(&b.class()),
    }
}

/// An integer's decimal text, written on the stack: a filter compares every
/// cell with its value, and a heap string per cell would cost more than the
/// comparison.
fn integer_text<R>(value: i128, then: impl FnOnce(&str) -> R) -> R {
    let mut digits = [0u8; 40];
    let mut at = digits.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        digits[at] = b'0' + u8::try_from(rest % 10).unwrap_or(0);
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        at -= 1;
        digits[at] = b'-';
    }
    then(std::str::from_utf8(&digits[at..]).unwrap_or("0"))
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

/// Whether `text` is a decimal the order compares exactly: digits with an
/// optional sign and point, no exponent.
pub(crate) fn is_decimal(text: &str) -> bool {
    Decimal::parse(text).is_some()
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

// --- Sort keys ---------------------------------------------------------------
//
// A sort compares each row some twenty times. Reading both cells again for
// each comparison (find the chunk, decode the value, walk the text) is most of
// a sort's time, so each row is turned once into a key whose order as plain
// bytes is the order above. Sorting then works on keys, and reads cells again
// only for rows whose keys are the same.
//
// A key is exact when equal keys mean equal cells: one kind of number, a
// date or time, a boolean, NULL. A text's key is its first bytes only, and a
// column that mixes kinds of number (an integer and a decimal) keys them all
// alike, so those keys only narrow the order and the comparison finishes it.
//
// A key is a cell's class, then its value. Numbers and times need eight
// bytes, so their columns take the narrow key; a column with text or bytes
// takes the wide one, which holds enough of most texts to tell them apart.

/// A key for a column of numbers, dates, times and booleans.
pub(crate) const NARROW_KEY: usize = 12;
/// A key for a column that holds text or bytes.
pub(crate) const WIDE_KEY: usize = 32;

/// Whether a cell of this kind byte needs the wide key.
pub(crate) fn wants_wide_key(kind: u8) -> bool {
    matches!(
        kind_from_u8(kind),
        ValueKind::Text | ValueKind::Json | ValueKind::Uuid | ValueKind::Other | ValueKind::Bytes
    )
}

/// Which kinds of number a column held, as bits.
const INT: u8 = 1;
const UINT: u8 = 2;
const FLOAT: u8 = 4;
const DECIMAL: u8 = 8;

/// Builds the keys of one column, and says afterwards whether they are exact.
#[derive(Default)]
pub(crate) struct KeyBuilder {
    numbers: u8,
}

impl KeyBuilder {
    /// The key of `cell`, `N` bytes, at least [`NARROW_KEY`]. A text is keyed
    /// from its byte `past` on, for rows known to be alike before it
    /// ([`skippable`]). Call [`Self::settle`] on every key once the column is read.
    pub(crate) fn key<const N: usize>(&mut self, cell: SortCell<'_>, past: usize) -> [u8; N] {
        let mut key = [0u8; N];
        let Some((class, payload)) = key.split_first_mut() else {
            return key;
        };
        *class = cell.class();
        let mut eight = |bytes: [u8; 8]| {
            if let Some(slot) = payload.get_mut(..8) {
                slot.copy_from_slice(&bytes);
            }
        };
        match cell {
            SortCell::Null => {}
            SortCell::Bool(value) => eight([u8::from(value), 0, 0, 0, 0, 0, 0, 0]),
            SortCell::Int(value) => {
                self.numbers |= INT;
                eight(signed(value));
            }
            SortCell::UInt(value) => {
                self.numbers |= UINT;
                eight(value.to_be_bytes());
            }
            SortCell::Float(value) => {
                self.numbers |= FLOAT;
                eight(float_order(value).to_be_bytes());
            }
            SortCell::Decimal(_) => self.numbers |= DECIMAL,
            SortCell::Instant(value) => eight(signed(value)),
            SortCell::Text(text) => natural_prefix(text.as_bytes().get(past..).unwrap_or_default(), payload),
            SortCell::Bytes(bytes) => {
                let taken = bytes.len().min(payload.len());
                payload[..taken].copy_from_slice(&bytes[..taken]);
            }
        }
        key
    }

    /// Whether every number keyed was of one kind that a key holds whole.
    fn one_kind_of_number(&self) -> bool {
        matches!(self.numbers, 0 | INT | UINT | FLOAT)
    }

    /// Whether `cell`'s key holds all of it, so that a row with the same key
    /// has an equal cell: NULL, a boolean, a date or time, and a number when
    /// the column held one kind of number. A text's key is its start only.
    /// Ask once the whole column is keyed.
    pub(crate) fn whole(&self, cell: SortCell<'_>) -> bool {
        match cell {
            SortCell::Null | SortCell::Bool(_) | SortCell::Instant(_) => true,
            SortCell::Int(_) | SortCell::UInt(_) | SortCell::Float(_) => self.one_kind_of_number(),
            SortCell::Decimal(_) | SortCell::Text(_) | SortCell::Bytes(_) => false,
        }
    }

    /// Finishes a key once the whole column is read: numbers of several kinds
    /// compare exactly only as cells, so their keys say "a number" and no more;
    /// a descending key is every bit turned, which reverses the order, NULL last.
    pub(crate) fn settle<const N: usize>(&self, key: &mut [u8; N], descending: bool) {
        if key.first() == Some(&2) && !self.one_kind_of_number() {
            key[1..].fill(0);
        }
        if descending {
            for byte in key.iter_mut() {
                *byte = !*byte;
            }
        }
    }
}

/// A signed integer as bytes that order as it does.
fn signed(value: i64) -> [u8; 8] {
    (u64::from_ne_bytes(value.to_ne_bytes()) ^ (1 << 63)).to_be_bytes()
}

/// A float as an integer that orders as [`float`] does: by value, `-0` with
/// `0`, NaN after every number.
fn float_order(value: f64) -> u64 {
    if value.is_nan() {
        return u64::MAX;
    }
    // -0.0 == 0.0, so both take the bits of 0.0.
    let bits = if value == 0.0 { 0 } else { value.to_bits() };
    if bits >> 63 == 0 { bits | (1 << 63) } else { !bits }
}

/// The first bytes of a text's place in [`natural`] order, zero-filled.
///
/// Every byte stands for itself, except a run of digits, which becomes the
/// parts `natural` compares, in the order it compares them: how many digits
/// it has without its leading zeros, those digits, then how many leading
/// zeros. The count of digits is one byte from `'0'` (none) to `'8'`, or `'9'`
/// and the count in a byte of its own, so it sits where a digit would among
/// other characters, and a number of up to eight digits costs two bytes more
/// than its digits. A count past 254 ends the key there, which leaves the
/// rest to the comparison.
fn natural_prefix(text: &[u8], out: &mut [u8]) {
    /// A count that does not fit a byte is written as this, and ends the key.
    const TOO_MANY: usize = 255;
    let mut at = 0;
    // Writes one byte; false once the key is full.
    let mut push = |byte: u8| match out.get_mut(at) {
        Some(slot) => {
            *slot = byte;
            at += 1;
            true
        }
        None => false,
    };
    let mut i = 0;
    while i < text.len() {
        if !text[i].is_ascii_digit() {
            if !push(text[i]) {
                return;
            }
            i += 1;
            continue;
        }
        let run = text[i..].iter().take_while(|byte| byte.is_ascii_digit()).count();
        let digits = trim_zeros(&text[i..i + run]);
        let zeros = run - digits.len();
        // Up to eight digits: the count is the mark. More: '9', then the count.
        let short = u8::try_from(digits.len()).ok().filter(|count| *count < 9);
        if !push(b'0' + short.unwrap_or(9)) {
            return;
        }
        if short.is_none() {
            let count = u8::try_from(digits.len()).ok().filter(|_| digits.len() < TOO_MANY);
            if !push(count.unwrap_or(u8::MAX)) || count.is_none() {
                return;
            }
        }
        if !digits.iter().all(|&digit| push(digit)) {
            return;
        }
        let count = u8::try_from(zeros).ok().filter(|_| zeros < TOO_MANY);
        if !push(count.unwrap_or(u8::MAX)) || count.is_none() {
            return;
        }
        i += run;
    }
}

/// How many bytes two texts share at their start.
pub(crate) fn shared_start(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// How much of the `shared` bytes that some texts all start with can be left
/// out when they are keyed again, given they were already alike up to `from`:
/// all of it, less the digits it ends in. Those may be the start of numbers
/// that go on differently (`item12` and `item123` share `item12`, and it is
/// 12 against 123 that orders them), and [`natural`] compares a number whole.
/// What comes before a digit-free end is equal in every text and says nothing.
pub(crate) fn skippable(text: &[u8], shared: usize, from: usize) -> usize {
    let digits = text
        .get(from..shared)
        .unwrap_or_default()
        .iter()
        .rev()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    shared.saturating_sub(digits).max(from)
}

/// One row of a sort: its key, and its place among the rows before the
/// sort, which keeps equal rows in the order they had.
#[derive(Clone, Copy)]
pub(crate) struct Keyed<const N: usize> {
    pub(crate) key: [u8; N],
    pub(crate) place: u32,
}

/// Compares two keys eight bytes at a time, as numbers, which is the order
/// of their bytes and cheaper than a call per comparison.
pub(crate) fn key_order<const N: usize>(a: &[u8; N], b: &[u8; N]) -> Ordering {
    for (x, y) in a.chunks(8).zip(b.chunks(8)) {
        let order = match (<[u8; 8]>::try_from(x), <[u8; 8]>::try_from(y)) {
            (Ok(x), Ok(y)) => u64::from_be_bytes(x).cmp(&u64::from_be_bytes(y)),
            _ => x.cmp(y),
        };
        if order.is_ne() {
            return order;
        }
    }
    Ordering::Equal
}

/// Sorts by key alone, without comparing: a radix sort, least significant
/// byte first. Each pass deals the rows into 256 piles by one byte of the key
/// and takes the piles up in order, keeping the order rows had inside a pile,
/// so after the last pass they are in key order and equal keys are still in
/// the order they came (their `place`s rise). A byte every key shares is
/// skipped: a column of numbers below a million costs three passes, not twelve.
///
/// It is the fastest sort known for keys of a fixed, short width: its work
/// grows with the rows, not with rows times the logarithm of rows. It needs
/// a second array as large as the first.
pub(crate) fn radix_sort<const N: usize>(rows: &mut Vec<Keyed<N>>) {
    let Some(&first) = rows.first() else {
        return;
    };
    let mut differs = [0u8; N];
    for row in rows.iter() {
        for (seen, (byte, base)) in differs.iter_mut().zip(row.key.iter().zip(&first.key)) {
            *seen |= byte ^ base;
        }
    }
    let mut dealt = vec![first; rows.len()];
    for at in (0..N).rev().filter(|&at| differs[at] != 0) {
        let mut starts = [0usize; 256];
        for row in rows.iter() {
            starts[usize::from(row.key[at])] += 1;
        }
        let mut total = 0;
        for start in &mut starts {
            let count = *start;
            *start = total;
            total += count;
        }
        for row in rows.iter() {
            let slot = &mut starts[usize::from(row.key[at])];
            if let Some(place) = dealt.get_mut(*slot) {
                *place = *row;
            }
            *slot += 1;
        }
        std::mem::swap(rows, &mut dealt);
    }
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
    fn integers_compare_exactly_with_decimals_at_their_limits() {
        assert_eq!(
            compare(SortCell::Int(i64::MIN), SortCell::Decimal("-9223372036854775808.5")),
            Ordering::Greater
        );
        assert_eq!(
            compare(SortCell::UInt(u64::MAX), SortCell::Decimal("18446744073709551615.0")),
            Ordering::Equal
        );
        assert_eq!(compare(SortCell::Decimal("-0.5"), SortCell::Int(0)), Ordering::Less);
        assert_eq!(compare(SortCell::Decimal("10"), SortCell::Int(9)), Ordering::Greater);
    }

    #[test]
    fn a_date_is_its_midnight_among_timestamps() {
        let date = SortCell::Instant(MICROS_PER_DAY);
        let earlier = SortCell::Instant(MICROS_PER_DAY - 1);
        assert_eq!(compare(earlier, date), Ordering::Less);
    }

    /// Keys every cell as one column, then checks each pair both ways: keys
    /// that differ order as the cells do, and a cell whose key is whole is
    /// equal to every cell with the same key. Returns whether every key was whole.
    fn keys_agree_with_compare(cells: &[SortCell<'_>]) -> bool {
        let narrow = keys_agree::<NARROW_KEY>(cells, 0);
        assert_eq!(keys_agree::<WIDE_KEY>(cells, 0), narrow);
        narrow
    }

    /// The same, with every text keyed from its byte `past` on.
    fn keys_agree<const N: usize>(cells: &[SortCell<'_>], past: usize) -> bool {
        let mut builder = KeyBuilder::default();
        for descending in [false, true] {
            builder = KeyBuilder::default();
            let mut keys: Vec<[u8; N]> = cells.iter().map(|cell| builder.key(*cell, past)).collect();
            for key in &mut keys {
                builder.settle(key, descending);
            }
            for (a, key_a) in cells.iter().zip(&keys) {
                for (b, key_b) in cells.iter().zip(&keys) {
                    let cells_say = compare(*a, *b);
                    let cells_say = if descending { cells_say.reverse() } else { cells_say };
                    assert_eq!(key_order(key_a, key_b), key_a.cmp(key_b));
                    match key_a.cmp(key_b) {
                        Ordering::Equal => {
                            assert!(!builder.whole(*a) || cells_say.is_eq(), "{a:?} and {b:?} share a key");
                        }
                        keys_say => assert_eq!(keys_say, cells_say, "{a:?} against {b:?}, descending {descending}"),
                    }
                }
            }
        }
        cells.iter().all(|cell| builder.whole(*cell))
    }

    #[test]
    fn texts_alike_at_the_start_are_keyed_past_what_they_share_less_its_last_digits() {
        // What may be left out: all that is shared, but never part of a number.
        assert_eq!(skippable(b"item12", 6, 0), 4);
        assert_eq!(skippable(b"item12/", 7, 0), 7);
        assert_eq!(skippable(b"2024", 4, 0), 0);
        assert_eq!(skippable(b"a1b22", 5, 3), 3);
        assert_eq!(skippable(b"", 0, 0), 0);
        assert_eq!(shared_start(b"item12", b"item123"), 6);
        assert_eq!(shared_start("đường".as_bytes(), "đưa".as_bytes()), 4);

        // Each group shares a start; keyed past it, the keys still order as
        // the whole texts do, and now tell apart what 31 bytes could not.
        let groups: [&[&str]; 4] = [
            &[
                "https://example.com/khach-hang/chi-tiet/1999",
                "https://example.com/khach-hang/chi-tiet/20",
                "https://example.com/khach-hang/chi-tiet/305",
                "https://example.com/khach-hang/chi-tiet/305/sua",
                "https://example.com/khach-hang/chi-tiet/",
                "https://example.com/khach-hang/chi-tiet/0305",
            ],
            &["item12", "item123", "item12a", "item1200", "item012"],
            &["x100", "x1000", "x10", "x10 ", "x10a"],
            &["đường 5 quận 1", "đường 5 quận 10", "đường 5 quận 2", "đường 5"],
        ];
        for texts in groups {
            let shared = texts
                .iter()
                .map(|text| shared_start(texts[0].as_bytes(), text.as_bytes()))
                .min()
                .unwrap();
            let past = skippable(texts[0].as_bytes(), shared, 0);
            let cells: Vec<SortCell<'_>> = texts.iter().map(|text| SortCell::Text(text)).collect();
            keys_agree::<WIDE_KEY>(&cells, past);
            // Told apart: no two of these share a key once the start is left out.
            let mut builder = KeyBuilder::default();
            let mut keys: Vec<[u8; WIDE_KEY]> = cells.iter().map(|cell| builder.key(*cell, past)).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), texts.len(), "{texts:?} past {past}");
        }
    }

    #[test]
    fn text_keys_order_as_the_natural_comparison_does() {
        let long_number = "9".repeat(300);
        let longer_number = format!("x{}", "8".repeat(301));
        let many_zeros = format!("x{}1", "0".repeat(300));
        let texts = [
            "",
            "item",
            "item2",
            "item02",
            "item002",
            "item10",
            "item9",
            "Item1",
            "item 2",
            "item!",
            "item:",
            "item/",
            "itema",
            "x0",
            "x00",
            "x0a",
            "x",
            "x1",
            "x01a",
            "x1z",
            "x1\u{0}",
            "x1\u{1}",
            "khách 1999",
            "khách 2000",
            "khách 12345678901",
            "khách 12345678902",
            "https://example.com/a/very/long/path/1",
            "https://example.com/a/very/long/path/2",
            "đường 5",
            "Đường 5",
            "1",
            "01",
            "10",
            "9",
            "1.5",
            "1.10",
            "a1b2c3d4e5f6g7h8",
            "a1b2c3d4e5f6g7h9",
            long_number.as_str(),
            longer_number.as_str(),
            many_zeros.as_str(),
            "x1",
        ];
        let cells: Vec<SortCell<'_>> = texts.iter().map(|text| SortCell::Text(text)).collect();
        assert!(!keys_agree_with_compare(&cells), "a text's key is only its start");
    }

    #[test]
    fn number_and_time_keys_are_exact_and_order_as_the_values_do() {
        let integers: Vec<SortCell<'_>> = [i64::MIN, -9973, -1, 0, 1, 255, 256, 65_536, i64::MAX, 0, -1]
            .into_iter()
            .map(SortCell::Int)
            .chain([SortCell::Null])
            .collect();
        assert!(keys_agree_with_compare(&integers));

        let unsigned: Vec<SortCell<'_>> = [0, 1, u64::MAX, 1 << 63, (1 << 63) - 1]
            .into_iter()
            .map(SortCell::UInt)
            .collect();
        assert!(keys_agree_with_compare(&unsigned));

        let floats: Vec<SortCell<'_>> = [
            f64::NEG_INFINITY,
            -1.5,
            -f64::MIN_POSITIVE,
            -0.0,
            0.0,
            f64::MIN_POSITIVE,
            1.5,
            1e300,
            f64::INFINITY,
            f64::NAN,
            f64::NAN,
        ]
        .into_iter()
        .map(SortCell::Float)
        .chain([SortCell::Null])
        .collect();
        assert!(keys_agree_with_compare(&floats));

        let instants: Vec<SortCell<'_>> = [i64::MIN, -MICROS_PER_DAY, -1, 0, 1, MICROS_PER_DAY, i64::MAX]
            .into_iter()
            .map(SortCell::Instant)
            .chain([SortCell::Null, SortCell::Bool(false), SortCell::Bool(true)])
            .collect();
        assert!(keys_agree_with_compare(&instants));
    }

    #[test]
    fn a_radix_sort_orders_by_key_and_keeps_equal_keys_in_the_order_they_came() {
        // Values that differ in one byte, in several, and not at all, with
        // the negatives and the limits a signed key must place.
        let values = [5i64, -1, 300, 5, i64::MAX, 0, -70_000, 5, i64::MIN, 300, 65_536, -1];
        let mut builder = KeyBuilder::default();
        let mut rows: Vec<Keyed<NARROW_KEY>> = values
            .iter()
            .zip(0u32..)
            .map(|(&value, place)| Keyed {
                key: builder.key(SortCell::Int(value), 0),
                place,
            })
            .collect();
        radix_sort(&mut rows);
        let sorted: Vec<(i64, u32)> = rows.iter().map(|row| (values[row.place as usize], row.place)).collect();
        let mut expected: Vec<(i64, u32)> = values.iter().copied().zip(0u32..).collect();
        expected.sort();
        assert_eq!(sorted, expected);

        // One row, no rows, and rows that are all the same are left as they are.
        let mut same: Vec<Keyed<NARROW_KEY>> = (0u32..4)
            .map(|place| Keyed {
                key: [7; NARROW_KEY],
                place,
            })
            .collect();
        radix_sort(&mut same);
        assert_eq!(same.iter().map(|row| row.place).collect::<Vec<_>>(), [0, 1, 2, 3]);
        let mut none: Vec<Keyed<NARROW_KEY>> = Vec::new();
        radix_sort(&mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn a_column_of_several_kinds_keys_by_class_and_leaves_mixed_numbers_to_the_comparison() {
        let mixed = [
            SortCell::Null,
            SortCell::Bool(true),
            SortCell::Int(10),
            SortCell::Int(-3),
            SortCell::UInt(u64::MAX),
            SortCell::Float(9.75),
            SortCell::Float(f64::NAN),
            SortCell::Decimal("9.5"),
            SortCell::Decimal("-9223372036854775808.5"),
            SortCell::Instant(5),
            SortCell::Text("b"),
            SortCell::Text("a10"),
            SortCell::Bytes(b"\x01"),
            SortCell::Bytes(b"\x01\x00"),
            SortCell::Bytes(b""),
        ];
        assert!(!keys_agree_with_compare(&mixed));
        // Integers beside a decimal are not exact either: 10 and 10.0 are equal cells.
        assert!(!keys_agree_with_compare(&[
            SortCell::Int(10),
            SortCell::Decimal("10.0"),
            SortCell::Int(9)
        ]));
    }
}
