//! PostgreSQL's binary wire values to [`Value`], exactly.
//!
//! Each built-in type decodes by its OID. Arrays, ranges, intervals, network
//! and geometric types become `Value::Other` with the text psql shows for
//! them, so a grid reads the same as the server's own client. A type this
//! module does not know (an extension's) is shown as text when its bytes are
//! printable text, else as bytes.

use std::fmt::Write as _;

use fallible_iterator::FallibleIterator;
use paboable_core::catalog::ColumnMeta;
use paboable_core::format;
use paboable_core::value::{Value, ValueKind};
use postgres_protocol::types as wire;
use tokio_postgres::types::{FromSql, Kind, Type};

/// Days from 1970-01-01 to PostgreSQL's epoch, 2000-01-01.
const EPOCH_DAYS: i64 = 10_957;
/// Microseconds from 1970-01-01 to 2000-01-01.
const EPOCH_MICROS: i64 = EPOCH_DAYS * 86_400_000_000;

/// Built-in type OIDs, fixed by PostgreSQL's catalog.
mod oid {
    pub const BOOL: u32 = 16;
    pub const BYTEA: u32 = 17;
    pub const CHAR: u32 = 18;
    pub const NAME: u32 = 19;
    pub const INT8: u32 = 20;
    pub const INT2: u32 = 21;
    pub const INT4: u32 = 23;
    pub const REGPROC: u32 = 24;
    pub const TEXT: u32 = 25;
    pub const OID: u32 = 26;
    pub const XID: u32 = 28;
    pub const CID: u32 = 29;
    pub const JSON: u32 = 114;
    pub const XML: u32 = 142;
    pub const POINT: u32 = 600;
    pub const CIDR: u32 = 650;
    pub const FLOAT4: u32 = 700;
    pub const FLOAT8: u32 = 701;
    pub const UNKNOWN: u32 = 705;
    pub const MACADDR8: u32 = 774;
    pub const MONEY: u32 = 790;
    pub const MACADDR: u32 = 829;
    pub const INET: u32 = 869;
    pub const BPCHAR: u32 = 1042;
    pub const VARCHAR: u32 = 1043;
    pub const DATE: u32 = 1082;
    pub const TIME: u32 = 1083;
    pub const TIMESTAMP: u32 = 1114;
    pub const TIMESTAMPTZ: u32 = 1184;
    pub const INTERVAL: u32 = 1186;
    pub const TIMETZ: u32 = 1266;
    pub const BIT: u32 = 1560;
    pub const VARBIT: u32 = 1562;
    pub const NUMERIC: u32 = 1700;
    pub const REGPROCEDURE: u32 = 2202;
    pub const REGOPER: u32 = 2203;
    pub const REGOPERATOR: u32 = 2204;
    pub const REGCLASS: u32 = 2205;
    pub const REGTYPE: u32 = 2206;
    pub const UUID: u32 = 2950;
    pub const PG_LSN: u32 = 3220;
    pub const REGCONFIG: u32 = 3734;
    pub const REGDICTIONARY: u32 = 3769;
    pub const JSONB: u32 = 3802;
    pub const REGNAMESPACE: u32 = 4089;
    pub const REGROLE: u32 = 4096;
}

/// A column's raw bytes, whatever its type, so every type reaches [`decode`].
pub(crate) struct Raw<'a>(pub(crate) &'a [u8]);

impl<'a> FromSql<'a> for Raw<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Raw(raw))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

pub(crate) fn column_meta(name: &str, ty: &Type) -> ColumnMeta {
    ColumnMeta {
        name: name.to_owned(),
        type_name: ty.name().to_owned(),
        kind: kind_of(ty),
    }
}

/// The kind most values of a column of this type have.
pub(crate) fn kind_of(ty: &Type) -> ValueKind {
    match ty.kind() {
        Kind::Domain(base) => return kind_of(base),
        Kind::Enum(_) => return ValueKind::Text,
        Kind::Simple => {}
        _ => return ValueKind::Other,
    }
    match ty.oid() {
        oid::BOOL => ValueKind::Bool,
        oid::INT2 | oid::INT4 | oid::INT8 | oid::OID | oid::XID | oid::CID => ValueKind::Int,
        oid::FLOAT4 => ValueKind::Float32,
        oid::FLOAT8 => ValueKind::Float,
        oid::NUMERIC | oid::MONEY => ValueKind::Decimal,
        oid::TEXT | oid::VARCHAR | oid::BPCHAR | oid::NAME | oid::CHAR | oid::XML | oid::UNKNOWN => ValueKind::Text,
        oid::BYTEA => ValueKind::Bytes,
        oid::UUID => ValueKind::Uuid,
        oid::JSON | oid::JSONB => ValueKind::Json,
        oid::DATE => ValueKind::Date,
        oid::TIME => ValueKind::Time,
        oid::TIMESTAMP => ValueKind::Timestamp,
        oid::TIMESTAMPTZ => ValueKind::TimestampTz,
        _ => ValueKind::Other,
    }
}

