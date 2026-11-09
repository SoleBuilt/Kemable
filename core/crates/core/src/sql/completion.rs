//! What the editor can offer at the cursor: the context (which clause, which
//! tables are in scope under which aliases, the word being typed and what
//! qualifies it), the catalog lookups that context needs, and the items built
//! from what the lookups found (`docs/decisions/0007-autocomplete.md`).
//!
//! Keywords are the editor's own; this module offers names: schemas, tables,
//! views, columns and the aliases a statement gave them. It reads text that
//! is still being written, so nothing here refuses an unclosed parenthesis.

use serde::Serialize;

use super::lex::{Kind, Open, Token, lex};
use super::{SqlGrammar, split};
use crate::catalog::{ColumnInfo, ObjectInfo, ObjectKind, SchemaInfo, TableRef};

/// Where the cursor is, as far as names go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clause {
    /// The first word of a statement: keywords only.
    Start,
    /// A table is expected: after `FROM`, `JOIN`, `UPDATE`, `INTO`, `TABLE`, or a comma in a `FROM` list.
    Table,
    /// After a table in a `FROM`: an alias or a keyword such as `WHERE`.
    AfterTable,
    /// An expression: the select list, `WHERE`, `ON`, `GROUP BY`, `ORDER BY`, `SET`, `HAVING`, function arguments.
    Expression,
    /// The column list of `INSERT INTO t (`.
    InsertColumns(TableRef),
    /// Anywhere names do not help: `LIMIT`, `VALUES`, after `CREATE`.
    Other,
}

/// A table the statement names, with the alias it gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableReference {
    pub table: TableRef,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionContext {
    /// Byte offset where the word being typed starts: what an item replaces, up to the cursor.
    pub from: usize,
    /// The word typed so far, without its opening quote.
    pub prefix: String,
    /// The closing quote, when the word is typed inside an open quoted name.
    pub quote: Option<char>,
    /// The names before the dots, as in `s.t.` (`["s", "t"]`).
    pub qualifier: Vec<String>,
    pub clause: Clause,
    pub tables: Vec<TableReference>,
}

/// What to read from the catalog for a context, after the schemas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lookups {
    /// The schemas whose tables and views are needed.
    pub objects_of: Vec<String>,
    pub columns_of: Vec<TableRef>,
}

