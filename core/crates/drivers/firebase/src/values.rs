//! Firestore's typed values (`{"integerValue": "1"}`, `{"mapValue": …}`)
//! as Kemable's values, exactly: integers stay integers, timestamps keep
//! their microseconds, maps and arrays become JSON with their typed values
//! unwrapped. And the Realtime Database's plain JSON as values.

use kemable_core::format::days_from_civil;
use kemable_core::value::{Value, ValueKind};
use serde_json::{Map, Value as Json};

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Microseconds since the epoch of an RFC 3339 time (`2026-11-14T01:02:03.456789Z`,
/// or with an offset); `None` when it does not read. Digits past the sixth are cut.
pub(crate) fn rfc3339_micros(text: &str) -> Option<i64> {
    let (date, time) = text.split_once(['T', 't', ' '])?;
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let (clock, offset_seconds) = if let Some(clock) = time.strip_suffix(['Z', 'z']) {
        (clock, 0)
    } else {
        let at = time.rfind(['+', '-'])?;
        let (clock, offset) = time.split_at(at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = offset[1..].split_once(':')?;
        (
            clock,
            sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60),
        )
    };
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut hms = whole.splitn(3, ':');
    let hour: i64 = hms.next()?.parse().ok()?;
    let minute: i64 = hms.next()?.parse().ok()?;
    let second: i64 = hms.next()?.parse().ok()?;
    let micros: i64 = format!("{:0<6}", fraction.get(..fraction.len().min(6))?).parse().ok()?;
    let seconds = hour * 3600 + minute * 60 + second - offset_seconds;
    days_from_civil(year, month, day)
        .checked_mul(MICROS_PER_DAY)?
        .checked_add(seconds.checked_mul(1_000_000)?)?
        .checked_add(micros)
}

/// Standard base64, as Firestore sends bytes; `None` when it does not read.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes().filter(|byte| !byte.is_ascii_whitespace() && *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// A typed value as plain JSON: what a map or an array holds, shown as JSON.
pub(crate) fn plain(typed: &Json) -> Json {
    let Some((kind, inner)) = typed.as_object().and_then(|object| object.iter().next()) else {
        return Json::Null;
    };
    match kind.as_str() {
        "nullValue" => Json::Null,
        "booleanValue" => inner.clone(),
        // Kept as text when it is past what a JSON number holds exactly.
        "integerValue" => inner
            .as_str()
            .and_then(|digits| digits.parse::<i64>().ok())
            .filter(|number| number.unsigned_abs() < (1 << 53))
            .map_or_else(|| inner.clone(), Json::from),
        "doubleValue" => inner.clone(),
        "timestampValue" | "stringValue" | "bytesValue" => inner.clone(),
        "referenceValue" => Json::String(reference(inner.as_str().unwrap_or_default())),
        "geoPointValue" => inner.clone(),
        "arrayValue" => Json::Array(
            inner
                .get("values")
                .and_then(Json::as_array)
                .map(|values| values.iter().map(plain).collect())
                .unwrap_or_default(),
        ),
        "mapValue" => Json::Object(plain_fields(inner.get("fields"))),
        _ => inner.clone(),
    }
}

/// A map's fields as plain JSON.
pub(crate) fn plain_fields(fields: Option<&Json>) -> Map<String, Json> {
    fields
        .and_then(Json::as_object)
        .map(|fields| {
            fields
                .iter()
                .map(|(name, value)| (name.clone(), plain(value)))
                .collect()
        })
        .unwrap_or_default()
}

/// A document's path below `documents/`, from its full resource name.
pub(crate) fn reference(name: &str) -> String {
    name.split_once("/documents/").map_or(name, |(_, path)| path).to_owned()
}

/// A typed value as a cell.
pub(crate) fn value(typed: &Json) -> Value {
    let Some((kind, inner)) = typed.as_object().and_then(|object| object.iter().next()) else {
        return Value::Null;
    };
    match kind.as_str() {
        "nullValue" => Value::Null,
        "booleanValue" => inner.as_bool().map_or(Value::Null, Value::Bool),
        "integerValue" => inner
            .as_str()
            .and_then(|digits| digits.parse().ok())
            .map_or_else(|| Value::Text(inner.to_string()), Value::Int),
        "doubleValue" => match inner {
            Json::Number(number) => number.as_f64().map_or(Value::Null, Value::Float),
            // NaN and the infinities come as strings.
            Json::String(text) => match text.as_str() {
                "NaN" => Value::Float(f64::NAN),
                "Infinity" => Value::Float(f64::INFINITY),
                "-Infinity" => Value::Float(f64::NEG_INFINITY),
                _ => Value::Text(text.clone()),
            },
            _ => Value::Null,
        },
        "timestampValue" => {
            let text = inner.as_str().unwrap_or_default();
            rfc3339_micros(text).map_or_else(
                || Value::Text(text.to_owned()),
                |micros| Value::TimestampTz {
                    micros,
                    offset_seconds: 0,
                },
            )
        }
        "stringValue" => Value::Text(inner.as_str().unwrap_or_default().to_owned()),
        "bytesValue" => {
            let text = inner.as_str().unwrap_or_default();
            base64_decode(text).map_or_else(|| Value::Text(text.to_owned()), Value::Bytes)
        }
        "referenceValue" => Value::Text(reference(inner.as_str().unwrap_or_default())),
        "geoPointValue" | "arrayValue" | "mapValue" => Value::Json(plain(typed).to_string()),
        _ => Value::Json(inner.to_string()),
    }
}

/// The kind a column of these values shows as: the first one's, or JSON.
pub(crate) fn kind_of(value: &Value) -> ValueKind {
    match value {
        Value::Null => ValueKind::Json,
        other => other.kind(),
    }
}

/// The Realtime Database's plain JSON as a cell: numbers as integers when
/// they are whole, objects and arrays as JSON.
pub(crate) fn rtdb_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(number) => number
            .as_i64()
            .map(Value::Int)
            .or_else(|| number.as_f64().map(Value::Float))
            .unwrap_or_else(|| Value::Text(number.to_string())),
        Json::String(text) => Value::Text(text.clone()),
        other => Value::Json(other.to_string()),
    }
}

/// A typed value for a query's filter, from what was typed: JSON (`12`,
/// `true`, `"text"`, `null`), else the word as a string.
pub(crate) fn typed_literal(text: &str) -> Json {
    let parsed: Json = serde_json::from_str(text).unwrap_or_else(|_| Json::String(text.to_owned()));
    to_typed(&parsed)
}

fn to_typed(json: &Json) -> Json {
    match json {
        Json::Null => serde_json::json!({ "nullValue": null }),
        Json::Bool(value) => serde_json::json!({ "booleanValue": value }),
        Json::Number(number) if number.is_i64() || number.is_u64() => {
            serde_json::json!({ "integerValue": number.to_string() })
        }
        Json::Number(number) => serde_json::json!({ "doubleValue": number }),
        Json::String(text) => serde_json::json!({ "stringValue": text }),
        Json::Array(items) => {
            serde_json::json!({ "arrayValue": { "values": items.iter().map(to_typed).collect::<Vec<_>>() } })
        }
        Json::Object(fields) => {
            let fields: Map<String, Json> = fields
                .iter()
                .map(|(name, value)| (name.clone(), to_typed(value)))
                .collect();
            serde_json::json!({ "mapValue": { "fields": fields } })
        }
    }
}