pub(crate) fn decode(ty: &Type, raw: &[u8]) -> Value {
    decode_known(ty, raw).unwrap_or_else(|| unknown(raw))
}

fn decode_known(ty: &Type, raw: &[u8]) -> Option<Value> {
    match ty.kind() {
        Kind::Domain(base) => return decode_known(base, raw),
        Kind::Enum(_) => return Some(Value::Text(utf8(raw)?)),
        Kind::Array(element) => return Some(Value::Other(array_text(element, raw)?)),
        Kind::Range(element) => return Some(Value::Other(range_text(element, raw)?)),
        Kind::Simple => {}
        _ => return None,
    }
    let value = match ty.oid() {
        oid::BOOL => Value::Bool(wire::bool_from_sql(raw).ok()?),
        oid::INT2 => Value::Int(i64::from(wire::int2_from_sql(raw).ok()?)),
        oid::INT4 => Value::Int(i64::from(wire::int4_from_sql(raw).ok()?)),
        oid::INT8 => Value::Int(wire::int8_from_sql(raw).ok()?),
        oid::OID
        | oid::XID
        | oid::CID
        | oid::REGPROC
        | oid::REGPROCEDURE
        | oid::REGOPER
        | oid::REGOPERATOR
        | oid::REGCLASS
        | oid::REGTYPE
        | oid::REGCONFIG
        | oid::REGDICTIONARY
        | oid::REGNAMESPACE
        | oid::REGROLE => Value::Int(i64::from(wire::oid_from_sql(raw).ok()?)),
        oid::FLOAT4 => Value::Float32(wire::float4_from_sql(raw).ok()?),
        oid::FLOAT8 => Value::Float(wire::float8_from_sql(raw).ok()?),
        oid::NUMERIC => Value::Decimal(numeric_text(raw)?),
        oid::MONEY => Value::Decimal(money_text(wire::int8_from_sql(raw).ok()?)),
        oid::TEXT | oid::VARCHAR | oid::BPCHAR | oid::NAME | oid::XML | oid::UNKNOWN => Value::Text(utf8(raw)?),
        oid::CHAR => Value::Text(char::from(*raw.first()?).to_string()),
        oid::BYTEA => Value::Bytes(wire::bytea_from_sql(raw).to_vec()),
        oid::UUID => Value::Uuid(wire::uuid_from_sql(raw).ok()?),
        oid::JSON => Value::Json(utf8(raw)?),
        oid::JSONB => match raw.split_first() {
            Some((1, document)) => Value::Json(utf8(document)?),
            _ => return None,
        },
        oid::DATE => date(wire::date_from_sql(raw).ok()?),
        oid::TIME => Value::Time(wire::time_from_sql(raw).ok()?),
        oid::TIMESTAMP => timestamp(wire::timestamp_from_sql(raw).ok()?, false),
        oid::TIMESTAMPTZ => timestamp(wire::timestamp_from_sql(raw).ok()?, true),
        oid::TIMETZ => Value::Other(timetz_text(raw)?),
        oid::INTERVAL => Value::Other(interval_text(raw)?),
        oid::INET | oid::CIDR => Value::Other(inet_text(raw, ty.oid() == oid::CIDR)?),
        oid::MACADDR | oid::MACADDR8 => Value::Other(mac_text(raw, if ty.oid() == oid::MACADDR { 6 } else { 8 })?),
        oid::POINT => {
            let point = wire::point_from_sql(raw).ok()?;
            Value::Other(format!("({},{})", point.x(), point.y()))
        }
        oid::BIT | oid::VARBIT => Value::Other(bits_text(raw)?),
        oid::PG_LSN => {
            let lsn = wire::lsn_from_sql(raw).ok()?;
            Value::Other(format!("{:X}/{:X}", lsn >> 32, lsn & 0xffff_ffff))
        }
        _ => return None,
    };
    Some(value)
}

/// A type this module does not know: its bytes as text when they are
/// printable (citext and most text-like extension types), else as bytes.
fn unknown(raw: &[u8]) -> Value {
    match std::str::from_utf8(raw) {
        Ok(text) if !text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) => {
            Value::Text(text.to_owned())
        }
        _ => Value::Bytes(raw.to_vec()),
    }
}

fn utf8(raw: &[u8]) -> Option<String> {
    std::str::from_utf8(raw).ok().map(str::to_owned)
}

fn date(days: i32) -> Value {
    match days {
        i32::MAX => Value::Other("infinity".into()),
        i32::MIN => Value::Other("-infinity".into()),
        days => match i32::try_from(i64::from(days) + EPOCH_DAYS) {
            Ok(unix) => Value::Date(unix),
            Err(_) => Value::Other(format!("{days} days after 2000-01-01")),
        },
    }
}

