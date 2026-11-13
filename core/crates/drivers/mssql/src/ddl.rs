//! The T-SQL that makes an object again, written from what the catalog holds.
//! The queries are in `connection.rs`; this file only writes text, so its
//! tests need no server.
//!
//! Statements end with `;` and are separated by a `GO` line, as SSMS scripts
//! them, so the text runs as it is in a SQL Server editor.

use std::fmt::Write;

use kemable_core::sql::{SqlGrammar, quote_name, typed_literal};

/// What goes between two batches of a script.
pub(crate) const BATCH_BREAK: &str = "\nGO\n";

/// A name in brackets, its `]` doubled.
pub(crate) fn bracket(name: &str) -> String {
    quote_name(SqlGrammar::SqlServer, name)
}

/// `[schema].[name]`, as `OBJECT_ID` and a script read it.
pub(crate) fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", bracket(schema), bracket(name))
}

/// Text as an `N'...'` literal, its quotes doubled.
pub(crate) fn text_literal(text: &str) -> String {
    typed_literal(SqlGrammar::SqlServer, text)
}

/// A column's or a sequence's type as `sys.types` and `sys.columns` hold it.
#[derive(Debug, Clone, Default)]
pub(crate) struct SqlType {
    pub name: String,
    /// The schema of a user-defined type, which is written with it; `None` for a system type.
    pub user_schema: Option<String>,
    /// In bytes, `-1` for `max`.
    pub max_length: i32,
    pub precision: i32,
    pub scale: i32,
}

