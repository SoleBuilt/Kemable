//! tiberius' column data to [`Value`], exactly.
//!
//! | SQL Server type | Value |
//! |---|---|
//! | `bit` | `Bool` |
//! | `tinyint`, `smallint`, `int`, `bigint` | `Int` |
//! | `real` / `float` | `Float32` / `Float` |
//! | `decimal`, `numeric` | `Decimal`, every digit of the scale |
//! | `money`, `smallmoney` | `Decimal` with four places (tiberius reads them as `f64`; four places restores the stored value) |
//! | `char`, `varchar`, `nchar`, `nvarchar`, `text`, `ntext`, `xml` | `Text` |
//! | `binary`, `varbinary`, `image` | `Bytes` |
//! | `uniqueidentifier` | `Uuid` |
//! | `date` | `Date` |
//! | `datetime` | `Timestamp`, to the millisecond, as SQL Server shows it |
//! | `smalldatetime` | `Timestamp`, to the minute |
//! | `time`, `datetime2` | `Time`, `Timestamp`; a value with a seventh fractional digit (100 ns) is `Other`, in full |
//! | `datetimeoffset` | `TimestampTz`, or `Other` in full with a seventh digit |

use std::fmt::Write as _;

use paboable_core::catalog::ColumnMeta;
use paboable_core::format;
use paboable_core::value::{Value, ValueKind};
use tiberius::time::{Date, Time};
use tiberius::{Column, ColumnData, ColumnType};

/// Days from 1970-01-01 to 1900-01-01, the epoch of `datetime`.
const DAYS_1900: i64 = -25_567;
/// Days from 1970-01-01 to 0001-01-01, the epoch of `date` and `datetime2`.
const DAYS_0001: i64 = -719_162;
const MICROS_PER_DAY: i64 = 86_400_000_000;

pub(crate) fn column_meta(column: &Column) -> ColumnMeta {
    let ty = column.column_type();
    ColumnMeta {
        name: column.name().to_owned(),
        type_name: type_name(ty).to_owned(),
        kind: kind_of(ty),
    }
}

pub(crate) fn kind_of(ty: ColumnType) -> ValueKind {
    match ty {
        ColumnType::Bit | ColumnType::Bitn => ValueKind::Bool,
        ColumnType::Int1 | ColumnType::Int2 | ColumnType::Int4 | ColumnType::Int8 | ColumnType::Intn => ValueKind::Int,
        ColumnType::Float4 => ValueKind::Float32,
        ColumnType::Float8 | ColumnType::Floatn => ValueKind::Float,
        ColumnType::Decimaln | ColumnType::Numericn | ColumnType::Money | ColumnType::Money4 => ValueKind::Decimal,
        ColumnType::Guid => ValueKind::Uuid,
        ColumnType::Daten => ValueKind::Date,
        ColumnType::Timen => ValueKind::Time,
        ColumnType::Datetime | ColumnType::Datetime4 | ColumnType::Datetimen | ColumnType::Datetime2 => {
            ValueKind::Timestamp
        }
        ColumnType::DatetimeOffsetn => ValueKind::TimestampTz,
        ColumnType::BigVarBin | ColumnType::BigBinary | ColumnType::Image => ValueKind::Bytes,
        ColumnType::BigVarChar
        | ColumnType::BigChar
        | ColumnType::NVarchar
        | ColumnType::NChar
        | ColumnType::Text
        | ColumnType::NText
        | ColumnType::Xml => ValueKind::Text,
        ColumnType::Null | ColumnType::Udt | ColumnType::SSVariant => ValueKind::Other,
    }
}

/// The SQL Server name a result column's TDS type stands for. TDS carries the
/// storage type, so `int` and its nullable `intn` read the same.
fn type_name(ty: ColumnType) -> &'static str {
    match ty {
        ColumnType::Null => "null",
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 | ColumnType::Intn => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money => "money",
        ColumnType::Money4 => "smallmoney",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Datetime | ColumnType::Datetimen => "datetime",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigBinary => "binary",
        ColumnType::Image => "image",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Text => "text",
        ColumnType::NText => "ntext",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::SSVariant => "sql_variant",
    }
}

