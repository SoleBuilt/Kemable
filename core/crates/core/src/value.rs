use serde::Serialize;

/// One value as a driver read it.
///
/// Drivers map the engine's types to the closest variant and never format
/// for display: [`crate::format`] writes the exact, locale-free text every UI
/// shows (`docs/decisions/0002-result-windows.md`).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    /// Exact decimal text as the engine wrote it, so no digit is lost.
    Decimal(String),
    Text(String),
    Json(String),
    Bytes(Vec<u8>),
    Uuid([u8; 16]),
    /// Days since 1970-01-01, before it negative.
    Date(i32),
    /// Microseconds since midnight.
    Time(i64),
    /// Microseconds since 1970-01-01T00:00:00, no zone.
    Timestamp(i64),
    /// Microseconds since the epoch in UTC, and the offset the engine reported.
    TimestampTz {
        micros: i64,
        offset_seconds: i32,
    },
    /// An engine type with no variant here (an interval, an array, a range),
    /// in the engine's own text form.
    Other(String),
}

/// One byte per value in a result window. The numbers are part of the window
/// format the UI decodes, so a variant keeps its number forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[repr(u8)]
#[serde(rename_all = "camelCase")]
pub enum ValueKind {
    Null = 0,
    Bool = 1,
    Int = 2,
    UInt = 3,
    Float = 4,
    Decimal = 5,
    Text = 6,
    Json = 7,
    Bytes = 8,
    Uuid = 9,
    Date = 10,
    Time = 11,
    Timestamp = 12,
    TimestampTz = 13,
    Other = 14,
}

impl ValueKind {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Whether a grid aligns the value to the right.
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::Int | Self::UInt | Self::Float | Self::Decimal)
    }
}

impl Value {
    pub const fn kind(&self) -> ValueKind {
        match self {
            Self::Null => ValueKind::Null,
            Self::Bool(_) => ValueKind::Bool,
            Self::Int(_) => ValueKind::Int,
            Self::UInt(_) => ValueKind::UInt,
            Self::Float(_) => ValueKind::Float,
            Self::Decimal(_) => ValueKind::Decimal,
            Self::Text(_) => ValueKind::Text,
            Self::Json(_) => ValueKind::Json,
            Self::Bytes(_) => ValueKind::Bytes,
            Self::Uuid(_) => ValueKind::Uuid,
            Self::Date(_) => ValueKind::Date,
            Self::Time(_) => ValueKind::Time,
            Self::Timestamp(_) => ValueKind::Timestamp,
            Self::TimestampTz { .. } => ValueKind::TimestampTz,
            Self::Other(_) => ValueKind::Other,
        }
    }

    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The integer a UI can use without parsing text: the value itself for
    /// integers and booleans, the bits of a float, the day number of a date,
    /// the microseconds of a time or timestamp, and the byte length otherwise.
    pub fn raw(&self) -> i64 {
        match self {
            Self::Null => 0,
            Self::Bool(value) => i64::from(*value),
            Self::Int(value) => *value,
            Self::UInt(value) => i64::from_ne_bytes(value.to_ne_bytes()),
            Self::Float(value) => i64::from_ne_bytes(value.to_bits().to_ne_bytes()),
            Self::Date(days) => i64::from(*days),
            Self::Time(micros) | Self::Timestamp(micros) | Self::TimestampTz { micros, .. } => *micros,
            Self::Uuid(_) => 16,
            Self::Bytes(bytes) => len_i64(bytes.len()),
            Self::Decimal(text) | Self::Text(text) | Self::Json(text) | Self::Other(text) => len_i64(text.len()),
        }
    }
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}