fn timestamp(micros: i64, with_zone: bool) -> Value {
    match micros {
        i64::MAX => Value::Other("infinity".into()),
        i64::MIN => Value::Other("-infinity".into()),
        micros => match micros.checked_add(EPOCH_MICROS) {
            // A timestamptz arrives in UTC; the grid shows it with its offset.
            Some(unix) if with_zone => Value::TimestampTz {
                micros: unix,
                offset_seconds: 0,
            },
            Some(unix) => Value::Timestamp(unix),
            None => Value::Other(format!("{micros} µs after 2000-01-01")),
        },
    }
}

/// `numeric`'s binary form: digit count, weight, sign and display scale, then
/// base-10000 digits. Written as PostgreSQL's `get_str_from_var` does, so the
/// text has exactly the scale the value carries.
pub(crate) fn numeric_text(raw: &[u8]) -> Option<String> {
    let u16_at = |at: usize| raw.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let count = usize::from(u16_at(0)?);
    let weight = i32::from(i16::from_be_bytes(u16_at(2)?.to_be_bytes()));
    let sign = u16_at(4)?;
    let scale = usize::from(u16_at(6)?);
    match sign {
        0x0000 | 0x4000 => {}
        0xC000 => return Some("NaN".into()),
        0xD000 => return Some("Infinity".into()),
        0xF000 => return Some("-Infinity".into()),
        _ => return None,
    }
    let digits: Vec<u16> = (0..count).map(|index| u16_at(8 + index * 2)).collect::<Option<_>>()?;
    let digit = |index: i32| {
        usize::try_from(index)
            .ok()
            .and_then(|i| digits.get(i))
            .copied()
            .unwrap_or(0)
    };

    let mut out = String::new();
    if sign == 0x4000 {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        for index in 0..=weight {
            if index == 0 {
                let _ = write!(out, "{}", digit(index));
            } else {
                let _ = write!(out, "{:04}", digit(index));
            }
        }
    }
    if scale > 0 {
        out.push('.');
        let mut fraction = String::new();
        let mut index = weight + 1;
        while fraction.len() < scale {
            let _ = write!(fraction, "{:04}", digit(index));
            index += 1;
        }
        fraction.truncate(scale);
        out.push_str(&fraction);
    }
    Some(out)
}

