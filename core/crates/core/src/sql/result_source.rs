//! The one table a query result comes from, for "Open <table> to edit"
//! (`docs/decisions/0011-filtering-a-result.md`, amended 2026-11-11).
//!
//! Only a plain `SELECT ... FROM <table>` names one: no join, no comma in the
//! `FROM`, no subquery or function there, no `GROUP BY`, `HAVING`, `DISTINCT`,
//! `INTO`, set operation or `FOR`. The select list may be anything, since the
//! table opens whole. Its `WHERE` comes along as table tab filters when every
//! condition is one the filter row holds (a column, a comparison and literal
//! values, joined all by `AND` or all by `OR`); otherwise the table opens
//! without it and `where_left` says so.

use serde::Serialize;

use super::SqlGrammar;
use super::batch::batch_statement_count;
use super::browse::{Filter, FilterOp};
use super::lex::{Kind, Token, lex_strict};
use crate::catalog::TableRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultSource {
    /// A SQL Server three-part name's database (`db.dbo.t`); `None` is the run's.
    pub database: Option<String>,
    pub table: TableRef,
    /// The `WHERE` as filters, when every condition fits one.
    pub filters: Vec<Filter>,
    /// The conditions were joined by `OR`.
    pub match_any: bool,
    /// The statement had a `WHERE` the filters could not hold: the table opens whole.
    pub where_left: bool,
}

/// Words at the top level that make a result more than the rows of one table.
const REFUSED: &[&str] = &[
    "JOIN",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "GROUP",
    "HAVING",
    "INTO",
    "DISTINCT",
    "APPLY",
    "PIVOT",
    "UNPIVOT",
    "FOR",
    "WINDOW",
];
/// Clauses after the `WHERE`.
const AFTER_WHERE: &[&str] = &["ORDER", "LIMIT", "OFFSET", "FETCH", "OPTION"];

/// The table behind `statement`'s result, or `None` when it is not exactly one.
pub fn result_source(statement: &str, grammar: SqlGrammar) -> Option<ResultSource> {
    if grammar == SqlGrammar::SqlServer && batch_statement_count(statement)? != 1 {
        return None;
    }
    let mut tokens = lex_strict(statement, grammar)?;
    while tokens
        .last()
        .is_some_and(|token| token.depth == 0 && token.kind == Kind::Punct(';'))
    {
        tokens.pop();
    }
    let top = |token: &Token| token.depth == 0;
    if tokens.iter().any(|token| top(token) && token.kind == Kind::Punct(';')) {
        return None;
    }
    if !tokens.first()?.is_word(statement, "SELECT") {
        return None;
    }
    if tokens
        .iter()
        .any(|token| top(token) && REFUSED.iter().any(|word| token.is_word(statement, word)))
    {
        return None;
    }
    let mut froms = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| top(token) && token.is_word(statement, "FROM"));
    let (from, _) = froms.next()?;
    if froms.next().is_some() {
        return None;
    }

    // The table's name: one to three parts.
    let mut at = from + 1;
    let mut parts = Vec::new();
    loop {
        let token = tokens.get(at)?;
        if !matches!(token.kind, Kind::Word | Kind::QuotedName) || !top(token) {
            return None;
        }
        parts.push(token.identifier(statement)?);
        at += 1;
        if tokens.get(at).is_some_and(|token| token.kind == Kind::Punct('.')) {
            at += 1;
        } else {
            break;
        }
    }
    let (database, schema, name) = match (grammar, parts.as_slice()) {
        (_, [name]) => (None, None, name.clone()),
        (_, [schema, name]) => (None, Some(schema.clone()), name.clone()),
        (SqlGrammar::SqlServer, [database, schema, name]) => {
            (Some(database.clone()), Some(schema.clone()), name.clone())
        }
        _ => return None,
    };
    // A table-valued function is not a table.
    if tokens.get(at).is_some_and(|token| token.kind == Kind::Punct('(')) {
        return None;
    }

    // An alias, and SQL Server's table hints.
    let mut alias = None;
    if tokens.get(at).is_some_and(|token| token.is_word(statement, "AS")) {
        at += 1;
        alias = Some(tokens.get(at)?.identifier(statement)?);
        at += 1;
    } else if let Some(token) = tokens.get(at)
        && matches!(token.kind, Kind::Word | Kind::QuotedName)
        && !["WHERE", "WITH"]
            .iter()
            .chain(AFTER_WHERE)
            .any(|word| token.is_word(statement, word))
    {
        alias = Some(token.identifier(statement)?);
        at += 1;
    }
    if tokens.get(at).is_some_and(|token| token.is_word(statement, "WITH"))
        && tokens.get(at + 1).is_some_and(|token| token.kind == Kind::Punct('('))
    {
        at += 2;
        while tokens.get(at).is_some_and(|token| token.depth > 0) {
            at += 1;
        }
        // The closing parenthesis.
        at += 1;
    }

    let mut source = ResultSource {
        database,
        table: TableRef { schema, name },
        filters: Vec::new(),
        match_any: false,
        where_left: false,
    };
    match tokens.get(at) {
        None => return Some(source),
        Some(token) if token.is_word(statement, "WHERE") => {}
        Some(token) if AFTER_WHERE.iter().any(|word| token.is_word(statement, word)) => return Some(source),
        // A comma (another table), or anything else the FROM cannot end with.
        Some(_) => return None,
    }
    let start = at + 1;
    let end = tokens[start..]
        .iter()
        .position(|token| top(token) && AFTER_WHERE.iter().any(|word| token.is_word(statement, word)))
        .map_or(tokens.len(), |offset| start + offset);
    let names = [Some(source.table.name.clone()), alias];
    match conditions(statement, &tokens[start..end], &names) {
        Some((filters, match_any)) => {
            source.filters = filters;
            source.match_any = match_any;
        }
        None => source.where_left = true,
    }
    Some(source)
}

