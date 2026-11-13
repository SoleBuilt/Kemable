//! The Firebase console: one command per line (`kemable_core::command`).
//!
//! | Command | Reads |
//! |---|---|
//! | `get users` | a collection's documents, page after page |
//! | `get users/u1` | one document |
//! | `get users/u1/orders where status == "new" and total > 10 order by total desc limit 50` | a query |
//! | `rtdb /chats/room1` (or `rtdb chats`) | the Realtime Database at a path: its children, or its value |
//!
//! A value after an operator is JSON (`12`, `true`, `"text"`, `null`, or
//! `'["a","b"]'` in single quotes when it has double quotes inside), or a word
//! read as a string. Operators: `==` `!=` `<` `<=` `>` `>=` `in`
//! `not-in` `array-contains` `array-contains-any`.

use kemable_core::{DriverError, Result};
use serde_json::{Value as Json, json};

use crate::values::typed_literal;

#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    /// A collection or a document of Firestore, with a query when one is given.
    Get { path: String, query: Option<Json> },
    /// A path of the Realtime Database, and at most how many children.
    Rtdb { path: String, limit: Option<u64> },
}

fn operator(word: &str) -> Option<&'static str> {
    Some(match word {
        "==" | "=" => "EQUAL",
        "!=" | "<>" => "NOT_EQUAL",
        "<" => "LESS_THAN",
        "<=" => "LESS_THAN_OR_EQUAL",
        ">" => "GREATER_THAN",
        ">=" => "GREATER_THAN_OR_EQUAL",
        "in" => "IN",
        "not-in" => "NOT_IN",
        "array-contains" => "ARRAY_CONTAINS",
        "array-contains-any" => "ARRAY_CONTAINS_ANY",
        _ => return None,
    })
}

fn usage() -> DriverError {
    DriverError::config(
        "get <collection or document> [where <field> <op> <value> [and …]] [order by <field> [desc]] [limit <n>], or rtdb <path> [limit <n>]",
    )
}

/// Reads one line's words (`parse_args`) as a command.
pub(crate) fn parse(words: &[Vec<u8>]) -> Result<Command> {
    let words: Vec<String> = words
        .iter()
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect();
    let lower: Vec<String> = words.iter().map(|word| word.to_ascii_lowercase()).collect();
    let (Some(command), Some(path)) = (lower.first(), words.get(1)) else {
        return Err(usage());
    };
    let path = path.trim_matches('/').to_owned();
    match command.as_str() {
        "rtdb" => {
            let limit = match lower.get(2).map(String::as_str) {
                Some("limit") => Some(number(words.get(3))?),
                Some(_) => return Err(usage()),
                None => None,
            };
            Ok(Command::Rtdb { path, limit })
        }
        "get" => {
            if path.is_empty() {
                return Err(usage());
            }
            let mut filters = Vec::new();
            let mut order = Vec::new();
            let mut limit = None;
            let mut at = 2;
            while let Some(word) = lower.get(at) {
                match word.as_str() {
                    "where" | "and" => {
                        let (Some(field), Some(op), Some(value)) =
                            (words.get(at + 1), lower.get(at + 2), words.get(at + 3))
                        else {
                            return Err(usage());
                        };
                        let op = operator(op)
                            .ok_or_else(|| DriverError::config(format!("{op} is not an operator Firestore knows")))?;
                        filters.push(json!({ "fieldFilter": { "field": { "fieldPath": field }, "op": op, "value": typed_literal(value) } }));
                        at += 4;
                    }
                    "order" if lower.get(at + 1).map(String::as_str) == Some("by") => {
                        let field = words.get(at + 2).ok_or_else(usage)?;
                        let descending = lower.get(at + 3).map(String::as_str) == Some("desc");
                        let skip = usize::from(matches!(lower.get(at + 3).map(String::as_str), Some("desc" | "asc")));
                        order.push(json!({ "field": { "fieldPath": field }, "direction": if descending { "DESCENDING" } else { "ASCENDING" } }));
                        at += 3 + skip;
                    }
                    "limit" => {
                        limit = Some(number(words.get(at + 1))?);
                        at += 2;
                    }
                    _ => return Err(usage()),
                }
            }
            let segments = path.split('/').count();
            if segments.is_multiple_of(2) && (!filters.is_empty() || !order.is_empty() || limit.is_some()) {
                return Err(DriverError::config("a query reads a collection, not a document"));
            }
            let query = (!filters.is_empty() || !order.is_empty() || limit.is_some()).then(|| {
                let collection = path.rsplit('/').next().unwrap_or_default();
                let mut query = json!({ "from": [{ "collectionId": collection }] });
                match filters.len() {
                    0 => {}
                    1 => query["where"] = filters.remove(0),
                    _ => query["where"] = json!({ "compositeFilter": { "op": "AND", "filters": filters } }),
                }
                if !order.is_empty() {
                    query["orderBy"] = Json::Array(order);
                }
                if let Some(limit) = limit {
                    query["limit"] = json!(limit);
                }
                query
            });
            Ok(Command::Get { path, query })
        }
        _ => Err(usage()),
    }
}

fn number(word: Option<&String>) -> Result<u64> {
    word.and_then(|word| word.parse().ok())
        .ok_or_else(|| DriverError::config("limit takes a number"))
}