pub(crate) fn decode(ty: ColumnType, data: &ColumnData<'_>) -> Value {
    let value = match data {
        ColumnData::Bit(value) => value.map(Value::Bool),
        ColumnData::U8(value) => value.map(|v| Value::Int(i64::from(v))),
        ColumnData::I16(value) => value.map(|v| Value::Int(i64::from(v))),
        ColumnData::I32(value) => value.map(|v| Value::Int(i64::from(v))),
        ColumnData::I64(value) => value.map(Value::Int),
        ColumnData::F32(value) if matches!(ty, ColumnType::Money4) => value.map(|v| Value::Decimal(format!("{v:.4}"))),
        ColumnData::F32(value) => value.map(Value::Float32),
        ColumnData::F64(value) if matches!(ty, ColumnType::Money | ColumnType::Money4) => {
            value.map(|v| Value::Decimal(format!("{v:.4}")))
        }
        ColumnData::F64(value) => value.map(Value::Float),
        ColumnData::Numeric(value) => value.map(|n| Value::Decimal(numeric_text(n.value(), n.scale()))),
        ColumnData::String(value) => value.as_ref().map(|text| Value::Text(text.to_string())),
        ColumnData::Xml(value) => value.as_ref().map(|xml| Value::Text(xml.to_string())),
        ColumnData::Binary(value) => value.as_ref().map(|bytes| Value::Bytes(bytes.to_vec())),
        ColumnData::Guid(value) => value.map(|uuid| Value::Uuid(*uuid.as_bytes())),
        ColumnData::Date(value) => value.map(|date| Value::Date(unix_days(date))),
        ColumnData::DateTime(value) => value.map(|stamp| {
            // A third of a millisecond per tick; SQL Server shows the rounded millisecond.
            let millis = (i64::from(stamp.seconds_fragments()) * 10 + 1) / 3;
            Value::Timestamp((i64::from(stamp.days()) + DAYS_1900) * MICROS_PER_DAY + millis * 1000)
        }),
        ColumnData::SmallDateTime(value) => value.map(|stamp| {
            let minutes = i64::from(stamp.seconds_fragments());
            Value::Timestamp((i64::from(stamp.days()) + DAYS_1900) * MICROS_PER_DAY + minutes * 60_000_000)
        }),
        ColumnData::Time(value) => value.map(|time| match micros(time) {
            Some(micros) => Value::Time(micros),
            None => Value::Other(time_text(time)),
        }),
        ColumnData::DateTime2(value) => value.map(|stamp| {
            let date = stamp.date();
            let time = stamp.time();
            match micros(time) {
                Some(micros) => Value::Timestamp(i64::from(unix_days(date)) * MICROS_PER_DAY + micros),
                None => Value::Other(format!(
                    "{} {}",
                    format::display(&Value::Date(unix_days(date))),
                    time_text(time)
                )),
            }
        }),
        ColumnData::DateTimeOffset(value) => value.map(|stamp| {
            let utc = stamp.datetime2();
            let offset_seconds = i32::from(stamp.offset()) * 60;
            match micros(utc.time()) {
                Some(micros) => Value::TimestampTz {
                    micros: i64::from(unix_days(utc.date())) * MICROS_PER_DAY + micros,
                    offset_seconds,
                },
                None => Value::Other(offset_text(utc.date(), utc.time(), offset_seconds)),
            }
        }),
    };
    value.unwrap_or(Value::Null)
}

fn unix_days(date: Date) -> i32 {
    i32::try_from(i64::from(date.days()) + DAYS_0001).unwrap_or(i32::MIN)
}

/// A time of day in microseconds, when its scale loses nothing there.
fn micros(time: Time) -> Option<i64> {
    let increments = i64::try_from(time.increments()).ok()?;
    let scale = u32::from(time.scale());
    if scale <= 6 {
        Some(increments * 10_i64.pow(6 - scale))
    } else {
        let divisor = 10_i64.pow(scale - 6);
        (increments % divisor == 0).then_some(increments / divisor)
    }
}

/// A time with every fractional digit its scale holds, trailing zeros trimmed.
fn time_text(time: Time) -> String {
    let scale = u32::from(time.scale());
    let per_second = 10_u64.pow(scale);
    let seconds = time.increments() / per_second;
    let fraction = time.increments() % per_second;
    let mut out = format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60);
    if fraction != 0 {
        let digits = format!("{fraction:0width$}", width = scale as usize);
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
    out
}

/// A `datetimeoffset` with a seventh digit: local time and offset, in full.
fn offset_text(utc_date: Date, utc_time: Time, offset_seconds: i32) -> String {
    let scale = u32::from(utc_time.scale());
    let per_day = 86_400 * 10_i64.pow(scale);
    let increments = i64::try_from(utc_time.increments()).unwrap_or(0);
    let shift = i64::from(offset_seconds) * 10_i64.pow(scale);
    let total = i64::from(unix_days(utc_date)) * per_day + increments + shift;
    let days = total.div_euclid(per_day);
    let local = Time::new(u64::try_from(total.rem_euclid(per_day)).unwrap_or(0), utc_time.scale());
    let mut out = format::display(&Value::Date(i32::try_from(days).unwrap_or(i32::MIN)));
    out.push(' ');
    out.push_str(&time_text(local));
    let abs = offset_seconds.unsigned_abs();
    let _ = write!(
        out,
        "{}{:02}:{:02}",
        if offset_seconds < 0 { '-' } else { '+' },
        abs / 3600,
        abs / 60 % 60
    );
    out
}

/// `decimal` and `numeric` as SQL Server writes them: every digit of the scale.
pub(crate) fn numeric_text(value: i128, scale: u8) -> String {
    let digits = value.unsigned_abs().to_string();
    let sign = if value < 0 { "-" } else { "" };
    let scale = usize::from(scale);
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}
