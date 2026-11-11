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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Index,
    Trigger,
    /// A function that returns a value: SQL Server's scalar-valued function, PostgreSQL's function.
    Function,
    /// A function that returns rows, read in a `FROM`: SQL Server's table-valued function.
    TableFunction,
    Procedure,
    Sequence,
    Collection,
    Key,
    Queue,
    Exchange,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The server fills it: an identity, an autoincrement or serial column,
    /// or a computed one. Generate Rows and Import leave it out.
    pub generated: bool,
}

/// One index of a table, for Show Structure's Indexes tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub name: String,
    /// Its key columns in order; an expression's text where a column would be.
    pub columns: Vec<String>,
    /// Columns carried along but not in the key (SQL Server's INCLUDE, PostgreSQL's INCLUDE).
    pub included: Vec<String>,
    pub unique: bool,
    /// It holds the primary key.
    pub primary: bool,
    /// How the engine names its kind: `btree`, `CLUSTERED`, `NONCLUSTERED COLUMNSTORE`, `gin`.
    pub method: String,
    /// A partial or filtered index's condition.
    pub filter: Option<String>,
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