/// The `WHERE`'s conditions as filters, and whether they are joined by `OR`.
fn conditions(text: &str, tokens: &[Token], names: &[Option<String>; 2]) -> Option<(Vec<Filter>, bool)> {
    let mut filters = Vec::new();
    let mut joiner: Option<bool> = None;
    let mut at = 0;
    loop {
        let (filter, next) = condition(text, tokens, at, names)?;
        filters.push(filter);
        at = next;
        let Some(token) = tokens.get(at) else { break };
        let or = if token.is_word(text, "AND") {
            false
        } else if token.is_word(text, "OR") {
            true
        } else {
            return None;
        };
        if joiner.is_some_and(|before| before != or) {
            return None;
        }
        joiner = Some(or);
        at += 1;
    }
    Some((filters, joiner.unwrap_or(false)))
}

/// One condition starting at `at`, and where the next token is.
fn condition(text: &str, tokens: &[Token], at: usize, names: &[Option<String>; 2]) -> Option<(Filter, usize)> {
    let (column, mut at) = column(text, tokens, at, names)?;
    let word = |at: usize, word: &str| {
        tokens
            .get(at)
            .is_some_and(|token| token.depth == 0 && token.is_word(text, word))
    };
    let filter = |op: FilterOp, value: String, second: String| Filter {
        column: column.clone(),
        op,
        value,
        second,
    };

    if word(at, "IS") {
        let not = word(at + 1, "NOT");
        let null_at = at + 1 + usize::from(not);
        if !word(null_at, "NULL") {
            return None;
        }
        let op = if not { FilterOp::IsNotNull } else { FilterOp::IsNull };
        return Some((filter(op, String::new(), String::new()), null_at + 1));
    }
    let not = word(at, "NOT");
    if not {
        at += 1;
    }
    if word(at, "LIKE") {
        let (pattern, next) = literal(text, tokens, at + 1)?;
        if word(next, "ESCAPE") {
            return None;
        }
        let op = like(&pattern, not)?;
        let value = pattern.trim_matches('%').to_owned();
        return Some((filter(op, value, String::new()), next));
    }
    if word(at, "IN") && !not {
        let open = tokens.get(at + 1)?;
        if open.kind != Kind::Punct('(') {
            return None;
        }
        let mut values = Vec::new();
        let mut next = at + 2;
        loop {
            let (value, after) = literal(text, tokens, next)?;
            if value.contains(',') {
                return None;
            }
            values.push(value);
            match tokens.get(after) {
                Some(token) if token.kind == Kind::Punct(',') => next = after + 1,
                Some(token) if token.kind == Kind::Punct(')') => {
                    return Some((filter(FilterOp::In, values.join(","), String::new()), after + 1));
                }
                _ => return None,
            }
        }
    }
    if word(at, "BETWEEN") && !not {
        let (low, after) = literal(text, tokens, at + 1)?;
        if !word(after, "AND") {
            return None;
        }
        let (high, next) = literal(text, tokens, after + 1)?;
        return Some((filter(FilterOp::Between, low, high), next));
    }
    if not {
        return None;
    }
    let (op, after) = comparison(tokens, at)?;
    let (value, next) = literal(text, tokens, after)?;
    Some((filter(op, value, String::new()), next))
}

