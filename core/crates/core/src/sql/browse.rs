//! The statements a table tab runs: one page of a table, filtered and sorted
//! on the server, and the count of the rows its filters match
//! (`docs/decisions/0006-table-browsing.md`).
//!
//! Every name comes from the catalog and is quoted; every value is written as
//! a literal with its quotes doubled, so a filter's value can never end the
//! statement. A raw filter is the user's own SQL and is written as typed,
//! in parentheses, but it must hold a single expression: a `;` refuses it.

use serde::Deserialize;

use super::literal::typed as literal;
use super::{SqlGrammar, StatementKind, quote_name as ident, split, table_name};
use crate::catalog::TableRef;
use crate::error::DriverError;

/// How a filter compares its column with its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    /// `value` is a comma-separated list.
    In,
    /// From `value` to `second`, both included.
    Between,
    IsNull,
    IsNotNull,
    /// `value` is a SQL condition written as it is; `column` is ignored.
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub second: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseSort {
    pub column: String,
    pub descending: bool,
}

/// One page of a table, as a table tab asks for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseQuery {
    pub table: TableRef,
    #[serde(default)]
    pub filters: Vec<Filter>,
    /// The rows match any filter, instead of every one.
    #[serde(default)]
    pub match_any: bool,
    #[serde(default)]
    pub sort: Vec<BrowseSort>,
    /// The primary key's columns, in key order: they break ties, so a row is
    /// on exactly one page, and they order a table nobody sorted.
    #[serde(default)]
    pub key: Vec<String>,
    pub offset: u64,
    pub limit: u64,
}

/// The page `query` asks for.
pub fn page_statement(grammar: SqlGrammar, query: &BrowseQuery) -> Result<String, DriverError> {
    if query.limit == 0 {
        return Err(DriverError::config("a page holds at least one row"));
    }
    let mut sql = format!("SELECT * FROM {}", table_name(grammar, &query.table));
    push_where(&mut sql, grammar, query)?;

    let mut order: Vec<String> = query
        .sort
        .iter()
        .map(|key| {
            format!(
                "{}{}",
                ident(grammar, &key.column),
                if key.descending { " DESC" } else { "" }
            )
        })
        .collect();
    for column in &query.key {
        if !query.sort.iter().any(|key| key.column == *column) {
            order.push(ident(grammar, column));
        }
    }

    match grammar {
        SqlGrammar::Postgres | SqlGrammar::Sqlite => {
            if !order.is_empty() {
                sql.push_str("\nORDER BY ");
                sql.push_str(&order.join(", "));
            }
            sql.push_str(&format!("\nLIMIT {}", query.limit));
            if query.offset > 0 {
                sql.push_str(&format!(" OFFSET {}", query.offset));
            }
        }
        SqlGrammar::SqlServer => {
            // OFFSET ... FETCH needs an ORDER BY; with no key, any order will do.
            sql.push_str("\nORDER BY ");
            if order.is_empty() {
                sql.push_str("(SELECT NULL)");
            } else {
                sql.push_str(&order.join(", "));
            }
            sql.push_str(&format!(
                "\nOFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
                query.offset, query.limit
            ));
        }
    }
    Ok(sql)
}

/// How many rows `query`'s filters match, as one row of one column.
pub fn count_statement(grammar: SqlGrammar, query: &BrowseQuery) -> Result<String, DriverError> {
    let mut sql = format!("SELECT COUNT(*) FROM {}", table_name(grammar, &query.table));
    push_where(&mut sql, grammar, query)?;
    Ok(sql)
}

fn push_where(sql: &mut String, grammar: SqlGrammar, query: &BrowseQuery) -> Result<(), DriverError> {
    let conditions = query
        .filters
        .iter()
        .map(|filter| condition(grammar, filter))
        .collect::<Result<Vec<_>, _>>()?;
    if conditions.is_empty() {
        return Ok(());
    }
    sql.push_str("\nWHERE ");
    sql.push_str(&conditions.join(if query.match_any { "\n   OR " } else { "\n  AND " }));
    Ok(())
}

