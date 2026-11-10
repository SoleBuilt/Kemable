//! A filter on a query result's rows, and the `WHERE` clause that says the
//! same thing to the server (`docs/decisions/0011-filtering-a-result.md`).
//!
//! The store applies the filter to the rows it holds (`paboable_store`); this
//! module only checks it and writes it as SQL to copy. A condition names a
//! column by its index in the result, because two columns of a result can
//! share a name. The clause is never run by Paboable.
//!
//! A condition on a column with no name, or with a name another column of
//! the result shares (case folded, as `sorted.rs` reads names), cannot be
//! written: the server would read another column or none. The clause leaves
//! it out and says which ones it left out.

use serde::{Deserialize, Serialize};

use super::SqlGrammar;
use super::browse::{Filter, FilterOp, condition};
use crate::error::{DriverError, ErrorCode};

/// One condition on a result's rows. `op` is the table tab's, but never
/// `raw`: a SQL condition can only be read by a server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultFilter {
    /// The column's index in the result.
    pub column: usize,
    pub op: FilterOp,
    #[serde(default)]
    pub value: String,
    /// The upper end of `between`.
    #[serde(default)]
    pub second: String,
}

impl ResultFilter {
    /// Whether the filter can apply to a result of `width` columns:
    /// `filter.raw` for a SQL condition, `filter.invalid` for a column past
    /// the last or an `in` with no values.
    pub fn check(&self, width: usize) -> Result<(), DriverError> {
        if self.op == FilterOp::Raw {
            return Err(DriverError::new(
                ErrorCode::FilterRaw,
                "a SQL condition filters only on the server",
            ));
        }
        if self.column >= width {
            return Err(DriverError::new(
                ErrorCode::FilterInvalid,
                format!("no column {} in a result of {width}", self.column),
            ));
        }
        if self.op == FilterOp::In && in_items(&self.value).next().is_none() {
            return Err(DriverError::new(
                ErrorCode::FilterInvalid,
                "an IN filter needs at least one value",
            ));
        }
        Ok(())
    }
}

/// The values of an `in` filter: comma-separated, trimmed, empty ones
/// skipped, as the table tab reads them (`browse.rs`).
pub fn in_items(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|item| !item.is_empty())
}

/// A result's filter as a `WHERE` clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultWhere {
    /// `WHERE ...` with no trailing `;`, empty when no condition could be written.
    pub sql: String,
    /// The indexes, in `filters`, of the conditions the clause leaves out.
    pub left_out: Vec<usize>,
}

