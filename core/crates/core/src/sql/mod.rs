//! Reading SQL scripts the way each engine's own client does.

mod batch;
mod browse;
mod completion;
mod database_change;
mod edit;
mod lex;
mod literal;
mod object_action;
mod result_filter;
mod result_source;
mod schema_change;
mod sorted;
mod split;

pub use batch::{batch_queries, batch_statement_count, split_after_created_databases};
pub use browse::{BrowseQuery, BrowseSort, Filter, FilterOp, count_statement, export_statement, page_statement};
pub use completion::{
    CatalogView, Clause, CompletionContext, CompletionItem, ItemKind, Lookups, TableReference, completion_context,
    completions,
};
pub use database_change::{DatabaseChange, database_change};
pub use edit::{NewValue, RowEdit, edit_statement};
pub use literal::{typed as typed_literal, value_literal};
pub use object_action::{Maintenance, ObjectAction, action_statements, maintenance_tasks, view_definition};
pub use result_filter::{ResultFilter, ResultWhere, in_items, result_where};
pub use result_source::{ResultSource, result_source};
pub use schema_change::may_change_schema;
pub use sorted::{SortColumn, sorted_statement};
pub use split::{StatementKind, StatementSpan, split, statement_at};

use serde::{Deserialize, Serialize};

/// The lexical rules a script is read with. One grammar per engine family,
/// shared by every engine that speaks it (Neon and Supabase read as
/// PostgreSQL, D1 as SQLite).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SqlGrammar {
    Postgres,
    SqlServer,
    Sqlite,
}

/// A name quoted for `grammar`.
pub fn quote_name(grammar: SqlGrammar, name: &str) -> String {
    match grammar {
        SqlGrammar::SqlServer => quote_ident_bracket(name),
        SqlGrammar::Postgres | SqlGrammar::Sqlite => quote_ident(name),
    }
}

/// A name as a person would paste it into a statement: quoted only when it
/// must be, for Copy Qualified Name (`dbo.[CRONUS International Ltd_$Customer]`,
/// `public."Orders"`). A part needs quotes when it is not a plain identifier
/// of the grammar, or is a keyword; PostgreSQL folds unquoted names to lower
/// case, so a capital letter needs them there.
pub fn readable_name(grammar: SqlGrammar, schema: Option<&str>, name: &str) -> String {
    let part = |part: &str| {
        if is_plain_identifier(grammar, part) {
            part.to_owned()
        } else {
            quote_name(grammar, part)
        }
    };
    match schema {
        Some(schema) => format!("{}.{}", part(schema), part(name)),
        None => part(name),
    }
}

fn is_plain_identifier(grammar: SqlGrammar, part: &str) -> bool {
    let mut chars = part.chars();
    let Some(first) = chars.next() else { return false };
    let lower_only = grammar == SqlGrammar::Postgres;
    let starts = first == '_'
        || if lower_only {
            first.is_ascii_lowercase()
        } else {
            first.is_ascii_alphabetic()
        };
    let rest = chars.all(|ch| {
        ch == '_'
            || ch == '$'
            || ch.is_ascii_digit()
            || if lower_only {
                ch.is_ascii_lowercase()
            } else {
                ch.is_ascii_alphabetic()
            }
    });
    starts && rest && !KEYWORDS.iter().any(|word| word.eq_ignore_ascii_case(part))
}

/// Words that mean something in a statement of every grammar: a name spelled
/// like one is quoted, so `order` and `user` can be pasted as names.
const KEYWORDS: &[&str] = &[
    "ADD",
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
    "CURRENT",
    "DATABASE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXISTS",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NOT",
    "NULL",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "PRIMARY",
    "REFERENCES",
    "RIGHT",
    "SELECT",
    "SET",
    "TABLE",
    "THEN",
    "TO",
    "TOP",
    "TRANSACTION",
    "TRIGGER",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USER",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WITH",
];

/// A table's name, with its schema when it has one, quoted for `grammar`.
pub fn table_name(grammar: SqlGrammar, table: &crate::catalog::TableRef) -> String {
    match &table.schema {
        Some(schema) => format!("{}.{}", quote_name(grammar, schema), quote_name(grammar, &table.name)),
        None => quote_name(grammar, &table.name),
    }
}

/// Quotes an identifier with double quotes, as PostgreSQL and SQLite read it.
pub fn quote_ident(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for ch in name.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// Quotes an identifier with brackets, as SQL Server reads it.
pub fn quote_ident_bracket(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('[');
    for ch in name.chars() {
        if ch == ']' {
            out.push(']');
        }
        out.push(ch);
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_readable_name_quotes_only_the_parts_that_need_it() {
        let customer = "CRONUS International Ltd_$Customer$437dbf0e";
        assert_eq!(
            readable_name(SqlGrammar::SqlServer, Some("dbo"), customer),
            "dbo.[CRONUS International Ltd_$Customer$437dbf0e]"
        );
        assert_eq!(
            readable_name(SqlGrammar::SqlServer, Some("dbo"), "Customer"),
            "dbo.Customer"
        );
        assert_eq!(readable_name(SqlGrammar::SqlServer, Some("dbo"), "a]b"), "dbo.[a]]b]");
        assert_eq!(
            readable_name(SqlGrammar::Postgres, Some("public"), "orders"),
            "public.orders"
        );
        // PostgreSQL folds unquoted names to lower case: Orders needs its quotes.
        assert_eq!(
            readable_name(SqlGrammar::Postgres, Some("public"), "Orders"),
            r#"public."Orders""#
        );
        assert_eq!(
            readable_name(SqlGrammar::Postgres, Some("bán hàng"), "order"),
            r#""bán hàng"."order""#
        );
        assert_eq!(
            readable_name(SqlGrammar::Sqlite, Some("main"), "Khách hàng"),
            r#"main."Khách hàng""#
        );
        assert_eq!(readable_name(SqlGrammar::Sqlite, None, "user"), r#""user""#);
        assert_eq!(readable_name(SqlGrammar::Sqlite, None, "2024_sales"), r#""2024_sales""#);
    }
    use super::*;

    #[test]
    fn quoting_doubles_the_closing_quote() {
        assert_eq!(quote_ident(r#"a"b"#), r#""a""b""#);
        assert_eq!(quote_ident_bracket("a]b"), "[a]]b]");
    }
}