fn condition(grammar: SqlGrammar, filter: &Filter) -> Result<String, DriverError> {
    let column = if filter.op == FilterOp::Raw {
        String::new()
    } else if filter.column.is_empty() {
        return Err(DriverError::config("a filter needs a column"));
    } else {
        ident(grammar, &filter.column)
    };
    let value = || literal(grammar, &filter.value);
    let text = match filter.op {
        FilterOp::Equals => format!("{column} = {}", value()),
        FilterOp::NotEquals => format!("{column} <> {}", value()),
        FilterOp::Greater => format!("{column} > {}", value()),
        FilterOp::GreaterOrEqual => format!("{column} >= {}", value()),
        FilterOp::Less => format!("{column} < {}", value()),
        FilterOp::LessOrEqual => format!("{column} <= {}", value()),
        FilterOp::Contains => like(
            grammar,
            &column,
            &format!("%{}%", escape_like(grammar, &filter.value)),
            false,
        ),
        FilterOp::NotContains => like(
            grammar,
            &column,
            &format!("%{}%", escape_like(grammar, &filter.value)),
            true,
        ),
        FilterOp::StartsWith => like(
            grammar,
            &column,
            &format!("{}%", escape_like(grammar, &filter.value)),
            false,
        ),
        FilterOp::EndsWith => like(
            grammar,
            &column,
            &format!("%{}", escape_like(grammar, &filter.value)),
            false,
        ),
        FilterOp::In => {
            let items: Vec<String> = filter
                .value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| literal(grammar, item))
                .collect();
            if items.is_empty() {
                return Err(DriverError::config("an IN filter needs at least one value"));
            }
            format!("{column} IN ({})", items.join(", "))
        }
        FilterOp::Between => format!(
            "{column} BETWEEN {} AND {}",
            literal(grammar, &filter.value),
            literal(grammar, &filter.second)
        ),
        FilterOp::IsNull => format!("{column} IS NULL"),
        FilterOp::IsNotNull => format!("{column} IS NOT NULL"),
        FilterOp::Raw => return raw_condition(grammar, &filter.value),
    };
    Ok(text)
}

fn raw_condition(grammar: SqlGrammar, text: &str) -> Result<String, DriverError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(DriverError::config("a SQL filter needs a condition"));
    }
    let spans = split(text, grammar);
    if spans.len() != 1 || spans.iter().any(|span| span.kind != StatementKind::Sql) || text.contains(';') {
        return Err(DriverError::config("a SQL filter is one condition, without ';'"));
    }
    // On its own line, so a `--` comment at its end cannot swallow what follows.
    Ok(format!(
        "({text}
)"
    ))
}

/// A LIKE that ignores case where the engine lets it, on a column of any type.
fn like(grammar: SqlGrammar, column: &str, pattern: &str, negated: bool) -> String {
    let not = if negated { "NOT " } else { "" };
    match grammar {
        // `::text` lets a number or a date match too; on a text column it costs nothing.
        SqlGrammar::Postgres => format!("{column}::text {not}ILIKE {} ESCAPE '\\'", literal(grammar, pattern)),
        // SQLite's LIKE ignores ASCII case; SQL Server's follows the column's collation.
        SqlGrammar::Sqlite | SqlGrammar::SqlServer => {
            format!("{column} {not}LIKE {} ESCAPE '\\'", literal(grammar, pattern))
        }
    }
}

