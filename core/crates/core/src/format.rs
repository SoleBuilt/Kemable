//! The exact, locale-free text a grid shows for a value.
//!
//! Locale, grouping and the NULL placeholder belong to the UI. This text is
//! what the UI falls back to and what copy and export write, so it never
//! rounds: decimals keep the engine's digits, floats print the shortest text
//! that reads back to the same bits, and times keep their microseconds.

use std::fmt::Write as _;

use crate::value::Value;

const MICROS_PER_SECOND: i64 = 1_000_000;
const MICROS_PER_DAY: i64 = 86_400 * MICROS_PER_SECOND;

/// Appends the display text of `value` to `out`. Writes nothing for
/// [`Value::Null`] and [`Value::Bytes`]: a null has no text, and bytes are
/// shown from their raw form ([`hex_preview`]).
pub fn write_display(value: &Value, out: &mut String) {
    match value {
        Value::Null | Value::Bytes(_) => {}
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Int(value) => push_display(out, value),
        Value::UInt(value) => push_display(out, value),
        Value::Float(value) => write_float(*value, out),
        Value::Float32(value) => write_float32(*value, out),
        Value::Decimal(text) | Value::Text(text) | Value::Json(text) | Value::Other(text) => out.push_str(text),
        Value::Uuid(bytes) => write_uuid(bytes, out),
        Value::Date(days) => write_date(i64::from(*days), out),
        Value::Time(micros) => write_time_of_day(*micros, out),
        Value::Timestamp(micros) => write_timestamp(*micros, out),
        Value::TimestampTz { micros, offset_seconds } => {
            let local = micros.saturating_add(i64::from(*offset_seconds) * MICROS_PER_SECOND);
            write_timestamp(local, out);
            write_offset(*offset_seconds, out);
        }
    }
}

pub fn display(value: &Value) -> String {
    let mut out = String::new();
    write_display(value, &mut out);
    out
}

/// `0x` and the hex of the leading bytes, at most `max_chars` characters.
/// Returns whether bytes were left out.
pub fn hex_preview(bytes: &[u8], max_chars: usize, out: &mut String) -> bool {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let room = max_chars.saturating_sub(2) / 2;
    let shown = bytes.len().min(room);
    out.push_str("0x");
    for byte in &bytes[..shown] {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    shown < bytes.len()
}

fn push_display(out: &mut String, value: &impl std::fmt::Display) {
    // Writing to a String cannot fail.
    let _ = write!(out, "{value}");
}

fn write_float(value: f64, out: &mut String) {
    if value.is_nan() {
        out.push_str("NaN");
    } else if value.is_infinite() {
        out.push_str(if value > 0.0 { "Infinity" } else { "-Infinity" });
    } else {
        // Rust's Display for f64 is the shortest text that round-trips.
        push_display(out, &value);
    }
}

fn write_float32(value: f32, out: &mut String) {
    if value.is_nan() {
        out.push_str("NaN");
    } else if value.is_infinite() {
        out.push_str(if value > 0.0 { "Infinity" } else { "-Infinity" });
    } else {
        push_display(out, &value);
    }
}

fn write_uuid(bytes: &[u8; 16], out: &mut String) {
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{byte:02x}");
    }
}