/// What the lookups found.
#[derive(Debug, Clone, Default)]
pub struct CatalogView {
    pub schemas: Vec<SchemaInfo>,
    pub objects: Vec<ObjectInfo>,
    pub columns: Vec<(TableRef, Vec<ColumnInfo>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    Schema,
    Table,
    View,
    Column,
    Alias,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    /// The text that replaces the word typed, quoted when the name needs it.
    pub apply: String,
    pub kind: ItemKind,
    /// The column's type and table, the table's schema.
    pub detail: String,
    /// Higher shows first among equal matches; keywords are 0.
    pub boost: i8,
}

/// Words that end a table reference, so they are never read as its alias,
/// and that a name must be quoted to be.
const RESERVED: &[&str] = &[
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BETWEEN",
    "BY",
    "CASE",
    "CHECK",
    "COLUMN",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "FETCH",
    "FOR",
    "FROM",
    "FULL",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NATURAL",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OPTION",
    "OR",
    "ORDER",
    "OUTER",
    "OUTPUT",
    "PRIMARY",
    "REFERENCES",
    "RETURNING",
    "RIGHT",
    "SELECT",
    "SET",
    "TABLE",
    "TABLESAMPLE",
    "THEN",
    "TOP",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USER",
    "USING",
    "VALUES",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
];

fn reserved(word: &str) -> bool {
    RESERVED.iter().any(|reserved| reserved.eq_ignore_ascii_case(word))
}

/// The context at byte `cursor` of `text`, or `None` inside a string or a comment.
pub fn completion_context(text: &str, cursor: usize, grammar: SqlGrammar) -> Option<CompletionContext> {
    let cursor = floor_char_boundary(text, cursor.min(text.len()));
    let (region_start, region_end, fresh) = region(text, cursor, grammar);

    let before = lex(&text[region_start..cursor], grammar);
    if matches!(
        before.open,
        Some(Open::Literal | Open::LineComment | Open::BlockComment)
    ) {
        return None;
    }
    let mut tokens: Vec<Token> = if fresh {
        Vec::new()
    } else {
        before.tokens.iter().map(|token| shift(*token, region_start)).collect()
    };
    // Only the statement the cursor is in: after the last top-level `;` before it.
    if let Some(last) = tokens
        .iter()
        .rposition(|token| token.kind == Kind::Punct(';') && token.depth == 0)
    {
        tokens.drain(..=last);
    }
    let statement_start = tokens.first().map_or(cursor, |token| token.start);

    // The word being typed ends at the cursor; an open quoted name always does.
    let typing = tokens
        .last()
        .copied()
        .filter(|token| token.end == cursor && matches!(token.kind, Kind::Word | Kind::QuotedName));
    let (from, prefix, quote) = match typing {
        Some(token) if token.kind == Kind::QuotedName && before.open == Some(Open::Name) => {
            let open = text[token.start..].chars().next()?;
            let close = if open == '[' { ']' } else { open };
            (
                token.start + open.len_utf8(),
                token.identifier(text).unwrap_or_default(),
                Some(close),
            )
        }
        // A closed quoted name before the cursor is finished: nothing is being typed.
        Some(token) if token.kind == Kind::QuotedName => (cursor, String::new(), None),
        Some(token) => (token.start, text[token.start..token.end].to_owned(), None),
        None => (cursor, String::new(), None),
    };
    if typing.is_some() && from != cursor || quote.is_some() {
        tokens.pop();
    }

    let mut qualifier = Vec::new();
    while tokens.last().is_some_and(|token| token.kind == Kind::Punct('.')) {
        tokens.pop();
        match tokens.last() {
            Some(token) if matches!(token.kind, Kind::Word | Kind::QuotedName) => {
                qualifier.insert(0, token.identifier(text).unwrap_or_default());
                tokens.pop();
            }
            _ => break,
        }
    }

    let clause = clause(text, &tokens, !qualifier.is_empty());

    let whole = lex(&text[statement_start..region_end.max(statement_start)], grammar);
    let mut statement: Vec<Token> = whole
        .tokens
        .iter()
        .map(|token| shift(*token, statement_start))
        .collect();
    if let Some(end) = statement
        .iter()
        .position(|token| token.kind == Kind::Punct(';') && token.depth == 0 && token.start >= cursor)
    {
        statement.truncate(end);
    }
    let tables = references(text, &statement, cursor);

    Some(CompletionContext {
        from,
        prefix,
        quote,
        qualifier,
        clause,
        tables,
    })
}

/// The statement, or SQL Server batch, the cursor is in; or the gap between two.
/// The statement, or SQL Server batch, the cursor is in, and whether the
/// cursor starts a new one: past a statement's end with a `;` or a `GO`
/// between, rather than only spaces and comments.
fn region(text: &str, cursor: usize, grammar: SqlGrammar) -> (usize, usize, bool) {
    let spans = split(text, grammar);
    let next = spans
        .iter()
        .filter(|span| span.start > cursor)
        .map(|span| span.start)
        .min()
        .unwrap_or(text.len());
    match spans.iter().rev().find(|span| span.start <= cursor) {
        Some(span) if cursor <= span.end => (span.start, span.end, false),
        Some(span) => {
            let fresh = !lex(&text[span.end..cursor], grammar).tokens.is_empty();
            if fresh {
                (span.end, next, true)
            } else {
                (span.start, next, false)
            }
        }
        None => (0, next, false),
    }
}

fn shift(mut token: Token, by: usize) -> Token {
    token.start += by;
    token.end += by;
    token
}

fn floor_char_boundary(text: &str, mut at: usize) -> usize {
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The clause the cursor is in, from the tokens before the word being typed.
fn clause(text: &str, tokens: &[Token], qualified: bool) -> Clause {
    let Some(last) = tokens.last() else {
        return Clause::Start;
    };
    let mut depth = if last.kind == Kind::Punct('(') {
        last.depth + 1
    } else {
        last.depth
    };
    let mut end = tokens.len();
    loop {
        // Back through this parenthesis level to the clause word that rules it.
        let mut opener = None;
        for index in (0..end).rev() {
            let token = &tokens[index];
            if token.depth < depth {
                opener = Some(index);
                break;
            }
            if token.depth > depth || token.kind != Kind::Word {
                continue;
            }
            let word = &text[token.start..token.end];
            let found = match word.to_ascii_uppercase().as_str() {
                "SELECT" | "WHERE" | "ON" | "HAVING" | "BY" | "SET" | "RETURNING" | "WHEN" | "THEN" | "ELSE" => {
                    Some(Clause::Expression)
                }
                "FROM" | "JOIN" | "UPDATE" | "INTO" | "TABLE" => {
                    Some(table_clause(text, tokens, index, end, qualified))
                }
                "LIMIT" | "OFFSET" | "FETCH" | "TOP" | "VALUES" | "CREATE" | "DROP" | "ALTER" | "USING" => {
                    Some(Clause::Other)
                }
                _ => None,
            };
            if let Some(clause) = found {
                return clause;
            }
        }
        match opener {
            // No clause word inside these parentheses: `INSERT INTO t (` lists columns;
            // a function call or an `IN (` list takes the clause outside.
            Some(index) => {
                if let Some(table) = insert_target(text, tokens, index) {
                    return Clause::InsertColumns(table);
                }
                depth = tokens[index].depth;
                end = index;
            }
            None if depth == 0 => {
                return if end == 0 { Clause::Start } else { Clause::Other };
            }
            None => depth -= 1,
        }
    }
}

/// After a table keyword: a table is expected right after it or after a
/// comma; past a table, an alias or the next keyword is.
fn table_clause(text: &str, tokens: &[Token], keyword: usize, end: usize, qualified: bool) -> Clause {
    if qualified || keyword + 1 == end {
        return Clause::Table;
    }
    let is_from = tokens[keyword].is_word(text, "FROM");
    match tokens[end - 1].kind {
        Kind::Punct(',') if is_from => Clause::Table,
        _ => Clause::AfterTable,
    }
}

/// The table of `INSERT INTO t (` when `opener` is that parenthesis.
fn insert_target(text: &str, tokens: &[Token], opener: usize) -> Option<TableRef> {
    let mut index = opener;
    let mut names = Vec::new();
    loop {
        let token = tokens.get(index.checked_sub(1)?)?;
        if !matches!(token.kind, Kind::Word | Kind::QuotedName)
            || (token.kind == Kind::Word && reserved(&text[token.start..token.end]))
        {
            break;
        }
        names.insert(0, token.identifier(text)?);
        index -= 1;
        match index.checked_sub(1).and_then(|at| tokens.get(at)) {
            Some(dot) if dot.kind == Kind::Punct('.') => index -= 1,
            _ => break,
        }
    }
    let into = tokens.get(index.checked_sub(1)?)?;
    if !into.is_word(text, "INTO") || names.is_empty() {
        return None;
    }
    Some(table_ref(&names))
}

fn table_ref(names: &[String]) -> TableRef {
    match names {
        [name] => TableRef {
            schema: None,
            name: name.clone(),
        },
        [.., schema, name] => TableRef {
            schema: Some(schema.clone()),
            name: name.clone(),
        },
        [] => TableRef {
            schema: None,
            name: String::new(),
        },
    }
}

/// The tables the statement names, at any depth, with their aliases. A name
/// the cursor is still in is left out: it is being typed, not named.
fn references(text: &str, tokens: &[Token], cursor: usize) -> Vec<TableReference> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        index += 1;
        let list = token.is_word(text, "FROM");
        if !(list
            || token.is_word(text, "JOIN")
            || token.is_word(text, "UPDATE")
            || token.is_word(text, "INTO")
            || token.is_word(text, "TABLE"))
        {
            continue;
        }
        loop {
            let at_name = index;
            let Some(first) = tokens.get(index) else { break };
            if first.kind == Kind::Punct('(') {
                // A derived table: skip it and its alias; its columns are not in the catalog.
                let depth = first.depth;
                index = tokens[index + 1..]
                    .iter()
                    .position(|t| t.kind == Kind::Punct(')') && t.depth == depth)
                    .map_or(tokens.len(), |offset| index + 1 + offset + 1);
                index = skip_alias(text, tokens, index).1;
            } else {
                let mut names = Vec::new();
                // A name that ends in a dot (`from sales.`) is still being typed.
                let mut dangling = false;
                while let Some(name) = tokens.get(index).filter(|t| is_name(text, t)) {
                    names.push(name.identifier(text).unwrap_or_default());
                    index += 1;
                    dangling = tokens.get(index).is_some_and(|t| t.kind == Kind::Punct('.'));
                    if dangling {
                        index += 1;
                    } else {
                        break;
                    }
                }
                if names.is_empty() {
                    break;
                }
                let typed = tokens[at_name..index]
                    .iter()
                    .any(|t| t.start < cursor && cursor <= t.end);
                let (alias, next) = skip_alias(text, tokens, index);
                index = next;
                if !typed && !dangling {
                    found.push(TableReference {
                        table: table_ref(&names),
                        alias,
                    });
                }
            }
            if list
                && tokens
                    .get(index)
                    .is_some_and(|t| t.kind == Kind::Punct(',') && t.depth == token.depth)
            {
                index += 1;
                continue;
            }
            break;
        }
    }
    found
}