/// `money` in cents, written with two decimals and no currency symbol.
fn money_text(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// `interval` as PostgreSQL's default `postgres` style writes it
/// (`EncodeInterval`, `INTSTYLE_POSTGRES`): `1 year 2 mons 3 days 04:05:06.5`.
pub(crate) fn interval_text(raw: &[u8]) -> Option<String> {
    let micros = i64::from_be_bytes(raw.get(0..8)?.try_into().ok()?);
    let days = i32::from_be_bytes(raw.get(8..12)?.try_into().ok()?);
    let months = i32::from_be_bytes(raw.get(12..16)?.try_into().ok()?);

    let mut out = String::new();
    let mut is_zero = true;
    let mut is_before = false;
    let mut part = |value: i64, unit: &str, out: &mut String| {
        if value == 0 {
            return;
        }
        let space = if is_zero { "" } else { " " };
        let plus = if is_before && value > 0 { "+" } else { "" };
        let plural = if value == 1 { "" } else { "s" };
        let _ = write!(out, "{space}{plus}{value} {unit}{plural}");
        is_before = value < 0;
        is_zero = false;
    };
    part(i64::from(months / 12), "year", &mut out);
    part(i64::from(months % 12), "mon", &mut out);
    part(i64::from(days), "day", &mut out);

    if is_zero || micros != 0 {
        let minus = micros < 0;
        let abs = micros.unsigned_abs();
        let seconds = abs / 1_000_000;
        let fraction = abs % 1_000_000;
        let space = if is_zero { "" } else { " " };
        let sign = if minus {
            "-"
        } else if is_before {
            "+"
        } else {
            ""
        };
        let _ = write!(
            out,
            "{space}{sign}{:02}:{:02}:{:02}",
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
    Some(out)
}

/// `timetz`: microseconds since midnight, then the zone in seconds west of UTC.
fn timetz_text(raw: &[u8]) -> Option<String> {
    let micros = i64::from_be_bytes(raw.get(0..8)?.try_into().ok()?);
    let west = i32::from_be_bytes(raw.get(8..12)?.try_into().ok()?);
    let mut out = format::display(&Value::Time(micros));
    let east = -west;
    let abs = east.unsigned_abs();
    let _ = write!(out, "{}{:02}", if east < 0 { '-' } else { '+' }, abs / 3600);
    if abs % 3600 != 0 {
        let _ = write!(out, ":{:02}", abs / 60 % 60);
    }
    if abs % 60 != 0 {
        let _ = write!(out, ":{:02}", abs % 60);
    }
    Some(out)
}

fn inet_text(raw: &[u8], cidr: bool) -> Option<String> {
    let inet = wire::inet_from_sql(raw).ok()?;
    let full = if inet.addr().is_ipv4() { 32 } else { 128 };
    Some(if cidr || inet.netmask() != full {
        format!("{}/{}", inet.addr(), inet.netmask())
    } else {
        inet.addr().to_string()
    })
}

fn mac_text(raw: &[u8], len: usize) -> Option<String> {
    let bytes = raw.get(..len)?;
    Some(
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

fn bits_text(raw: &[u8]) -> Option<String> {
    let bits = wire::varbit_from_sql(raw).ok()?;
    let mut out = String::with_capacity(bits.len());
    for index in 0..bits.len() {
        let byte = bits.bytes().get(index / 8)?;
        out.push(if byte & (0x80 >> (index % 8)) != 0 { '1' } else { '0' });
    }
    Some(out)
}

/// The text of one element inside an array or a range.
fn element_text(ty: &Type, raw: &[u8]) -> String {
    match decode(ty, raw) {
        Value::Bytes(bytes) => {
            let mut out = String::from("\\x");
            for byte in bytes {
                let _ = write!(out, "{byte:02x}");
            }
            out
        }
        value => format::display(&value),
    }
}

/// Appends `text`, quoted as `array_out` and `range_out` quote: when it is
/// empty, holds whitespace or one of `specials`, or (for arrays) reads NULL.
fn push_element(out: &mut String, text: &str, specials: &str, null_word: bool) {
    let needs_quotes = text.is_empty()
        || (null_word && text.eq_ignore_ascii_case("NULL"))
        || text.chars().any(|c| c.is_whitespace() || specials.contains(c));
    if !needs_quotes {
        out.push_str(text);
        return;
    }
    out.push('"');
    for ch in text.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
}

/// An array as `array_out` writes it: `{1,NULL,3}`, nested braces per
/// dimension, and `[2:3]=` before them when a lower bound is not 1.
pub(crate) fn array_text(element: &Type, raw: &[u8]) -> Option<String> {
    let array = wire::array_from_sql(raw).ok()?;
    let dimensions: Vec<wire::ArrayDimension> = array.dimensions().collect().ok()?;
    if dimensions.is_empty() {
        return Some("{}".into());
    }
    let mut out = String::new();
    if dimensions.iter().any(|dimension| dimension.lower_bound != 1) {
        for dimension in &dimensions {
            let _ = write!(
                out,
                "[{}:{}]",
                dimension.lower_bound,
                dimension.lower_bound + dimension.len - 1
            );
        }
        out.push('=');
    }
    let mut values = array.values();
    write_level(&dimensions, 0, element, &mut values, &mut out)?;
    Some(out)
}

fn write_level(
    dimensions: &[wire::ArrayDimension],
    level: usize,
    element: &Type,
    values: &mut wire::ArrayValues<'_>,
    out: &mut String,
) -> Option<()> {
    out.push('{');
    for index in 0..dimensions.get(level)?.len {
        if index > 0 {
            out.push(',');
        }
        if level + 1 < dimensions.len() {
            write_level(dimensions, level + 1, element, values, out)?;
        } else {
            match values.next().ok()?? {
                None => out.push_str("NULL"),
                Some(bytes) => push_element(out, &element_text(element, bytes), "{},\"\\", true),
            }
        }
    }
    out.push('}');
    Some(())
}

/// A range as `range_out` writes it: `[1,5)`, `(,10]`, or `empty`.
pub(crate) fn range_text(element: &Type, raw: &[u8]) -> Option<String> {
    let (lower, upper) = match wire::range_from_sql(raw).ok()? {
        wire::Range::Empty => return Some("empty".into()),
        wire::Range::Nonempty(lower, upper) => (lower, upper),
    };
    let mut out = String::new();
    let bound = |bound: wire::RangeBound<Option<&[u8]>>, open: char, closed: char, out: &mut String, lower: bool| {
        let (marker, value) = match bound {
            wire::RangeBound::Inclusive(value) => (closed, value),
            wire::RangeBound::Exclusive(value) => (open, value),
            wire::RangeBound::Unbounded => (open, None),
        };
        if lower {
            out.push(marker);
        }
        if let Some(bytes) = value {
            push_element(out, &element_text(element, bytes), "()[],\"\\", false);
        }
        if !lower {
            out.push(marker);
        }
    };
    bound(lower, '(', '[', &mut out, true);
    out.push(',');
    bound(upper, ')', ']', &mut out, false);
    Some(out)
}
