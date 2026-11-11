//! Show DDL for PostgreSQL (`docs/decisions/0017-tree-actions.md`): the
//! catalog queries that read an object, and the pure functions that put the
//! server's own pieces together into statements.
//!
//! The server writes every piece it can (`format_type`, `pg_get_expr`,
//! `pg_get_constraintdef`, `pg_get_indexdef`, `pg_get_viewdef`,
//! `pg_get_functiondef`), so the text follows its version's own rules; the
//! code here only adds the keywords around them and quotes names and comments.

use paboable_core::catalog::ObjectKind;
use paboable_core::sql::{SqlGrammar, quote_name};

/// One relation by schema, name and the `relkind`s its `ObjectKind` covers.
/// An empty schema means the session's current one.
pub(crate) const RELATION: &str = "\
select c.oid::text, c.relkind::text, n.nspname::text, c.relname::text, c.relpersistence::text,
       c.reloptions::text[],
       case when c.relkind = 'p' then pg_get_partkeydef(c.oid) end,
       pn.nspname::text, pc.relname::text,
       case when c.relispartition then pg_get_expr(c.relpartbound, c.oid) end,
       fs.srvname::text, ft.ftoptions::text[],
       c.relispopulated,
       obj_description(c.oid, 'pg_class'),
       case when c.relkind in ('v', 'm') then pg_get_viewdef(c.oid, true) end,
       otn.nspname::text, ot.typname::text
from pg_class c
join pg_namespace n on n.oid = c.relnamespace
left join pg_type ot on ot.oid = c.reloftype
left join pg_namespace otn on otn.oid = ot.typnamespace
left join pg_inherits i on c.relispartition and i.inhrelid = c.oid
left join pg_class pc on pc.oid = i.inhparent
left join pg_namespace pn on pn.oid = pc.relnamespace
left join pg_foreign_table ft on ft.ftrelid = c.oid
left join pg_foreign_server fs on fs.oid = ft.ftserver
where n.nspname = coalesce(nullif($1, ''), current_schema()) and c.relname = $2
  and strpos($3, c.relkind::text) > 0";

/// A relation's columns. The collation is written only when it is not the
/// type's own, and schema-qualified when it lives outside `pg_catalog`. A
/// column the table only inherits comes with the default and not-null its
/// parents give it, so the script can say where the child differs.
pub(crate) const COLUMNS: &str = "\
select a.attname::text, format_type(a.atttypid, a.atttypmod),
       case when a.attcollation <> 0 and a.attcollation <> t.typcollation then
         case when cn.nspname = 'pg_catalog' then quote_ident(co.collname::text)
              else quote_ident(cn.nspname::text) || '.' || quote_ident(co.collname::text) end
       end,
       pg_get_expr(d.adbin, d.adrelid),
       a.attidentity::text, a.attgenerated::text, a.attnotnull,
       col_description(a.attrelid, a.attnum),
       not a.attislocal and a.attinhcount > 0,
       (select pg_get_expr(pd.adbin, pd.adrelid)
          from pg_inherits i
          join pg_attribute pa on pa.attrelid = i.inhparent and pa.attname = a.attname
          join pg_attrdef pd on pd.adrelid = pa.attrelid and pd.adnum = pa.attnum
         where i.inhrelid = a.attrelid
         order by i.inhseqno
         limit 1),
       exists (select 1
                 from pg_inherits i
                 join pg_attribute pa on pa.attrelid = i.inhparent and pa.attname = a.attname
                where i.inhrelid = a.attrelid and pa.attnotnull)
from pg_attribute a
join pg_type t on t.oid = a.atttypid
left join pg_collation co on co.oid = a.attcollation
left join pg_namespace cn on cn.oid = co.collnamespace
left join pg_attrdef d on d.adrelid = a.attrelid and d.adnum = a.attnum
where a.attrelid = $1::oid and a.attnum > 0 and not a.attisdropped
order by a.attnum";

/// The table's constraints, primary key first. Not-null constraints (`n`,
/// PostgreSQL 18) are written on their columns instead. A partition or an
/// inheritance child leaves out what it only inherits: attaching it to its
/// parent, or `INHERITS`, makes those again.
pub(crate) const CONSTRAINTS: &str = "\
select con.conname::text, pg_get_constraintdef(con.oid, true)
from pg_constraint con
where con.conrelid = $1::oid and con.contype in ('p', 'u', 'x', 'c', 'f')
  and con.conislocal
