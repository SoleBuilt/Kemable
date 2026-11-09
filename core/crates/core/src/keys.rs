//! The key-value data model (Redis): keys found by a scan, and one key read
//! as a result set (`docs/decisions/0008-key-value-engines.md`).

use serde::{Deserialize, Serialize};

/// What a key holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyKind {
    String,
    List,
    Set,
    SortedSet,
    Hash,
    Stream,
    /// A module's type, such as RedisJSON: read through its own commands.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub name: String,
    pub kind: KeyKind,
    /// Milliseconds before it expires; `None` when it does not.
    pub ttl_ms: Option<i64>,
}

/// One step of a scan over the keys.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyScan {
    /// A glob, `*` for every key.
    pub pattern: String,
    /// Where the last step stopped; `None` to start.
    pub cursor: Option<String>,
    /// How many keys the server looks at in this step: a hint, not a page size.
    pub count: u32,
    /// Only keys of this kind.
    pub kind: Option<KeyKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPage {
    pub keys: Vec<KeyInfo>,
    /// Where to go on; `None` once the scan has seen every key.
    pub cursor: Option<String>,
}

/// What reading a key found besides its rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyDetail {
    pub kind: KeyKind,
    pub ttl_ms: Option<i64>,
    /// Elements in a collection, bytes in a string.
    pub length: u64,
}