/// A column, maybe qualified by the table's name or alias; its own name.
fn column(text: &str, tokens: &[Token], mut at: usize, names: &[Option<String>; 2]) -> Option<(String, usize)> {
    let mut parts = Vec::new();
    loop {
        let token = tokens.get(at)?;
        if token.depth != 0 || !matches!(token.kind, Kind::Word | Kind::QuotedName) {
            return None;
        }
        // A variable or a keyword is no column.
        let raw = &text[token.start..token.end];
        if token.kind == Kind::Word
            && (raw.starts_with('@')
                || ["NOT", "NULL", "EXISTS", "CASE"]
                    .iter()
                    .any(|word| raw.eq_ignore_ascii_case(word)))
        {
            return None;
        }
        parts.push(token.identifier(text)?);
        at += 1;
        if tokens.get(at).is_some_and(|token| token.kind == Kind::Punct('.')) {
            at += 1;
        } else {
            break;
        }
    }
    let name = parts.pop()?;
    match parts.as_slice() {
        [] => {}
        [qualifier] if names.iter().flatten().any(|name| name.eq_ignore_ascii_case(qualifier)) => {}
        _ => return None,
    }
    Some((name, at))
}

/// `=`, `<>`, `!=`, `<`, `<=`, `>`, `>=`, read from the one-character tokens the lexer gives.
fn comparison(tokens: &[Token], at: usize) -> Option<(FilterOp, usize)> {
    let first = tokens.get(at)?;
    let second = tokens.get(at + 1).filter(|token| token.start == first.end);
    let pair = |second_char: char| second.is_some_and(|token| token.kind == Kind::Punct(second_char));
    match first.kind {
        Kind::Punct('=') => Some((FilterOp::Equals, at + 1)),
        Kind::Punct('!') if pair('=') => Some((FilterOp::NotEquals, at + 2)),
        Kind::Punct('<') if pair('>') => Some((FilterOp::NotEquals, at + 2)),
        Kind::Punct('<') if pair('=') => Some((FilterOp::LessOrEqual, at + 2)),
        Kind::Punct('<') => Some((FilterOp::Less, at + 1)),
        Kind::Punct('>') if pair('=') => Some((FilterOp::GreaterOrEqual, at + 2)),
        Kind::Punct('>') => Some((FilterOp::Greater, at + 1)),
        _ => None,
    }
}

/// A literal value as a filter holds it: a string without its quotes, or a
/// number as written (a sign kept). `N'...'` is a string too.
fn literal(text: &str, tokens: &[Token], at: usize) -> Option<(String, usize)> {
    let token = tokens.get(at)?;
    match token.kind {
        Kind::Punct('-') => {
            let number = tokens
                .get(at + 1)
                .filter(|next| next.start == token.end && next.kind == Kind::Literal)?;
            let raw = &text[number.start..number.end];
            raw.as_bytes()
                .first()?
                .is_ascii_digit()
                .then(|| (format!("-{raw}"), at + 2))
        }
        Kind::Word if text[token.start..token.end].eq_ignore_ascii_case("N") => {
            let string = tokens
                .get(at + 1)
                .filter(|next| next.start == token.end && next.kind == Kind::Literal)?;
            Some((string_value(&text[string.start..string.end])?, at + 2))
        }
        Kind::Word
            if ["TRUE", "FALSE"]
                .iter()
                .any(|word| text[token.start..token.end].eq_ignore_ascii_case(word)) =>
        {
            Some((text[token.start..token.end].to_ascii_lowercase(), at + 1))
        }
        Kind::Literal => {
            let raw = &text[token.start..token.end];
            if raw.starts_with('\'') {
                Some((string_value(raw)?, at + 1))
            } else if raw.as_bytes().first()?.is_ascii_digit() {
                Some((raw.to_owned(), at + 1))
            } else {
                // E'...', $$...$$, X'...': not a plain value.
                None
            }
        }
        _ => None,
    }
}

fn string_value(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner.replace("''", "'"))
}