/// Writes `filters` on a result whose columns are named `columns` as a
/// `WHERE` for `grammar`, with the quoting and literals of a table tab's
/// filters. Every filter is checked first ([`ResultFilter::check`]).
pub fn result_where(
    grammar: SqlGrammar,
    columns: &[String],
    filters: &[ResultFilter],
    match_any: bool,
) -> Result<ResultWhere, DriverError> {
    for filter in filters {
        filter.check(columns.len())?;
    }
    let mut conditions = Vec::with_capacity(filters.len());
    let mut left_out = Vec::new();
    for (index, filter) in filters.iter().enumerate() {
        let name = &columns[filter.column];
        let unique = columns.iter().filter(|other| other.eq_ignore_ascii_case(name)).count() == 1;
        if name.is_empty() || !unique {
            left_out.push(index);
            continue;
        }
        conditions.push(condition(
            grammar,
            &Filter {
                column: name.clone(),
                op: filter.op,
                value: filter.value.clone(),
                second: filter.second.clone(),
            },
        )?);
    }
    let sql = if conditions.is_empty() {
        String::new()
    } else {
        format!(
            "WHERE {}",
            conditions.join(if match_any { "\n   OR " } else { "\n  AND " })
        )
    };
    Ok(ResultWhere { sql, left_out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(column: usize, op: FilterOp, value: &str) -> ResultFilter {
        ResultFilter {
            column,
            op,
            value: value.to_owned(),
            second: String::new(),
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn postgres_writes_the_conditions_as_a_table_tab_does() {
        let columns = names(&["id", "name", "total"]);
        let filters = [
            filter(2, FilterOp::Greater, "100"),
            filter(1, FilterOp::Contains, "o'b%"),
            filter(0, FilterOp::In, "1, 2,,3"),
        ];
        let clause = result_where(SqlGrammar::Postgres, &columns, &filters, false).unwrap();
        assert_eq!(
            clause.sql,
            "WHERE \"total\" > '100'\n  AND \"name\"::text ILIKE '%o''b\\%%' ESCAPE '\\'\n  AND \"id\" IN ('1', '2', '3')"
        );
        assert!(clause.left_out.is_empty());
    }

    #[test]
    fn sql_server_quotes_with_brackets_and_writes_unicode_literals() {
        let columns = names(&["we]ird", "d"]);
        let mut range = filter(1, FilterOp::Between, "2026-01-01");
        range.second = "2026-12-31".into();
        let filters = [filter(0, FilterOp::Equals, "xin chào"), range];
        let clause = result_where(SqlGrammar::SqlServer, &columns, &filters, true).unwrap();
        assert_eq!(
            clause.sql,
            "WHERE [we]]ird] = N'xin chào'\n   OR [d] BETWEEN N'2026-01-01' AND N'2026-12-31'"
        );
    }

    #[test]
    fn sqlite_writes_a_plain_number_bare() {
        let columns = names(&["n", "code"]);
        let filters = [
            filter(0, FilterOp::LessOrEqual, "42"),
            filter(1, FilterOp::Equals, "007"),
            filter(1, FilterOp::IsNotNull, ""),
        ];
        let clause = result_where(SqlGrammar::Sqlite, &columns, &filters, false).unwrap();
        assert_eq!(
            clause.sql,
            "WHERE \"n\" <= 42\n  AND \"code\" = '007'\n  AND \"code\" IS NOT NULL"
        );
    }

    #[test]
    fn a_column_whose_name_is_shared_or_empty_is_left_out_and_reported() {
        // A join gives two `id` columns; an expression may give no name.
        let columns = names(&["id", "ID", "", "total"]);
        let filters = [
            filter(0, FilterOp::Equals, "1"),
            filter(3, FilterOp::IsNull, ""),
            filter(2, FilterOp::Equals, "x"),
            filter(1, FilterOp::Equals, "2"),
        ];
        let clause = result_where(SqlGrammar::Postgres, &columns, &filters, false).unwrap();
        assert_eq!(clause.sql, "WHERE \"total\" IS NULL");
        assert_eq!(clause.left_out, [0, 2, 3]);

        let none = result_where(SqlGrammar::Postgres, &columns, &filters[..1], false).unwrap();
        assert_eq!(none.sql, "");
        assert_eq!(none.left_out, [0]);
    }

    #[test]
    fn a_sql_condition_or_a_column_past_the_last_is_refused_with_its_code() {
        let columns = names(&["a"]);
        let raw = result_where(
            SqlGrammar::Sqlite,
            &columns,
            &[filter(0, FilterOp::Raw, "a > 1")],
            false,
        );
        assert_eq!(raw.err().map(|error| error.code), Some(ErrorCode::FilterRaw));
        let past = result_where(SqlGrammar::Sqlite, &columns, &[filter(1, FilterOp::Equals, "1")], false);
        assert_eq!(past.err().map(|error| error.code), Some(ErrorCode::FilterInvalid));
        let empty_in = result_where(SqlGrammar::Sqlite, &columns, &[filter(0, FilterOp::In, " , ")], false);
        assert_eq!(empty_in.err().map(|error| error.code), Some(ErrorCode::FilterInvalid));
    }

    #[test]
    fn no_filter_writes_no_clause() {
        let clause = result_where(SqlGrammar::Postgres, &names(&["a"]), &[], true).unwrap();
        assert_eq!(
            clause,
            ResultWhere {
                sql: String::new(),
                left_out: Vec::new(),
            }
        );
    }
}
