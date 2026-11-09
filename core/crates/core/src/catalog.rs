use serde::{Deserialize, Serialize};

use crate::value::ValueKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub name: String,
    /// The database this connection's session is on.
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
    pub name: String,
    /// The schema a name without one resolves to.
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Index,
    Trigger,
    Function,
    Procedure,
    Sequence,
    Collection,
    Key,
    Queue,
    Exchange,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub schema: Option<String>,
    pub name: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRef {
    pub schema: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub ordinal: u32,
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// Position in the primary key, from 1, when the column is part of it.
    pub primary_key: Option<u32>,
}

/// A result column as a statement returned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub name: String,
    /// The engine's type name, such as `int4` or `nvarchar(50)`; empty when
    /// the engine does not say.
    pub type_name: String,
    /// The kind most values of the column have. A cell carries its own kind,
    /// since SQLite and others mix kinds in one column.
    pub kind: ValueKind,
}
