//! A message's properties as one JSON object, for the Live tab's
//! `properties` column: only the ones the publisher set.

use kemable_core::value::Value;
use lapin::BasicProperties;
use lapin::types::{AMQPValue, FieldTable};

/// `text` as a JSON string, its quotes and control characters escaped.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".to_owned()
    }
}

/// A header's value as JSON: bytes that are not UTF-8 as an array of numbers.
pub(crate) fn json(value: &AMQPValue) -> String {
    match value {
        AMQPValue::Boolean(value) => value.to_string(),
        AMQPValue::ShortShortInt(value) => value.to_string(),
        AMQPValue::ShortShortUInt(value) => value.to_string(),
        AMQPValue::ShortInt(value) => value.to_string(),
        AMQPValue::ShortUInt(value) => value.to_string(),
        AMQPValue::LongInt(value) => value.to_string(),
        AMQPValue::LongUInt(value) => value.to_string(),
        AMQPValue::LongLongInt(value) => value.to_string(),
        AMQPValue::Timestamp(value) => value.to_string(),
        AMQPValue::Float(value) => number(f64::from(*value)),
        AMQPValue::Double(value) => number(*value),
        // Exact: the digits with the point placed by the scale, as text.
        AMQPValue::DecimalValue(decimal) => {
            let digits = decimal.value.to_string();
            let scale = usize::from(decimal.scale);
            let text = if scale == 0 {
                digits
            } else {
                let padded = format!("{digits:0>width$}", width = scale + 1);
                let (whole, fraction) = padded.split_at(padded.len() - scale);
                format!("{whole}.{fraction}")
            };
            string(&text)
        }
        AMQPValue::ShortString(text) => string(text.as_str()),
        AMQPValue::LongString(text) => bytes(text.as_bytes()),
        AMQPValue::ByteArray(array) => bytes(array.as_slice()),
        AMQPValue::FieldArray(array) => {
            let items: Vec<String> = array.as_slice().iter().map(json).collect();
            format!("[{}]", items.join(","))
        }
        AMQPValue::FieldTable(table) => object(table),
        AMQPValue::Void => "null".to_owned(),
    }
}

fn bytes(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(text) => string(text),
        Err(_) => {
            let items: Vec<String> = data.iter().map(u8::to_string).collect();
            format!("[{}]", items.join(","))
        }
    }
}

fn object(table: &FieldTable) -> String {
    let fields: Vec<String> = table
        .inner()
        .iter()
        .map(|(key, value)| format!("{}:{}", string(key.as_str()), json(value)))
        .collect();
    format!("{{{}}}", fields.join(","))
}

/// The properties set on a message, as a JSON object; NULL when none is.
pub(crate) fn properties(properties: &BasicProperties) -> Value {
    let mut fields: Vec<String> = Vec::new();
    let mut text = |name: &str, value: Option<&str>| {
        if let Some(value) = value {
            fields.push(format!("{}:{}", string(name), string(value)));
        }
    };
    text(
        "content_type",
        properties.content_type().as_ref().map(|value| value.as_str()),
    );
    text(
        "content_encoding",
        properties.content_encoding().as_ref().map(|value| value.as_str()),
    );
    text(
        "correlation_id",
        properties.correlation_id().as_ref().map(|value| value.as_str()),
    );
    text("reply_to", properties.reply_to().as_ref().map(|value| value.as_str()));
    text(
        "expiration",
        properties.expiration().as_ref().map(|value| value.as_str()),
    );
    text(
        "message_id",
        properties.message_id().as_ref().map(|value| value.as_str()),
    );
    text("type", properties.kind().as_ref().map(|value| value.as_str()));
    text("user_id", properties.user_id().as_ref().map(|value| value.as_str()));
    text("app_id", properties.app_id().as_ref().map(|value| value.as_str()));
    if let Some(mode) = properties.delivery_mode() {
        fields.push(format!("\"delivery_mode\":{mode}"));
    }
    if let Some(priority) = properties.priority() {
        fields.push(format!("\"priority\":{priority}"));
    }
    if let Some(timestamp) = properties.timestamp() {
        fields.push(format!("\"timestamp\":{timestamp}"));
    }
    if let Some(headers) = properties.headers() {
        fields.push(format!("\"headers\":{}", object(headers)));
    }
    if fields.is_empty() {
        Value::Null
    } else {
        Value::Json(format!("{{{}}}", fields.join(",")))
    }
}