fn is_name(text: &str, token: &Token) -> bool {
    match token.kind {
        Kind::QuotedName => true,
        Kind::Word => !reserved(&text[token.start..token.end]),
        _ => false,
    }
}

/// The alias after a table, with or without `AS`, and the index after it.
fn skip_alias(text: &str, tokens: &[Token], mut index: usize) -> (Option<String>, usize) {
    if tokens.get(index).is_some_and(|t| t.is_word(text, "AS")) {
        index += 1;
    }
    match tokens.get(index) {
        Some(token) if is_name(text, token) => (token.identifier(text), index + 1),
        _ => (None, index),
    }
}

impl CompletionContext {
    /// What the items need from the catalog, given its schemas.
    pub fn lookups(&self, schemas: &[SchemaInfo]) -> Lookups {
        let default = default_schema(schemas);
        let is_schema = |name: &str| schemas.iter().any(|schema| schema.name.eq_ignore_ascii_case(name));
        let mut lookups = Lookups::default();
        let mut objects = |schema: Option<&str>| {
            if let Some(schema) = schema.or(default.as_deref()) {
                lookups.objects_of.push(schema.to_owned());
            }
        };
        match (&self.clause, self.qualifier.as_slice()) {
            (Clause::Table, []) => objects(None),
            (Clause::Table, [.., schema]) => objects(Some(schema)),
            (Clause::Expression, []) => {
                for reference in &self.tables {
                    lookups.columns_of.push(resolve(&reference.table, default.as_deref()));
                }
            }
            (Clause::Expression, [name]) => match self.in_scope(name) {
                Some(table) => lookups.columns_of.push(resolve(table, default.as_deref())),
                None if is_schema(name) => objects(Some(name)),
                None => lookups
                    .columns_of
                    .push(resolve(&table_ref(std::slice::from_ref(name)), default.as_deref())),
            },
            (Clause::Expression, names) => lookups.columns_of.push(table_ref(names)),
            (Clause::InsertColumns(table), _) => lookups.columns_of.push(resolve(table, default.as_deref())),
            _ => {}
        }
        lookups
    }