order by case con.contype when 'p' then 0 when 'u' then 1 when 'x' then 2 when 'c' then 3 else 4 end,
         con.conname";

/// The indexes no constraint owns; a key's own index comes with its constraint.
pub(crate) const INDEXES: &str = "\
select pg_get_indexdef(i.indexrelid)
from pg_index i
join pg_class ic on ic.oid = i.indexrelid
where i.indrelid = $1::oid
  and not exists (select 1 from pg_constraint con
                   where con.conindid = i.indexrelid and con.conrelid = i.indrelid
                     and con.contype in ('p', 'u', 'x'))
order by ic.relname";

/// The parents of a table that inherits, in `INHERITS` order. A partition's
/// parent is not one here: `ATTACH PARTITION` brings it back.
pub(crate) const PARENTS: &str = "\
select pn.nspname::text, p.relname::text
from pg_inherits i
join pg_class c on c.oid = i.inhrelid
join pg_class p on p.oid = i.inhparent
join pg_namespace pn on pn.oid = p.relnamespace
where i.inhrelid = $1::oid and not c.relispartition
order by i.inhseqno";

/// The sequences the table's columns own (`serial`, or `OWNED BY`), with the
/// column that owns each. An identity column's sequence is internal (`i`)
/// and comes back with its column.
pub(crate) const OWNED_SEQUENCES: &str = "\
select sn.nspname::text, s.relname::text, s.relpersistence::text, a.attname::text,
       format_type(q.seqtypid, null), q.seqincrement::text, q.seqmin::text, q.seqmax::text,
       q.seqstart::text, q.seqcache::text, q.seqcycle
from pg_depend dep
join pg_class s on s.oid = dep.objid and s.relkind = 'S'
join pg_namespace sn on sn.oid = s.relnamespace
join pg_sequence q on q.seqrelid = s.oid
join pg_attribute a on a.attrelid = dep.refobjid and a.attnum = dep.refobjsubid
where dep.classid = 'pg_class'::regclass and dep.refclassid = 'pg_class'::regclass
  and dep.refobjid = $1::oid and dep.refobjsubid > 0 and dep.deptype = 'a'
order by a.attnum, s.relname";

pub(crate) const SEQUENCE: &str = "\
select format_type(s.seqtypid, null), s.seqincrement::text, s.seqmin::text, s.seqmax::text,
       s.seqstart::text, s.seqcache::text, s.seqcycle
from pg_sequence s
where s.seqrelid = $1::oid";

/// Every overload of a routine. `$2` is its bare name, or the name with its
/// arguments as `list_objects` shows it, which picks one overload.
pub(crate) const ROUTINES: &str = "\
select pg_get_functiondef(p.oid)
from pg_proc p join pg_namespace n on n.oid = p.pronamespace
where n.nspname = coalesce(nullif($1, ''), current_schema())
  and strpos($3, p.prokind::text) > 0
  and (p.proname = $2 or p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')' = $2)
order by pg_get_function_identity_arguments(p.oid)";

pub(crate) const RELATION_COMMENT: &str = "\
select obj_description(c.oid, 'pg_class')
from pg_class c join pg_namespace n on n.oid = c.relnamespace
where n.nspname = coalesce(nullif($1, ''), current_schema()) and c.relname = $2
  and strpos($3, c.relkind::text) > 0";

pub(crate) const ROUTINE_COMMENT: &str = "\
select obj_description(p.oid, 'pg_proc')
from pg_proc p join pg_namespace n on n.oid = p.pronamespace
where n.nspname = coalesce(nullif($1, ''), current_schema())
  and strpos($3, p.prokind::text) > 0
  and (p.proname = $2 or p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')' = $2)
order by pg_get_function_identity_arguments(p.oid)";

/// The `pg_class.relkind`s an object kind covers, as `list_objects` maps them.
pub(crate) fn relkinds(kind: ObjectKind) -> Option<&'static str> {
    match kind {
        ObjectKind::Table => Some("rpf"),
        ObjectKind::View => Some("v"),
        ObjectKind::MaterializedView => Some("m"),
        ObjectKind::Sequence => Some("S"),
        _ => None,
    }
}