/// The type as a declaration writes it, sized the way the column list shows it.
/// A user-defined type carries its own size, so it is written by name alone.
pub(crate) fn type_text(ty: &SqlType) -> String {
    if let Some(schema) = &ty.user_schema {
        return qualified(schema, &ty.name);
    }
    let size = |length: i32| {
        if length == -1 {
            "max".to_owned()
        } else {
            length.to_string()
        }
    };
    let name = ty.name.as_str();
    match name {
        "varchar" | "char" | "varbinary" | "binary" => format!("{name}({})", size(ty.max_length)),
        // Two bytes a character.
        "nvarchar" | "nchar" => format!(
            "{name}({})",
            if ty.max_length == -1 {
                size(-1)
            } else {
                size(ty.max_length / 2)
            }
        ),
        "decimal" | "numeric" => format!("{name}({},{})", ty.precision, ty.scale),
        "datetime2" | "time" | "datetimeoffset" => format!("{name}({})", ty.scale),
        _ => name.to_owned(),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Identity {
    pub seed: String,
    pub increment: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Computed {
    pub definition: String,
    pub persisted: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DefaultConstraint {
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TableColumn {
    pub name: String,
    pub ty: SqlType,
    pub nullable: bool,
    pub collation: Option<String>,
    pub identity: Option<Identity>,
    pub computed: Option<Computed>,
    pub default: Option<DefaultConstraint>,
}

/// One column of `CREATE TABLE`, without its comma. `COLLATE` is written only
/// when it differs from the database's, so the script follows the database it
/// runs in for the rest.
pub(crate) fn column_line(column: &TableColumn, database_collation: &str) -> String {
    let mut line = bracket(&column.name);
    if let Some(computed) = &column.computed {
        line.push_str(" AS ");
        line.push_str(&computed.definition);
        if computed.persisted {
            line.push_str(" PERSISTED");
            // The server takes NOT NULL on a computed column only when it is persisted.
            if !column.nullable {
                line.push_str(" NOT NULL");
            }
        }
        return line;
    }
    line.push(' ');
    line.push_str(&type_text(&column.ty));
    if let Some(collation) = column
        .collation
        .as_deref()
        .filter(|collation| !collation.eq_ignore_ascii_case(database_collation))
    {
        line.push_str(" COLLATE ");
        line.push_str(collation);
    }
    if let Some(identity) = &column.identity {
        let _ = write!(line, " IDENTITY({}, {})", identity.seed, identity.increment);
    }
    line.push_str(if column.nullable { " NULL" } else { " NOT NULL" });
    if let Some(default) = &column.default {
        let _ = write!(
            line,
            " CONSTRAINT {} DEFAULT {}",
            bracket(&default.name),
            default.definition
        );
    }
    line
}

#[derive(Debug, Clone)]
pub(crate) struct IndexColumn {
    pub name: String,
    pub descending: bool,
}

fn index_columns(columns: &[IndexColumn]) -> String {
    columns
        .iter()
        .map(|column| {
            format!(
                "{} {}",
                bracket(&column.name),
                if column.descending { "DESC" } else { "ASC" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn name_list(names: &[String]) -> String {
    names.iter().map(|name| bracket(name)).collect::<Vec<_>>().join(", ")
}

/// A primary key or a unique constraint.
#[derive(Debug, Clone)]
pub(crate) struct KeyConstraint {
    pub name: String,
    pub primary: bool,
    pub clustered: bool,
    pub columns: Vec<IndexColumn>,
}

pub(crate) fn key_constraint_line(key: &KeyConstraint) -> String {
    format!(
        "CONSTRAINT {} {} {} ({})",
        bracket(&key.name),
        if key.primary { "PRIMARY KEY" } else { "UNIQUE" },
        if key.clustered { "CLUSTERED" } else { "NONCLUSTERED" },
        index_columns(&key.columns)
    )
}

#[derive(Debug, Clone)]
pub(crate) struct CheckConstraint {
    pub name: String,
    /// As the server keeps it, in its own parentheses.
    pub definition: String,
}

pub(crate) fn check_line(check: &CheckConstraint) -> String {
    format!("CONSTRAINT {} CHECK {}", bracket(&check.name), check.definition)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    /// `sys.foreign_keys`' `*_referential_action_desc`: `NO_ACTION`, `CASCADE`, `SET_NULL`, `SET_DEFAULT`.
    pub on_delete: String,
    pub on_update: String,
}

/// The clause for a referential action; `None` for `NO_ACTION`, the default, which SSMS leaves out too.
pub(crate) fn referential_action(desc: &str) -> Option<&'static str> {
    match desc {
        "CASCADE" => Some("CASCADE"),
        "SET_NULL" => Some("SET NULL"),
        "SET_DEFAULT" => Some("SET DEFAULT"),
        _ => None,
    }
}

/// A foreign key, added after the table so the script does not depend on the
/// order tables are made in.
pub(crate) fn foreign_key_statement(table: &str, key: &ForeignKey) -> String {
    let mut statement = format!(
        "ALTER TABLE {table} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        bracket(&key.name),
        name_list(&key.columns),
        qualified(&key.referenced_schema, &key.referenced_table),
        name_list(&key.referenced_columns)
    );
    if let Some(action) = referential_action(&key.on_delete) {
        let _ = write!(statement, " ON DELETE {action}");
    }
    if let Some(action) = referential_action(&key.on_update) {
        let _ = write!(statement, " ON UPDATE {action}");
    }
    statement.push(';');
    statement
}

/// An index that backs no constraint.
#[derive(Debug, Clone, Default)]
pub(crate) struct Index {
    pub name: String,
    pub unique: bool,
    pub clustered: bool,
    pub columns: Vec<IndexColumn>,
    pub included: Vec<String>,
    pub filter: Option<String>,
}

pub(crate) fn index_statement(table: &str, index: &Index) -> String {
    let mut statement = format!(
        "CREATE {}{} INDEX {} ON {table} ({})",
        if index.unique { "UNIQUE " } else { "" },
        if index.clustered { "CLUSTERED" } else { "NONCLUSTERED" },
        bracket(&index.name),
        index_columns(&index.columns)
    );
    if !index.included.is_empty() {
        let _ = write!(statement, " INCLUDE ({})", name_list(&index.included));
    }
    if let Some(filter) = &index.filter {
        let _ = write!(statement, " WHERE {filter}");
    }
    statement.push(';');
    statement
}

/// A columnstore index. A clustered one stores the whole table, so it names no columns.
#[derive(Debug, Clone, Default)]
pub(crate) struct ColumnstoreIndex {
    pub name: String,
    pub clustered: bool,
    pub columns: Vec<String>,
    /// `ORDER`'s columns (SQL Server 2022 and later); empty when it has none.
    pub order: Vec<String>,
    pub filter: Option<String>,
}

pub(crate) fn columnstore_statement(table: &str, index: &ColumnstoreIndex) -> String {
    let mut statement = format!(
        "CREATE {} COLUMNSTORE INDEX {} ON {table}",
        if index.clustered { "CLUSTERED" } else { "NONCLUSTERED" },
        bracket(&index.name)
    );
    if !index.clustered {
        let _ = write!(statement, " ({})", name_list(&index.columns));
    }
    if !index.order.is_empty() {
        let _ = write!(statement, " ORDER ({})", name_list(&index.order));
    }
    if let Some(filter) = &index.filter {
        let _ = write!(statement, " WHERE {filter}");
    }
    statement.push(';');
    statement
}

/// What a secondary XML index is for, from `sys.xml_indexes.secondary_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum XmlPurpose {
    Value,
    Path,
    Property,
}

impl XmlPurpose {
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        match code.trim() {
            "V" => Some(Self::Value),
            "P" => Some(Self::Path),
            "R" => Some(Self::Property),
            _ => None,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Value => "VALUE",
            Self::Path => "PATH",
            Self::Property => "PROPERTY",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SecondaryXml {
    /// The primary XML index it is built on.
    pub primary: String,
    pub purpose: XmlPurpose,
}

#[derive(Debug, Clone)]
pub(crate) struct XmlIndex {
    pub name: String,
    pub column: String,
    /// `None` for a primary XML index.
    pub secondary: Option<SecondaryXml>,
}

pub(crate) fn xml_index_statement(table: &str, index: &XmlIndex) -> String {
    match &index.secondary {
        None => format!(
            "CREATE PRIMARY XML INDEX {} ON {table} ({});",
            bracket(&index.name),
            bracket(&index.column)
        ),
        Some(secondary) => format!(
            "CREATE XML INDEX {} ON {table} ({}) USING XML INDEX {} FOR {};",
            bracket(&index.name),
            bracket(&index.column),
            bracket(&secondary.primary),
            secondary.purpose.keyword()
        ),
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SpatialIndex {
    pub name: String,
    pub column: String,
    /// `sys.spatial_index_tessellations.tessellation_scheme`: `GEOMETRY_GRID`,
    /// `GEOMETRY_AUTO_GRID`, `GEOGRAPHY_GRID` or `GEOGRAPHY_AUTO_GRID`.
    pub scheme: String,
    /// `xmin, ymin, xmax, ymax`; a geography index has none.
    pub bounding_box: Option<[f64; 4]>,
    /// The density of each level from the first: `LOW`, `MEDIUM` or `HIGH`.
    pub grids: Vec<String>,
    pub cells_per_object: Option<i32>,
}

pub(crate) fn spatial_index_statement(table: &str, index: &SpatialIndex) -> String {
    let mut options = Vec::new();
    // The server takes a bounding box only for geometry, and refuses GRIDS with an auto grid.
    if index.scheme.starts_with("GEOMETRY")
        && let Some([xmin, ymin, xmax, ymax]) = index.bounding_box
    {
        options.push(format!("BOUNDING_BOX = ({xmin}, {ymin}, {xmax}, {ymax})"));
    }
    if !index.scheme.ends_with("AUTO_GRID") && !index.grids.is_empty() {
        let levels: Vec<String> = index
            .grids
            .iter()
            .enumerate()
            .map(|(level, density)| format!("LEVEL_{} = {density}", level + 1))
            .collect();
        options.push(format!("GRIDS = ({})", levels.join(", ")));
    }
    if let Some(cells) = index.cells_per_object {
        options.push(format!("CELLS_PER_OBJECT = {cells}"));
    }
    let mut statement = format!(
        "CREATE SPATIAL INDEX {} ON {table} ({}) USING {}",
        bracket(&index.name),
        bracket(&index.column),
        index.scheme
    );
    if !options.is_empty() {
        let _ = write!(statement, " WITH ({})", options.join(", "));
    }
    statement.push(';');
    statement
}

/// An `MS_Description` on a table, or on one of its columns.
#[derive(Debug, Clone)]
pub(crate) struct Description {
    pub column: Option<String>,
    pub text: String,
}

pub(crate) fn description_statement(schema: &str, table: &str, description: &Description) -> String {
    let mut statement = format!(
        "EXEC sys.sp_addextendedproperty @name = N'MS_Description', @value = {}, \
         @level0type = N'SCHEMA', @level0name = {}, @level1type = N'TABLE', @level1name = {}",
        text_literal(&description.text),
        text_literal(schema),
        text_literal(table)
    );
    if let Some(column) = &description.column {
        let _ = write!(
            statement,
            ", @level2type = N'COLUMN', @level2name = {}",
            text_literal(column)
        );
    }
    statement.push(';');
    statement
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Table {
    pub schema: String,
    pub name: String,
    pub database_collation: String,
    pub columns: Vec<TableColumn>,
    pub keys: Vec<KeyConstraint>,
    pub checks: Vec<CheckConstraint>,
    pub foreign_keys: Vec<ForeignKey>,
    pub indexes: Vec<Index>,
    pub columnstores: Vec<ColumnstoreIndex>,
    pub xml_indexes: Vec<XmlIndex>,
    pub spatial_indexes: Vec<SpatialIndex>,
    pub descriptions: Vec<Description>,
}

pub(crate) fn table_ddl(table: &Table) -> String {
    let name = qualified(&table.schema, &table.name);
    let lines: Vec<String> = table
        .columns
        .iter()
        .map(|column| column_line(column, &table.database_collation))
        .chain(table.keys.iter().map(key_constraint_line))
        .chain(table.checks.iter().map(check_line))
        .collect();
    let mut create = format!("CREATE TABLE {name} (\n");
    for (index, line) in lines.iter().enumerate() {
        create.push_str("    ");
        create.push_str(line);
        if index + 1 < lines.len() {
            create.push(',');
        }
        create.push('\n');
    }
    create.push_str(");");

    let statements: Vec<String> = std::iter::once(create)
        .chain(table.foreign_keys.iter().map(|key| foreign_key_statement(&name, key)))
        .chain(table.indexes.iter().map(|index| index_statement(&name, index)))
        .chain(
            table
                .columnstores
                .iter()
                .map(|index| columnstore_statement(&name, index)),
        )
        // A secondary XML index is built on its primary, so every primary comes first.
        .chain(
            table
                .xml_indexes
                .iter()
                .filter(|index| index.secondary.is_none())
                .chain(table.xml_indexes.iter().filter(|index| index.secondary.is_some()))
                .map(|index| xml_index_statement(&name, index)),
        )
        .chain(
            table
                .spatial_indexes
                .iter()
                .map(|index| spatial_index_statement(&name, index)),
        )
        .chain(
            table
                .descriptions
                .iter()
                .map(|description| description_statement(&table.schema, &table.name, description)),
        )
        .collect();
    statements.join(BATCH_BREAK)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Sequence {
    pub schema: String,
    pub name: String,
    pub ty: SqlType,
    pub start: String,
    pub increment: String,
    pub minimum: String,
    pub maximum: String,
    pub cycling: bool,
    pub cached: bool,
    /// `None` when the server picks the cache's size.
    pub cache_size: Option<i32>,
}

pub(crate) fn sequence_ddl(sequence: &Sequence) -> String {
    let cache = match (sequence.cached, sequence.cache_size) {
        (false, _) => "NO CACHE".to_owned(),
        (true, Some(size)) => format!("CACHE {size}"),
        (true, None) => "CACHE".to_owned(),
    };
    format!(
        "CREATE SEQUENCE {} AS {} START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} {} {cache};",
        qualified(&sequence.schema, &sequence.name),
        type_text(&sequence.ty),
        sequence.start,
        sequence.increment,
        sequence.minimum,
        sequence.maximum,
        if sequence.cycling { "CYCLE" } else { "NO CYCLE" }
    )
}