    /// The table an alias or a table name stands for in this statement.
    fn in_scope(&self, name: &str) -> Option<&TableRef> {
        self.tables
            .iter()
            .find(|reference| {
                reference
                    .alias
                    .as_deref()
                    .is_some_and(|alias| alias.eq_ignore_ascii_case(name))
            })
            .or_else(|| {
                self.tables
                    .iter()
                    .find(|reference| reference.alias.is_none() && reference.table.name.eq_ignore_ascii_case(name))
            })
            .map(|reference| &reference.table)
    }
}

fn default_schema(schemas: &[SchemaInfo]) -> Option<String> {
    schemas
        .iter()
        .find(|schema| schema.is_default)
        .map(|schema| schema.name.clone())
}

/// A table named without a schema lives in the default one.
fn resolve(table: &TableRef, default: Option<&str>) -> TableRef {
    TableRef {
        schema: table.schema.clone().or_else(|| default.map(str::to_owned)),
        name: table.name.clone(),
    }
}

/// The items for `context`, from what its lookups found.
pub fn completions(context: &CompletionContext, catalog: &CatalogView, grammar: SqlGrammar) -> Vec<CompletionItem> {
    let default = default_schema(&catalog.schemas);
    let lookups = context.lookups(&catalog.schemas);
    let mut items = Vec::new();
    let item = |label: &str, kind: ItemKind, detail: String, boost: i8| CompletionItem {
        label: label.to_owned(),
        apply: apply(label, context.quote, grammar),
        kind,
        detail,
        boost,
    };

    for schema in &lookups.objects_of {
        for object in catalog.objects.iter().filter(|object| {
            object
                .schema
                .as_deref()
                .is_none_or(|own| own.eq_ignore_ascii_case(schema))
                && matches!(
                    object.kind,
                    ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView
                )
        }) {
            let kind = if object.kind == ObjectKind::Table {
                ItemKind::Table
            } else {
                ItemKind::View
            };
            items.push(item(&object.name, kind, object.schema.clone().unwrap_or_default(), 2));
        }
    }
    if context.clause == Clause::Table && context.qualifier.is_empty() && catalog.schemas.len() > 1 {
        for schema in &catalog.schemas {
            items.push(item(&schema.name, ItemKind::Schema, "schema".to_owned(), 1));
        }
    }

    let mut seen: Vec<(String, usize)> = Vec::new();
    for table in &lookups.columns_of {
        let Some((_, columns)) = catalog
            .columns
            .iter()
            .find(|(known, _)| same_table(known, table, default.as_deref()))
        else {
            continue;
        };
        let owner = context
            .tables
            .iter()
            .find(|reference| same_table(&reference.table, table, default.as_deref()))
            .and_then(|reference| reference.alias.clone())
            .unwrap_or_else(|| table.name.clone());
        for column in columns {
            // One item per name: a column two tables share says both.
            if let Some((_, at)) = seen.iter().find(|(name, _)| *name == column.name) {
                items[*at].detail.push_str(&format!(", {owner}"));
                continue;
            }
            seen.push((column.name.clone(), items.len()));
            items.push(item(
                &column.name,
                ItemKind::Column,
                format!("{} · {owner}", column.type_name),
                3,
            ));
        }
    }

    // The aliases and tables in scope, for `alias.` in an expression.
    if context.clause == Clause::Expression && context.qualifier.is_empty() {
        for reference in &context.tables {
            let label = reference.alias.as_ref().unwrap_or(&reference.table.name);
            if !items.iter().any(|item| item.label == *label) {
                items.push(item(label, ItemKind::Alias, reference.table.name.clone(), 1));
            }
        }
    }
    items
}