fn escape_like(grammar: SqlGrammar, value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        let special = matches!(ch, '\\' | '%' | '_') || (grammar == SqlGrammar::SqlServer && ch == '[');
        if special {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(schema: Option<&str>, name: &str) -> BrowseQuery {
        BrowseQuery {
            table: TableRef {
                schema: schema.map(str::to_owned),
                name: name.to_owned(),
            },
            filters: Vec::new(),
            match_any: false,
            sort: Vec::new(),
            key: Vec::new(),
            offset: 0,
            limit: 1000,
        }
    }

    fn filter(column: &str, op: FilterOp, value: &str) -> Filter {
        Filter {
            column: column.to_owned(),
            op,
            value: value.to_owned(),
            second: String::new(),
        }
    }

    #[test]
    fn a_first_page_has_a_limit_and_no_offset() {
        let page = page_statement(SqlGrammar::Postgres, &query(Some("public"), "orders"));
        assert_eq!(
            page.ok().as_deref(),
            Some("SELECT * FROM \"public\".\"orders\"\nLIMIT 1000")
        );
    }

    #[test]
    fn a_later_page_orders_by_the_key_and_skips_the_pages_before() {
        let mut browse = query(None, "orders");
        browse.key = vec!["id".into()];
        browse.offset = 2000;
        let page = page_statement(SqlGrammar::Sqlite, &browse);
        assert_eq!(
            page.ok().as_deref(),
            Some("SELECT * FROM \"orders\"\nORDER BY \"id\"\nLIMIT 1000 OFFSET 2000")
        );
    }

    #[test]
    fn a_sort_comes_first_and_the_key_breaks_ties() {
        let mut browse = query(Some("dbo"), "orders");
        browse.key = vec!["id".into()];
        browse.sort = vec![
            BrowseSort {
                column: "total".into(),
                descending: true,
            },
            BrowseSort {
                column: "id".into(),
                descending: false,
            },
        ];
        browse.offset = 50;
        browse.limit = 25;
        let page = page_statement(SqlGrammar::SqlServer, &browse);
        assert_eq!(
            page.ok().as_deref(),
            Some("SELECT * FROM [dbo].[orders]\nORDER BY [total] DESC, [id]\nOFFSET 50 ROWS FETCH NEXT 25 ROWS ONLY")
        );
    }

    #[test]
    fn sql_server_pages_a_table_with_no_key() {
        let page = page_statement(SqlGrammar::SqlServer, &query(Some("dbo"), "log"));
        assert_eq!(
            page.ok().as_deref(),
            Some("SELECT * FROM [dbo].[log]\nORDER BY (SELECT NULL)\nOFFSET 0 ROWS FETCH NEXT 1000 ROWS ONLY")
        );
    }

    #[test]
    fn names_are_quoted_so_a_strange_name_stays_a_name() {
        let mut browse = query(Some("we\"ird"), "ta]ble");
        browse.sort = vec![BrowseSort {
            column: "a\"b".into(),
            descending: false,
        }];
        let postgres = page_statement(SqlGrammar::Postgres, &browse).unwrap_or_default();
        assert!(postgres.starts_with("SELECT * FROM \"we\"\"ird\".\"ta]ble\"\nORDER BY \"a\"\"b\""));
        let mssql = page_statement(SqlGrammar::SqlServer, &browse).unwrap_or_default();
        assert!(mssql.starts_with("SELECT * FROM [we\"ird].[ta]]ble]"));
    }

    #[test]
    fn a_value_cannot_end_its_literal() {
        let mut browse = query(None, "users");
        browse.filters = vec![filter("name", FilterOp::Equals, "O'Brien'; drop table users; --")];
        let count = count_statement(SqlGrammar::Postgres, &browse).unwrap_or_default();
        assert_eq!(
            count,
            "SELECT COUNT(*) FROM \"users\"\nWHERE \"name\" = 'O''Brien''; drop table users; --'"
        );
        let mssql = count_statement(SqlGrammar::SqlServer, &browse).unwrap_or_default();
        assert!(mssql.ends_with("[name] = N'O''Brien''; drop table users; --'"));
    }

    #[test]
    fn filters_join_with_and_or_with_or() {
        let mut browse = query(None, "t");
        browse.filters = vec![filter("a", FilterOp::IsNull, ""), filter("b", FilterOp::Greater, "5")];
        let every = count_statement(SqlGrammar::Sqlite, &browse).unwrap_or_default();
        assert_eq!(
            every,
            "SELECT COUNT(*) FROM \"t\"\nWHERE \"a\" IS NULL\n  AND \"b\" > 5"
        );
        browse.match_any = true;
        let any = count_statement(SqlGrammar::Sqlite, &browse).unwrap_or_default();
        assert_eq!(any, "SELECT COUNT(*) FROM \"t\"\nWHERE \"a\" IS NULL\n   OR \"b\" > 5");
    }

    #[test]
    fn contains_escapes_the_wildcards_and_ignores_case() {
        let mut browse = query(None, "t");
        browse.filters = vec![filter("code", FilterOp::Contains, "50%_off\\[x]")];
        let postgres = count_statement(SqlGrammar::Postgres, &browse).unwrap_or_default();
        assert!(
            postgres.ends_with("\"code\"::text ILIKE '%50\\%\\_off\\\\[x]%' ESCAPE '\\'"),
            "{postgres}"
        );
        let mssql = count_statement(SqlGrammar::SqlServer, &browse).unwrap_or_default();
        assert!(
            mssql.ends_with("[code] LIKE N'%50\\%\\_off\\\\\\[x]%' ESCAPE '\\'"),
            "{mssql}"
        );
    }

    #[test]
    fn each_operator_writes_its_condition() {
        let cases = [
            (FilterOp::NotEquals, "x", "\"c\" <> 'x'"),
            (FilterOp::NotContains, "x", "\"c\"::text NOT ILIKE '%x%' ESCAPE '\\'"),
            (FilterOp::StartsWith, "x", "\"c\"::text ILIKE 'x%' ESCAPE '\\'"),
            (FilterOp::EndsWith, "x", "\"c\"::text ILIKE '%x' ESCAPE '\\'"),
            (FilterOp::GreaterOrEqual, "1", "\"c\" >= '1'"),
            (FilterOp::Less, "1", "\"c\" < '1'"),
            (FilterOp::LessOrEqual, "1", "\"c\" <= '1'"),
            (FilterOp::In, " a, b ,,c ", "\"c\" IN ('a', 'b', 'c')"),
            (FilterOp::IsNotNull, "", "\"c\" IS NOT NULL"),
        ];
        for (op, value, expected) in cases {
            let mut browse = query(None, "t");
            browse.filters = vec![filter("c", op, value)];
            let count = count_statement(SqlGrammar::Postgres, &browse).unwrap_or_default();
            assert_eq!(count, format!("SELECT COUNT(*) FROM \"t\"\nWHERE {expected}"), "{op:?}");
        }
    }

    #[test]
    fn between_takes_both_ends() {
        let mut browse = query(None, "t");
        let mut range = filter("d", FilterOp::Between, "2026-01-01");
        range.second = "2026-12-31".into();
        browse.filters = vec![range];
        let count = count_statement(SqlGrammar::Postgres, &browse).unwrap_or_default();
        assert!(count.ends_with("\"d\" BETWEEN '2026-01-01' AND '2026-12-31'"));
    }

    #[test]
    fn sqlite_writes_a_number_bare_and_a_code_with_a_leading_zero_as_text() {
        assert_eq!(literal(SqlGrammar::Sqlite, "42"), "42");
        assert_eq!(literal(SqlGrammar::Sqlite, "-3.5"), "-3.5");
        assert_eq!(literal(SqlGrammar::Sqlite, "0"), "0");
        assert_eq!(literal(SqlGrammar::Sqlite, "007"), "'007'");
        assert_eq!(literal(SqlGrammar::Sqlite, "1."), "'1.'");
        assert_eq!(literal(SqlGrammar::Sqlite, "1e5"), "'1e5'");
        assert_eq!(literal(SqlGrammar::Postgres, "42"), "'42'");
    }

    #[test]
    fn a_raw_filter_is_one_condition_on_its_own_lines() {
        let mut browse = query(None, "t");
        browse.filters = vec![
            filter("", FilterOp::Raw, "total > 100 -- big ones"),
            filter("a", FilterOp::IsNull, ""),
        ];
        let count = count_statement(SqlGrammar::Postgres, &browse).unwrap_or_default();
        assert_eq!(
            count,
            "SELECT COUNT(*) FROM \"t\"\nWHERE (total > 100 -- big ones\n)\n  AND \"a\" IS NULL"
        );
    }

    #[test]
    fn a_raw_filter_with_a_second_statement_is_refused() {
        let mut browse = query(None, "t");
        browse.filters = vec![filter("", FilterOp::Raw, "1 = 1; drop table t")];
        let error = page_statement(SqlGrammar::Postgres, &browse).err();
        assert_eq!(
            error.map(|error| error.code),
            Some(crate::error::ErrorCode::ConfigInvalid)
        );
        // A `;` inside a string is still refused: a filter has no use for one.
        browse.filters = vec![filter("", FilterOp::Raw, "name = ';'")];
        assert!(page_statement(SqlGrammar::Postgres, &browse).is_err());
        browse.filters = vec![filter("", FilterOp::Raw, "  ")];
        assert!(page_statement(SqlGrammar::Postgres, &browse).is_err());
    }

    #[test]
    fn a_filter_with_no_column_or_no_values_is_refused() {
        let mut browse = query(None, "t");
        browse.filters = vec![filter("", FilterOp::Equals, "1")];
        assert!(count_statement(SqlGrammar::Sqlite, &browse).is_err());
        browse.filters = vec![filter("c", FilterOp::In, " , ")];
        assert!(count_statement(SqlGrammar::Sqlite, &browse).is_err());
        browse.filters = Vec::new();
        browse.limit = 0;
        assert!(page_statement(SqlGrammar::Sqlite, &browse).is_err());
    }
}