/// Year, month and day of a day number, proleptic Gregorian, from Howard
/// Hinnant's `civil_from_days`. Year 0 is 1 BC, as ISO 8601 counts.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    // Both are in range by construction: month 1..=12, day 1..=31.
    (year, u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

fn write_date(days: i64, out: &mut String) {
    let (year, month, day) = civil_from_days(days);
    if (0..=9999).contains(&year) {
        let _ = write!(out, "{year:04}-{month:02}-{day:02}");
    } else {
        // ISO 8601 expanded years carry a sign.
        let sign = if year < 0 { '-' } else { '+' };
        let _ = write!(out, "{sign}{:04}-{month:02}-{day:02}", year.unsigned_abs());
    }
}

fn write_time_of_day(micros: i64, out: &mut String) {
    let micros = micros.rem_euclid(MICROS_PER_DAY);
    let seconds = micros / MICROS_PER_SECOND;
    let fraction = micros % MICROS_PER_SECOND;
    let _ = write!(
        out,
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if fraction != 0 {
        let digits = format!("{fraction:06}");
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
}

fn write_timestamp(micros: i64, out: &mut String) {
    write_date(micros.div_euclid(MICROS_PER_DAY), out);
    out.push(' ');
    write_time_of_day(micros.rem_euclid(MICROS_PER_DAY), out);
}

fn write_offset(offset_seconds: i32, out: &mut String) {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let total = offset_seconds.unsigned_abs();
    let _ = write!(out, "{sign}{:02}:{:02}", total / 3600, total / 60 % 60);
    if !total.is_multiple_of(60) {
        let _ = write!(out, ":{:02}", total % 60);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_around_the_epoch() {
        assert_eq!(display(&Value::Date(0)), "1970-01-01");
        assert_eq!(display(&Value::Date(-1)), "1969-12-31");
        assert_eq!(display(&Value::Date(19_723)), "2024-01-01");
        assert_eq!(display(&Value::Date(19_782)), "2024-02-29");
    }

    #[test]
    fn dates_outside_four_digit_years_carry_a_sign() {
        assert_eq!(display(&Value::Date(-719_528)), "0000-01-01");
        assert_eq!(display(&Value::Date(-719_529)), "-0001-12-31");
        assert_eq!(display(&Value::Date(2_932_897)), "+10000-01-01");
    }

    #[test]
    fn timestamps_keep_microseconds_and_trim_trailing_zeros() {
        assert_eq!(display(&Value::Timestamp(1_700_000_000_000_000)), "2023-11-14 22:13:20");
        assert_eq!(
            display(&Value::Timestamp(1_700_000_000_304_865)),
            "2023-11-14 22:13:20.304865"
        );
        assert_eq!(
            display(&Value::Timestamp(1_700_000_000_300_000)),
            "2023-11-14 22:13:20.3"
        );
        assert_eq!(display(&Value::Timestamp(-1)), "1969-12-31 23:59:59.999999");
    }

    #[test]
    fn timestamps_with_zone_show_local_time_and_offset() {
        let value = Value::TimestampTz {
            micros: 1_700_000_000_000_000,
            offset_seconds: 7 * 3600,
        };
        assert_eq!(display(&value), "2023-11-15 05:13:20+07:00");
        let value = Value::TimestampTz {
            micros: 0,
            offset_seconds: -(5 * 3600 + 30 * 60),
        };
        assert_eq!(display(&value), "1969-12-31 18:30:00-05:30");
    }

    #[test]
    fn floats_round_trip_and_name_the_specials() {
        assert_eq!(display(&Value::Float(0.1)), "0.1");
        assert_eq!(display(&Value::Float(1e300)), format!("{}", 1e300));
        assert_eq!(display(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(display(&Value::Float(f64::NEG_INFINITY)), "-Infinity");
    }

    #[test]
    fn single_precision_floats_print_their_own_shortest_text() {
        assert_eq!(display(&Value::Float32(0.1)), "0.1");
        assert_eq!(
            display(&Value::Float32(-3.4e38)),
            "-340000000000000000000000000000000000000"
        );
        assert_eq!(display(&Value::Float32(f32::INFINITY)), "Infinity");
    }

    #[test]
    fn decimals_keep_every_digit() {
        let text = "2553875341786898348777948.6813196487";
        assert_eq!(display(&Value::Decimal(text.into())), text);
    }

    #[test]
    fn uuids_are_lowercase_with_dashes() {
        let bytes = [
            0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x12, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40, 0x00,
        ];
        assert_eq!(display(&Value::Uuid(bytes)), "123e4567-e89b-12d3-a456-426614174000");
    }

    #[test]
    fn hex_preview_cuts_at_the_cap() {
        let mut out = String::new();
        assert!(!hex_preview(&[0xde, 0xad], 10, &mut out));
        assert_eq!(out, "0xdead");
        out.clear();
        assert!(hex_preview(&[1, 2, 3, 4, 5], 8, &mut out));
        assert_eq!(out, "0x010203");
    }
}