fn same_table(a: &TableRef, b: &TableRef, default: Option<&str>) -> bool {
    let schema = |table: &TableRef| table.schema.clone().or_else(|| default.map(str::to_owned));
    a.name.eq_ignore_ascii_case(&b.name)
        && match (schema(a), schema(b)) {
            (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
            _ => true,
        }
}

/// The name as it must be written: inside an open quote, its doubled close
/// and the closing quote; otherwise quoted only when bare it would not read
/// as the same name.
fn apply(name: &str, quote: Option<char>, grammar: SqlGrammar) -> String {
    if let Some(close) = quote {
        return format!("{}{close}", name.replace(close, &format!("{close}{close}")));
    }
    if needs_quotes(name, grammar) {
        match grammar {
            SqlGrammar::SqlServer => super::quote_ident_bracket(name),
            SqlGrammar::Postgres | SqlGrammar::Sqlite => super::quote_ident(name),
        }
    } else {
        name.to_owned()
    }
}

fn needs_quotes(name: &str, grammar: SqlGrammar) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    // PostgreSQL folds a bare name to lower case, so an upper-case letter needs quotes there.
    let letter = |ch: char| match grammar {
        SqlGrammar::Postgres => ch.is_ascii_lowercase() || !ch.is_ascii(),
        SqlGrammar::SqlServer | SqlGrammar::Sqlite => ch.is_ascii_alphabetic() || !ch.is_ascii(),
    };
    !(letter(first) || first == '_')
        || !chars.all(|ch| letter(ch) || ch.is_ascii_digit() || ch == '_' || ch == '$')
        || reserved(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The context at the `|` in `text`.
    fn at(text: &str, grammar: SqlGrammar) -> Option<CompletionContext> {
        let cursor = text.find('|').unwrap_or(text.len());
        let text = text.replacen('|', "", 1);
        completion_context(&text, cursor, grammar)
    }

    fn pg(text: &str) -> CompletionContext {
        at(text, SqlGrammar::Postgres).unwrap_or_else(|| panic!("a context for {text}"))
    }

    fn table(schema: Option<&str>, name: &str) -> TableRef {
        TableRef {
            schema: schema.map(str::to_owned),
            name: name.to_owned(),
        }
    }

    #[test]
    fn the_clause_follows_the_word_that_rules_it() {
        let cases = [
            ("|", Clause::Start),
            ("sel|", Clause::Start),
            ("select 1; |", Clause::Start),
            ("select |", Clause::Expression),
            ("select a, | from t", Clause::Expression),
            ("select * from |", Clause::Table),
            ("select * from ord|", Clause::Table),
            ("select * from a, |", Clause::Table),
            ("select * from a |", Clause::AfterTable),
            ("select * from a join |", Clause::Table),
            ("select * from a join b on |", Clause::Expression),
            ("select * from a where x = 1 and |", Clause::Expression),
            ("select * from a order by |", Clause::Expression),
            ("update t set |", Clause::Expression),
            ("delete from |", Clause::Table),
            ("select * from t limit |", Clause::Other),
            ("select count(| from t", Clause::Expression),
            ("select * from t where id in (select | from u)", Clause::Expression),
            ("select * from (select 1) s where |", Clause::Expression),
            ("insert into t values (|", Clause::Other),
            (
                "insert into public.t (a, |",
                Clause::InsertColumns(table(Some("public"), "t")),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(pg(text).clause, expected, "{text}");
        }
    }

    #[test]
    fn no_context_inside_a_string_or_a_comment() {
        assert!(at("select 'abc|", SqlGrammar::Postgres).is_none());
        assert!(at("select 1 -- note |", SqlGrammar::Postgres).is_none());
        assert!(at("select /* a |", SqlGrammar::Postgres).is_none());
        assert!(at("select 1 -- note\n|", SqlGrammar::Postgres).is_some());
    }

    #[test]
    fn the_word_typed_and_its_qualifier() {
        let context = pg("select o.cu| from orders o");
        assert_eq!(context.prefix, "cu");
        assert_eq!(context.from, "select o.".len());
        assert_eq!(context.qualifier, ["o"]);

        let context = pg("select * from sales.|");
        assert_eq!(
            (context.prefix.as_str(), context.qualifier.as_slice()),
            ("", &["sales".to_owned()][..])
        );
        assert_eq!(context.clause, Clause::Table);

        let context = pg("select \"Order\".\"Cu|");
        assert_eq!(context.prefix, "Cu");
        assert_eq!(context.quote, Some('"'));
        assert_eq!(context.qualifier, ["Order"]);
        assert_eq!(context.from, "select \"Order\".\"".len());

        let context = at("select [o].[cu|", SqlGrammar::SqlServer).unwrap_or_else(|| panic!("a context"));
        assert_eq!((context.prefix.as_str(), context.quote), ("cu", Some(']')));
    }

    #[test]
    fn the_tables_in_scope_and_their_aliases() {
        let context = pg(
            "select | from public.orders as o, items join customers c on c.id = o.customer_id \
             where exists (select 1 from payments p)",
        );
        let found: Vec<(TableRef, Option<String>)> = context
            .tables
            .into_iter()
            .map(|reference| (reference.table, reference.alias))
            .collect();
        assert_eq!(
            found,
            [
                (table(Some("public"), "orders"), Some("o".to_owned())),
                (table(None, "items"), None),
                (table(None, "customers"), Some("c".to_owned())),
                (table(None, "payments"), Some("p".to_owned())),
            ]
        );
    }

    #[test]
    fn a_table_being_typed_is_not_in_scope_and_other_statements_are_not_either() {
        let context = pg("select * from orders o; select * from ord|");
        assert!(context.tables.is_empty(), "{:?}", context.tables);
        assert!(pg("select * from sales.|").tables.is_empty());
        let context = pg("select | from a; select * from b");
        assert_eq!(context.tables.len(), 1);
        assert_eq!(context.tables[0].table.name, "a");
    }

    #[test]
    fn a_sql_server_batch_reads_its_statements_without_semicolons() {
        let context = at(
            "select 1\nselect x.| from dbo.orders x\nGO\nselect 2",
            SqlGrammar::SqlServer,
        )
        .unwrap_or_else(|| panic!("a context"));
        assert_eq!(context.clause, Clause::Expression);
        assert_eq!(context.qualifier, ["x"]);
        assert_eq!(context.tables[0].table, table(Some("dbo"), "orders"));

        let fresh = at("select * from dbo.orders\nGO\n|", SqlGrammar::SqlServer).unwrap_or_else(|| panic!("a context"));
        assert_eq!(fresh.clause, Clause::Start);
        assert!(fresh.tables.is_empty());
        let same = at("select *\n  |", SqlGrammar::SqlServer).unwrap_or_else(|| panic!("a context"));
        assert_eq!(same.clause, Clause::Expression);
    }

    fn schemas() -> Vec<SchemaInfo> {
        vec![
            SchemaInfo {
                name: "public".into(),
                is_default: true,
            },
            SchemaInfo {
                name: "sales".into(),
                is_default: false,
            },
        ]
    }

    fn column(name: &str, type_name: &str) -> ColumnInfo {
        ColumnInfo {
            ordinal: 0,
            name: name.into(),
            type_name: type_name.into(),
            nullable: true,
            default: None,
            primary_key: None,
        }
    }

    fn catalog() -> CatalogView {
        let object = |schema: &str, name: &str, kind| ObjectInfo {
            schema: Some(schema.into()),
            name: name.into(),
            kind,
        };
        CatalogView {
            schemas: schemas(),
            objects: vec![
                object("public", "orders", ObjectKind::Table),
                object("public", "Order Lines", ObjectKind::Table),
                object("public", "big_orders", ObjectKind::View),
                object("public", "orders_id_seq", ObjectKind::Sequence),
                object("sales", "targets", ObjectKind::Table),
            ],
            columns: vec![
                (
                    table(Some("public"), "orders"),
                    vec![column("id", "int4"), column("customer_id", "int4")],
                ),
                (
                    table(Some("public"), "customers"),
                    vec![column("id", "int4"), column("Full Name", "text")],
                ),
            ],
        }
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    #[test]
    fn after_from_the_default_schemas_tables_and_views_and_the_schemas() {
        let items = completions(&pg("select * from |"), &catalog(), SqlGrammar::Postgres);
        assert_eq!(
            labels(&items),
            ["orders", "Order Lines", "big_orders", "public", "sales"]
        );
        assert_eq!(items[1].apply, "\"Order Lines\"");
        assert_eq!(items[2].kind, ItemKind::View);

        let context = pg("select * from sales.|");
        assert_eq!(context.lookups(&schemas()).objects_of, ["sales"]);
        let items = completions(&context, &catalog(), SqlGrammar::Postgres);
        assert_eq!(labels(&items), ["targets"]);
    }

    #[test]
    fn an_expression_offers_the_columns_of_the_tables_in_scope() {
        let context = pg("select | from orders o join customers c on c.id = o.customer_id");
        assert_eq!(
            context.lookups(&schemas()).columns_of,
            [table(Some("public"), "orders"), table(Some("public"), "customers")]
        );
        let items = completions(&context, &catalog(), SqlGrammar::Postgres);
        assert_eq!(labels(&items), ["id", "customer_id", "Full Name", "o", "c"]);
        assert_eq!(items[0].detail, "int4 · o, c");
        assert_eq!(items[2].apply, "\"Full Name\"");
    }

    #[test]
    fn an_alias_before_a_dot_offers_its_tables_columns_only() {
        let context = pg("select c.| from orders o join customers c on true");
        assert_eq!(
            context.lookups(&schemas()).columns_of,
            [table(Some("public"), "customers")]
        );
        let items = completions(&context, &catalog(), SqlGrammar::Postgres);
        assert_eq!(labels(&items), ["id", "Full Name"]);
        // A schema before the dot offers its tables instead.
        let context = pg("select * from orders where id in (select id from x) and sales.|");
        assert_eq!(context.lookups(&schemas()).objects_of, ["sales"]);
    }

    #[test]
    fn insert_columns_come_from_the_target_table() {
        let context = pg("insert into orders (|");
        let items = completions(&context, &catalog(), SqlGrammar::Postgres);
        assert_eq!(labels(&items), ["id", "customer_id"]);
    }

    #[test]
    fn inside_an_open_quote_the_name_closes_it() {
        let items = completions(&pg("select * from \"Ord|"), &catalog(), SqlGrammar::Postgres);
        let lines = items
            .iter()
            .find(|item| item.label == "Order Lines")
            .map(|item| item.apply.as_str());
        assert_eq!(lines, Some("Order Lines\""));
    }

    #[test]
    fn a_name_is_quoted_only_when_bare_it_would_read_differently() {
        assert_eq!(apply("orders", None, SqlGrammar::Postgres), "orders");
        assert_eq!(apply("Orders", None, SqlGrammar::Postgres), "\"Orders\"");
        assert_eq!(apply("Orders", None, SqlGrammar::SqlServer), "Orders");
        assert_eq!(apply("order", None, SqlGrammar::SqlServer), "[order]");
        assert_eq!(apply("2fa", None, SqlGrammar::Sqlite), "\"2fa\"");
        assert_eq!(apply("khách_hàng", None, SqlGrammar::Postgres), "khách_hàng");
        assert_eq!(apply("a]b", Some(']'), SqlGrammar::SqlServer), "a]]b]");
    }

    #[test]
    fn keywords_only_where_names_do_not_help() {
        for text in ["|", "select * from t limit |", "select * from orders o |"] {
            assert!(
                completions(&pg(text), &catalog(), SqlGrammar::Postgres).is_empty(),
                "{text}"
            );
        }
    }

    #[test]
    fn multi_byte_text_before_the_cursor_keeps_offsets_on_char_boundaries() {
        let text = "select 'đơn hàng', khá| from orders";
        let context = pg(text);
        assert_eq!(context.prefix, "khá");
        assert!(text.replacen('|', "", 1).is_char_boundary(context.from));
        // A cursor inside a character is moved back to its start, never past it.
        let plain = "select đ";
        assert!(completion_context(plain, plain.len() - 1, SqlGrammar::Postgres).is_some());
    }
}