/// The filter a `LIKE` pattern is: `%x%`, `x%` or `%x`, with no other wildcard.
fn like(pattern: &str, not: bool) -> Option<FilterOp> {
    let inner = pattern.trim_matches('%');
    if inner.is_empty() || inner.contains(['%', '_', '[']) {
        return None;
    }
    let leading = pattern.starts_with('%');
    let trailing = pattern.ends_with('%');
    // Only one % on each side: "%%x" would trim the same and mean something else.
    if pattern.len() - inner.len() != usize::from(leading) + usize::from(trailing) {
        return None;
    }
    match (leading, trailing, not) {
        (true, true, false) => Some(FilterOp::Contains),
        (true, true, true) => Some(FilterOp::NotContains),
        (false, true, false) => Some(FilterOp::StartsWith),
        (true, false, false) => Some(FilterOp::EndsWith),
        (false, false, false) => Some(FilterOp::Equals),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(statement: &str, grammar: SqlGrammar) -> Option<ResultSource> {
        result_source(statement, grammar)
    }

    fn f(column: &str, op: FilterOp, value: &str, second: &str) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: value.into(),
            second: second.into(),
        }
    }

    #[test]
    fn a_plain_select_from_one_table_names_it() {
        let found = source(
            "select * from dbo.[CRONUS International Ltd_$Customer$437dbf0e]",
            SqlGrammar::SqlServer,
        )
        .unwrap();
        assert_eq!(
            found.table,
            TableRef {
                schema: Some("dbo".into()),
                name: "CRONUS International Ltd_$Customer$437dbf0e".into()
            }
        );
        assert!(found.filters.is_empty() && !found.where_left);

        let found = source(
            "SELECT TOP (10) c.No_, c.Name FROM [Demo DB].dbo.Customer AS c WITH (NOLOCK) ORDER BY c.Name;",
            SqlGrammar::SqlServer,
        )
        .unwrap();
        assert_eq!(found.database.as_deref(), Some("Demo DB"));
        assert_eq!(found.table.name, "Customer");

        let found = source(
            "select id, total * 2 from public.\"Orders\" o limit 50",
            SqlGrammar::Postgres,
        )
        .unwrap();
        assert_eq!(
            found.table,
            TableRef {
                schema: Some("public".into()),
                name: "Orders".into()
            }
        );
    }

    #[test]
    fn a_result_of_more_than_one_table_names_none() {
        for statement in [
            "select * from a join b on a.id = b.id",
            "select * from a, b",
            "select count(*) from a group by x",
            "select distinct x from a",
            "select * from a union select * from b",
            "select * from (select * from a) s",
            "select * from fn_rows(1)",
            "select * into b from a",
            "with c as (select 1) select * from c",
            "select 1",
            "select * from a; select * from b",
            "update a set x = 1",
            "select * from a cross apply b",
        ] {
            assert_eq!(source(statement, SqlGrammar::SqlServer), None, "{statement}");
        }
        // Two statements in one SQL Server batch are not one table's result.
        assert_eq!(source("select * from a\nselect * from b", SqlGrammar::SqlServer), None);
    }

    #[test]
    fn a_where_the_filter_row_can_hold_becomes_its_filters() {
        let found = source(
            "select * from dbo.Customer c where c.City = N'Miami' and Balance >= -12.5 and [Name] like '%Furn%' \
             and Code in ('A', 'B') and Amount between 10 and 20 and Blocked is not null and Note <> 'x''y' order by Name",
            SqlGrammar::SqlServer,
        )
        .unwrap();
        assert!(!found.where_left && !found.match_any);
        assert_eq!(
            found.filters,
            [
                f("City", FilterOp::Equals, "Miami", ""),
                f("Balance", FilterOp::GreaterOrEqual, "-12.5", ""),
                f("Name", FilterOp::Contains, "Furn", ""),
                f("Code", FilterOp::In, "A,B", ""),
                f("Amount", FilterOp::Between, "10", "20"),
                f("Blocked", FilterOp::IsNotNull, "", ""),
                f("Note", FilterOp::NotEquals, "x'y", ""),
            ]
        );
        let found = source(
            "select * from t where a = 1 or b like 'x%' or c is null",
            SqlGrammar::Postgres,
        )
        .unwrap();
        assert!(found.match_any);
        assert_eq!(found.filters[1], f("b", FilterOp::StartsWith, "x", ""));
    }

    #[test]
    fn a_where_it_cannot_hold_leaves_the_table_whole_and_says_so() {
        for statement in [
            "select * from t where a = 1 and b = 2 or c = 3",
            "select * from t where (a = 1 or b = 2) and c = 3",
            "select * from t where upper(a) = 'X'",
            "select * from t where a = b",
            "select * from t where a like 'x_y%'",
            "select * from t where a not in (1, 2)",
            "select * from t where a = @n",
            "select * from t where x.a = 1",
            "select * from t where a in ('1,5', '2')",
            "select * from t where exists (select 1 from u)",
        ] {
            let found = source(statement, SqlGrammar::SqlServer).unwrap_or_else(|| panic!("a table for {statement}"));
            assert!(found.where_left && found.filters.is_empty(), "{statement}");
        }
    }

    #[test]
    fn vietnamese_text_in_names_and_values_survives() {
        let found = source(
            "select * from [Khách hàng] where [Thành phố] = N'Hồ Chí Minh'",
            SqlGrammar::SqlServer,
        )
        .unwrap();
        assert_eq!(found.table.name, "Khách hàng");
        assert_eq!(found.filters, [f("Thành phố", FilterOp::Equals, "Hồ Chí Minh", "")]);
    }
}
