//! Reading SQL scripts the way each engine's own client does.

mod browse;
mod completion;
mod database_change;
mod lex;
mod sorted;
mod split;

pub use browse::{BrowseQuery, BrowseSort, Filter, FilterOp, count_statement, page_statement};
pub use completion::{
    CatalogView, Clause, CompletionContext, CompletionItem, ItemKind, Lookups, TableReference, completion_context,
    completions,
};
pub use database_change::{DatabaseChange, database_change};
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
    use super::*;

    #[test]
    fn quoting_doubles_the_closing_quote() {
        assert_eq!(quote_ident(r#"a"b"#), r#""a""b""#);
        assert_eq!(quote_ident_bracket("a]b"), "[a]]b]");
    }
}