/// The `pg_proc.prokind`s an object kind covers.
pub(crate) fn prokinds(kind: ObjectKind) -> Option<&'static str> {
    match kind {
        ObjectKind::Function => Some("f"),
        ObjectKind::Procedure => Some("p"),
        _ => None,
    }
}

pub(crate) fn qualified(schema: &str, name: &str) -> String {
    format!(
        "{}.{}",
        quote_name(SqlGrammar::Postgres, schema),
        quote_name(SqlGrammar::Postgres, name)
    )
}

/// Text as a string literal, quotes doubled. Text with a backslash goes in
/// `E'...'` with the backslash doubled, so it reads back the same whatever
/// `standard_conforming_strings` says.
pub(crate) fn literal(text: &str) -> String {
    let escape = text.contains('\\');
    let mut out = String::with_capacity(text.len() + 3);
    if escape {
        out.push('E');
    }
    out.push('\'');
    for ch in text.chars() {
        if ch == '\'' || (escape && ch == '\\') {
            out.push(ch);
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// How a column gets its value when an insert leaves it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fill {
    Nothing,
    Default(String),
    Identity { always: bool },
    Generated { expression: String, stored: bool },
}

impl Fill {
    /// From `attidentity` (`a`, `d` or empty), `attgenerated` (`s`, `v` or
    /// empty) and the expression `pg_attrdef` holds, which is the generation
    /// expression for a generated column.
    pub(crate) fn from_catalog(identity: &str, generated: &str, expression: Option<String>) -> Self {
        match (identity, generated, expression) {
            ("a", _, _) => Self::Identity { always: true },
            ("d", _, _) => Self::Identity { always: false },
            (_, "s", Some(expression)) => Self::Generated {
                expression,
                stored: true,
            },
            (_, "v", Some(expression)) => Self::Generated {
                expression,
                stored: false,
            },
            (_, _, Some(expression)) => Self::Default(expression),
            _ => Self::Nothing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Column {
    pub name: String,
    /// `format_type`'s text.
    pub type_name: String,
    /// Already quoted by the server; `None` for the type's own collation.
    pub collation: Option<String>,
    pub fill: Fill,
    pub not_null: bool,
    pub comment: Option<String>,
    /// Set when the table only inherits the column: what its parents give it.
    pub inherited: Option<Inherited>,
}

/// The default and not-null a column gets from its parents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Inherited {
    pub default: Option<String>,
    pub not_null: bool,
}

/// One column as it stands inside `CREATE TABLE`, without its comma.
pub(crate) fn column_line(column: &Column) -> String {
    let mut line = format!(
        "{} {}",
        quote_name(SqlGrammar::Postgres, &column.name),
        column.type_name
    );
    if let Some(collation) = &column.collation {
        line.push_str(" COLLATE ");
        line.push_str(collation);
    }
    push_fill(&mut line, column);
    line
}

/// One column of a typed table (`OF type`), which takes its type and
/// collation from the type; `None` when it adds nothing to them.
pub(crate) fn typed_column_line(column: &Column) -> Option<String> {
    if column.fill == Fill::Nothing && !column.not_null {
        return None;
    }
    let mut line = format!("{} WITH OPTIONS", quote_name(SqlGrammar::Postgres, &column.name));
    push_fill(&mut line, column);
    Some(line)
}

fn push_fill(line: &mut String, column: &Column) {
    match &column.fill {
        Fill::Nothing => {}
        Fill::Default(expression) => {
            line.push_str(" DEFAULT ");
            line.push_str(expression);
        }
        Fill::Identity { always: true } => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
        Fill::Identity { always: false } => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
        Fill::Generated { expression, stored } => {
            line.push_str(" GENERATED ALWAYS AS (");
            line.push_str(expression);
            line.push_str(if *stored { ") STORED" } else { ") VIRTUAL" });
        }
    }
    if column.not_null {
        line.push_str(" NOT NULL");
    }
}

/// What `CREATE TABLE` needs besides its columns, constraints and indexes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Table {
    pub schema: String,
    pub name: String,
    pub unlogged: bool,
    /// `reloptions`, each `name=value` as the server keeps it.
    pub options: Vec<String>,
    pub partition_key: Option<String>,
    /// The parent's schema and name, and `FOR VALUES ...` or `DEFAULT`.
    pub partition_of: Option<(String, String, String)>,
    /// A foreign table's server, with its options as `name=value`.
    pub server: Option<(String, Vec<String>)>,
    pub comment: Option<String>,
    /// The parents' schemas and names in `INHERITS` order; never a
    /// partition's parent.
    pub inherits: Vec<(String, String)>,
    /// The composite type of a typed table (`OF type`): schema and name.
    pub of_type: Option<(String, String)>,
    /// The sequences its columns own, made before the table.
    pub sequences: Vec<OwnedSequence>,
}

/// A sequence a column owns, as `serial` makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedSequence {
    pub schema: String,
    pub name: String,
    pub unlogged: bool,
    pub column: String,
    pub sequence: Sequence,
}

/// A constraint's name and `pg_get_constraintdef`'s text.
pub(crate) type Constraint = (String, String);

pub(crate) fn table_ddl(table: &Table, columns: &[Column], constraints: &[Constraint], indexes: &[String]) -> String {
    let name = qualified(&table.schema, &table.name);
    // A child leaves out the columns it only inherits: INHERITS brings them back.
    let only_inherited = |column: &Column| !table.inherits.is_empty() && column.inherited.is_some();
    let mut lines: Vec<String> = columns
        .iter()
        .filter(|column| !only_inherited(column))
        .filter_map(|column| match table.of_type {
            Some(_) => typed_column_line(column),
            None => Some(column_line(column)),
        })
        .collect();
    lines.extend(constraints.iter().map(|(constraint, definition)| {
        format!(
            "CONSTRAINT {} {definition}",
            quote_name(SqlGrammar::Postgres, constraint)
        )
    }));
    let (what, noun) = match (&table.server, table.unlogged) {
        (Some(_), _) => ("FOREIGN TABLE", "FOREIGN TABLE"),
        (None, true) => ("UNLOGGED TABLE", "TABLE"),
        (None, false) => ("TABLE", "TABLE"),
    };
    let mut create = format!("CREATE {what} {name}");
    if let Some((schema, type_name)) = &table.of_type {
        create.push_str(" OF ");
        create.push_str(&qualified(schema, type_name));
    }
    if !lines.is_empty() {
        create.push_str(" (\n");
        for (index, line) in lines.iter().enumerate() {
            create.push_str("    ");
            create.push_str(line);
            create.push_str(if index + 1 < lines.len() { ",\n" } else { "\n" });
        }
        create.push(')');
    } else if table.of_type.is_none() {
        // A typed table takes no empty list; any other table needs one.
        create.push_str(" ()");
    }
    if !table.inherits.is_empty() {
        let parents: Vec<String> = table
            .inherits
            .iter()
            .map(|(schema, parent)| qualified(schema, parent))
            .collect();
        create.push_str(" INHERITS (");
        create.push_str(&parents.join(", "));
        create.push(')');
    }
    if let Some(key) = &table.partition_key {
        create.push_str(" PARTITION BY ");
        create.push_str(key);
    }
    if let Some((server, options)) = &table.server {
        create.push_str(" SERVER ");
        create.push_str(&quote_name(SqlGrammar::Postgres, server));
        create.push_str(&generic_options(options));
    }
    create.push_str(&with_options(&table.options));
    create.push(';');

    // The column defaults name the sequences, so those come first.
    let mut statements: Vec<String> = table
        .sequences
        .iter()
        .map(|owned| sequence_ddl(&owned.schema, &owned.name, owned.unlogged, &owned.sequence))
        .collect();
    statements.push(create);
    if let Some((schema, parent, bound)) = &table.partition_of {
        statements.push(format!(
            "ALTER TABLE {} ATTACH PARTITION {name} {bound};",
            qualified(schema, parent)
        ));
    }
    for column in columns.iter().filter(|column| only_inherited(column)) {
        statements.extend(inherited_column_changes(noun, &name, column));
    }
    statements.extend(table.sequences.iter().map(|owned| {
        format!(
            "ALTER SEQUENCE {} OWNED BY {name}.{};",
            qualified(&owned.schema, &owned.name),
            quote_name(SqlGrammar::Postgres, &owned.column)
        )
    }));
    statements.extend(indexes.iter().map(|index| statement(index)));
    if let Some(comment) = &table.comment {
        statements.push(format!("COMMENT ON {noun} {name} IS {};", literal(comment)));
    }
    for column in columns {
        if let Some(comment) = &column.comment {
            statements.push(format!(
                "COMMENT ON COLUMN {name}.{} IS {};",
                quote_name(SqlGrammar::Postgres, &column.name),
                literal(comment)
            ));
        }
    }
    statements.join("\n\n")
}

/// Where a column the child only inherits differs from what its parents
/// give it: its own default, no default, or a not-null of its own.
fn inherited_column_changes(noun: &str, table: &str, column: &Column) -> Vec<String> {
    let Some(inherited) = &column.inherited else {
        return Vec::new();
    };
    let alter = format!(
        "ALTER {noun} ONLY {table} ALTER COLUMN {}",
        quote_name(SqlGrammar::Postgres, &column.name)
    );
    let mut changes = Vec::new();
    match &column.fill {
        Fill::Default(expression) if inherited.default.as_ref() != Some(expression) => {
            changes.push(format!("{alter} SET DEFAULT {expression};"));
        }
        Fill::Nothing if inherited.default.is_some() => changes.push(format!("{alter} DROP DEFAULT;")),
        _ => {}
    }
    if column.not_null && !inherited.not_null {
        changes.push(format!("{alter} SET NOT NULL;"));
    }
    changes
}

/// One statement, which `CREATE OR REPLACE` can run again to change the view.
pub(crate) fn view_ddl(schema: &str, name: &str, options: &[String], definition: &str) -> String {
    format!(
        "CREATE OR REPLACE VIEW {}{} AS\n{}",
        qualified(schema, name),
        with_options(options),
        statement(definition)
    )
}

pub(crate) fn materialized_view_ddl(
    schema: &str,
    name: &str,
    options: &[String],
    definition: &str,
    populated: bool,
    indexes: &[String],
    comment: Option<&str>,
) -> String {
    let qualified_name = qualified(schema, name);
    let query = definition.trim().trim_end_matches(';').trim_end();
    let data = if populated { "" } else { "\nWITH NO DATA" };
    let mut statements = vec![format!(
        "CREATE MATERIALIZED VIEW {qualified_name}{} AS\n{query}{data};",
        with_options(options)
    )];
    statements.extend(indexes.iter().map(|index| statement(index)));
    if let Some(comment) = comment {
        statements.push(format!(
            "COMMENT ON MATERIALIZED VIEW {qualified_name} IS {};",
            literal(comment)
        ));
    }
    statements.join("\n\n")
}

/// `pg_sequence`'s numbers, as text so no width is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sequence {
    pub type_name: String,
    pub increment: String,
    pub min: String,
    pub max: String,
    pub start: String,
    pub cache: String,
    pub cycle: bool,
}

pub(crate) fn sequence_ddl(schema: &str, name: &str, unlogged: bool, sequence: &Sequence) -> String {
    format!(
        "CREATE {}SEQUENCE {}\n    AS {}\n    INCREMENT BY {}\n    MINVALUE {}\n    MAXVALUE {}\n    START WITH {}\n    CACHE {}\n    {}CYCLE;",
        if unlogged { "UNLOGGED " } else { "" },
        qualified(schema, name),
        sequence.type_name,
        sequence.increment,
        sequence.min,
        sequence.max,
        sequence.start,
        sequence.cache,
        if sequence.cycle { "" } else { "NO " }
    )
}

/// `pg_get_functiondef` ends a body with a newline and no `;`.
pub(crate) fn routines_ddl(definitions: &[String]) -> String {
    definitions
        .iter()
        .map(|definition| statement(definition))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The text trimmed, ending with one `;`.
pub(crate) fn statement(text: &str) -> String {
    let body = text.trim();
    if body.ends_with(';') {
        body.to_owned()
    } else {
        format!("{body};")
    }
}

fn with_options(options: &[String]) -> String {
    if options.is_empty() {
        String::new()
    } else {
        format!(" WITH ({})", options.join(", "))
    }
}

/// A foreign server's `name=value` options as `OPTIONS (name 'value')`.
pub(crate) fn generic_options(options: &[String]) -> String {
    if options.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = options
        .iter()
        .map(|option| {
            let (key, value) = option.split_once('=').unwrap_or((option.as_str(), ""));
            format!("{} {}", quote_name(SqlGrammar::Postgres, key), literal(value))
        })
        .collect();
    format!(" OPTIONS ({})", pairs.join(", "))
}
