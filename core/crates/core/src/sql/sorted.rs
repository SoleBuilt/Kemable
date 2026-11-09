//! Writing a statement again with an `ORDER BY`, for sorting a result on the
//! server (`docs/decisions/0005-sorting-a-result.md`).
//!
//! Only one plain `SELECT` (or `WITH ... SELECT`) is written again; anything
//! that might change data, take locks, or not run again the same way gives
//! `None`, and the caller sorts the rows it holds instead. A sort key names
//! its column when the name is unique in the result and its select item is
//! the bare `*`, an unqualified column or an alias; otherwise it gives the
//! column's position, which PostgreSQL, SQLite and SQL Server all read.
//!
//! The new `ORDER BY` replaces the statement's own top-level one and goes
//! before `LIMIT`, `OFFSET`, `FETCH` and SQL Server's `OPTION (...)`, on a
//! line of its own so a `--` comment before it cannot swallow it. A comment
//! after the last token is left out.

use super::{SqlGrammar, quote_ident, quote_ident_bracket};

/// One column of a sort, by its index in the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortColumn {
    pub column: usize,
    pub descending: bool,
}

/// The statement written again with `ORDER BY keys`, or `None` when it
/// cannot safely run again. `columns` are the result's column names.
pub fn sorted_statement(
    statement: &str,
    grammar: SqlGrammar,
    columns: &[String],
    keys: &[SortColumn],
) -> Option<String> {
    if keys.is_empty() || keys.iter().any(|key| key.column >= columns.len()) {
        return None;
    }
    let mut tokens = lex(statement, grammar)?;
    // One statement: a trailing `;` is dropped, any other top-level one refuses.
    while tokens
        .last()
        .is_some_and(|token| token.depth == 0 && token.kind == Kind::Punct(';'))
    {
        tokens.pop();
    }
    if tokens
        .iter()
        .any(|token| token.depth == 0 && token.kind == Kind::Punct(';'))
    {
        return None;
    }
    let first = tokens.first()?;
    if !(first.is_word(statement, "SELECT") || first.is_word(statement, "WITH")) {
        return None;
    }
    if tokens
        .iter()
        .any(|token| token.kind == Kind::Word && WRITES.iter().any(|word| token.is_word(statement, word)))
    {
        return None;
    }
    let top: Vec<&Token> = tokens.iter().filter(|token| token.depth == 0).collect();
    if top
        .iter()
        .any(|token| token.is_word(statement, "INTO") || token.is_word(statement, "FOR"))
    {
        return None;
    }

    // Where the old ORDER BY starts, and where the clauses after it start.
    let order_at = top.windows(2).find_map(|pair| {
        (pair[0].is_word(statement, "ORDER") && pair[1].is_word(statement, "BY")).then_some(pair[0].start)
    });
    let tail_at = top
        .iter()
        .filter(|token| order_at.is_none_or(|order| token.start > order))
        .find(|token| TAIL.iter().any(|word| token.is_word(statement, word)))
        .map(|token| token.start);
    let end = tokens.last()?.end;
    let cut = order_at.or(tail_at).unwrap_or(end);

    let names = safe_names(statement, &tokens, columns);
    let references: Vec<String> = keys
        .iter()
        .map(|key| {
            let reference = match &names[key.column] {
                Some(name) if grammar == SqlGrammar::SqlServer => quote_ident_bracket(name),
                Some(name) => quote_ident(name),
                None => (key.column + 1).to_string(),
            };
            if key.descending {
                format!("{reference} DESC")
            } else {
                reference
            }
        })
        .collect();

    let mut out = statement[..cut].trim_end().to_owned();
    out.push_str("\nORDER BY ");
    out.push_str(&references.join(", "));
    if let Some(tail) = tail_at {
        out.push(' ');
        out.push_str(statement[tail..end].trim_end());
    }
    Some(out)
}

/// Words that mean the statement writes, calls something, or is not a query.
const WRITES: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "TRUNCATE", "DROP", "ALTER", "CREATE", "EXEC", "EXECUTE", "CALL", "GRANT",
    "REVOKE", "COPY",
];

/// Clauses that follow an ORDER BY.
const TAIL: &[&str] = &["LIMIT", "OFFSET", "FETCH", "OPTION"];

/// For each result column, its name when an `ORDER BY` may use it.
fn safe_names(statement: &str, tokens: &[Token], columns: &[String]) -> Vec<Option<String>> {
    let unique = |name: &str| columns.iter().filter(|other| other.eq_ignore_ascii_case(name)).count() == 1;
    let items = select_items(statement, tokens);
    let bare_star = items.len() == 1 && items[0].len() == 1 && items[0][0].kind == Kind::Punct('*');
    columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            if !unique(name) {
                return None;
            }
            if bare_star {
                return Some(name.clone());
            }
            let item = items.get(index).filter(|_| items.len() == columns.len())?;
            let last = item.last()?;
            let named = last.identifier(statement)?.eq_ignore_ascii_case(name);
            // A lone column, `expr AS name`, or `expr name`; never `t.name`.
            let unqualified = match item.len() {
                1 => true,
                count => item[count - 2].kind != Kind::Punct('.'),
            };
            (named && unqualified).then(|| name.clone())
        })
        .collect()
}

/// The first top-level select list, split at its commas.
fn select_items<'t>(statement: &str, tokens: &'t [Token]) -> Vec<Vec<&'t Token>> {
    let Some(select) = tokens
        .iter()
        .position(|token| token.depth == 0 && token.is_word(statement, "SELECT"))
    else {
        return Vec::new();
    };
    let mut at = select + 1;
    // SELECT [ALL | DISTINCT [ON (...)]] [TOP n | TOP (n) [PERCENT] [WITH TIES]]
    let word = |at: usize, word: &str| tokens.get(at).is_some_and(|token| token.is_word(statement, word));
    if word(at, "ALL") || word(at, "DISTINCT") {
        at += 1;
        if word(at, "ON") {
            at = skip_group(tokens, at + 1);
        }
    }
    if word(at, "TOP") {
        at = skip_group(tokens, at + 1);
        if word(at, "PERCENT") {
            at += 1;
        }
        if word(at, "WITH") && word(at + 1, "TIES") {
            at += 2;
        }
    }
    let mut items = vec![Vec::new()];
    for token in &tokens[at..] {
        if token.depth == 0 && END_OF_LIST.iter().any(|word| token.is_word(statement, word)) {
            break;
        }
        if token.depth == 0 && token.kind == Kind::Punct(',') {
            items.push(Vec::new());
        } else if token.depth == 0 {
            if let Some(item) = items.last_mut() {
                item.push(token);
            }
        } else if let Some(item) = items.last_mut() {
            // Inside parentheses: the item is an expression, not a name.
            item.push(token);
        }
    }
    items
}

const END_OF_LIST: &[&str] = &[
    "FROM",
    "WHERE",
    "GROUP",
    "HAVING",
    "WINDOW",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "INTO",
    "OPTION",
    "FOR",
];

/// Skips one token, or a whole parenthesised group, starting at `at`.
fn skip_group(tokens: &[Token], at: usize) -> usize {
    match tokens.get(at) {
        Some(token) if token.kind == Kind::Punct('(') => {
            let depth = token.depth;
            tokens[at + 1..]
                .iter()
                .position(|t| t.kind == Kind::Punct(')') && t.depth == depth)
                .map_or(tokens.len(), |offset| at + 1 + offset + 1)
        }
        Some(_) => at + 1,
        None => at,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    QuotedName,
    Literal,
    Punct(char),
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
    /// Parentheses around the token; a `(` and its `)` sit at the outer depth.
    depth: u32,
}

impl Token {
    fn is_word(&self, text: &str, word: &str) -> bool {
        self.kind == Kind::Word && text[self.start..self.end].eq_ignore_ascii_case(word)
    }

    /// The name a word or quoted name stands for.
    fn identifier(&self, text: &str) -> Option<String> {
        let raw = &text[self.start..self.end];
        match self.kind {
            Kind::Word => Some(raw.to_owned()),
            Kind::QuotedName => {
                let close = raw.chars().last()?;
                let inner = raw.get(1..raw.len() - close.len_utf8())?;
                Some(inner.replace(&format!("{close}{close}"), &close.to_string()))
            }
            _ => None,
        }
    }
}

/// The tokens of a statement, comments dropped. `None` when a quote, comment
/// or parenthesis is left open, since nothing sure can be said about it.
fn lex(text: &str, grammar: SqlGrammar) -> Option<Vec<Token>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut depth = 0u32;
    let mut i = 0;
    let nested = matches!(grammar, SqlGrammar::Postgres | SqlGrammar::SqlServer);
    while i < bytes.len() {
        let byte = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        let start = i;
        let kind = match byte {
            b if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'-' if next == b'-' => {
                i = bytes[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(bytes.len(), |n| i + n + 1);
                continue;
            }
            b'/' if next == b'*' => {
                let mut level = 0;
                loop {
                    if i + 1 >= bytes.len() {
                        return None;
                    }
                    if bytes[i] == b'/' && bytes[i + 1] == b'*' && (level == 0 || nested) {
                        level += 1;
                        i += 2;
                    } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        level -= 1;
                        i += 2;
                        if level == 0 {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            b'\'' => {
                let backslash = grammar == SqlGrammar::Postgres && i > 0 && matches!(bytes[i - 1], b'E' | b'e');
                i = quoted(bytes, i, b'\'', backslash)?;
                Kind::Literal
            }
            b'"' => {
                i = quoted(bytes, i, b'"', false)?;
                Kind::QuotedName
            }
            b'[' if grammar == SqlGrammar::SqlServer => {
                i = quoted(bytes, i, b']', false)?;
                Kind::QuotedName
            }
            b'`' if grammar == SqlGrammar::Sqlite => {
                i = quoted(bytes, i, b'`', false)?;
                Kind::QuotedName
            }
            b'$' if grammar == SqlGrammar::Postgres && (next == b'$' || next.is_ascii_alphabetic() || next == b'_') => {
                let tag_end = bytes[i + 1..].iter().position(|&b| b == b'$').map(|n| i + 1 + n)?;
                let tag = &bytes[i..=tag_end];
                let body = tag_end + 1;
                let close = bytes[body..].windows(tag.len()).position(|w| w == tag)?;
                i = body + close + tag.len();
                Kind::Literal
            }
            b if b.is_ascii_alphabetic() || b == b'_' || b >= 0x80 || b == b'@' || b == b'#' => {
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric()
                        || matches!(bytes[i], b'_' | b'$' | b'@' | b'#')
                        || bytes[i] >= 0x80)
                {
                    i += 1;
                }
                Kind::Word
            }
            b if b.is_ascii_digit() => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                    i += 1;
                }
                Kind::Literal
            }
            b'(' => {
                tokens.push(Token {
                    kind: Kind::Punct('('),
                    start,
                    end: i + 1,
                    depth,
                });
                depth += 1;
                i += 1;
                continue;
            }
            b')' => {
                depth = depth.checked_sub(1)?;
                tokens.push(Token {
                    kind: Kind::Punct(')'),
                    start,
                    end: i + 1,
                    depth,
                });
                i += 1;
                continue;
            }
            other => {
                i += 1;
                Kind::Punct(char::from(other))
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
            depth,
        });
    }
    (depth == 0).then_some(tokens)
}

/// The end of a quoted run opened at `start`, a doubled close staying inside.
fn quoted(bytes: &[u8], start: usize, close: u8, backslash: bool) -> Option<usize> {
    let mut i = start + 1;
    while i < bytes.len() {
        if backslash && bytes[i] == b'\\' {
            i += 2;
        } else if bytes[i] == close {
            if bytes.get(i + 1) == Some(&close) {
                i += 2;
            } else {
                return Some(i + 1);
            }
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn by(column: usize) -> Vec<SortColumn> {
        vec![SortColumn {
            column,
            descending: false,
        }]
    }

    fn by_desc(column: usize) -> Vec<SortColumn> {
        vec![SortColumn {
            column,
            descending: true,
        }]
    }

    #[test]
    fn a_bare_star_sorts_by_name_and_keeps_the_limit_after_the_order() {
        let sorted = sorted_statement(
            "select * from items limit 100;",
            SqlGrammar::Postgres,
            &names(&["id", "price"]),
            &by_desc(1),
        );
        assert_eq!(
            sorted.as_deref(),
            Some("select * from items\nORDER BY \"price\" DESC limit 100")
        );
    }

    #[test]
    fn the_old_order_by_is_replaced_and_inner_ones_are_left_alone() {
        let statement = "select id, row_number() over (order by id) as rn from t order by id limit 5";
        let sorted = sorted_statement(statement, SqlGrammar::Postgres, &names(&["id", "rn"]), &by(1));
        assert_eq!(
            sorted.as_deref(),
            Some("select id, row_number() over (order by id) as rn from t\nORDER BY \"rn\" limit 5")
        );
    }

    #[test]
    fn duplicate_names_expressions_and_qualified_columns_sort_by_position() {
        let join = "select a.id, b.id from a join b on a.id = b.a_id";
        assert_eq!(
            sorted_statement(join, SqlGrammar::Sqlite, &names(&["id", "id"]), &by(1)).as_deref(),
            Some("select a.id, b.id from a join b on a.id = b.a_id\nORDER BY 2")
        );
        let count = "select count(*) from t";
        assert_eq!(
            sorted_statement(count, SqlGrammar::Sqlite, &names(&["count(*)"]), &by(0)).as_deref(),
            Some("select count(*) from t\nORDER BY 1")
        );
        let qualified = "select t.name from t";
        assert_eq!(
            sorted_statement(qualified, SqlGrammar::Postgres, &names(&["name"]), &by(0)).as_deref(),
            Some("select t.name from t\nORDER BY 1")
        );
    }

    #[test]
    fn aliases_with_or_without_as_sort_by_name() {
        let statement = "select price * 2 as doubled, name label from t";
        let sorted = sorted_statement(
            statement,
            SqlGrammar::Postgres,
            &names(&["doubled", "label"]),
            &[
                SortColumn {
                    column: 1,
                    descending: false,
                },
                SortColumn {
                    column: 0,
                    descending: true,
                },
            ],
        );
        assert_eq!(
            sorted.as_deref(),
            Some("select price * 2 as doubled, name label from t\nORDER BY \"label\", \"doubled\" DESC")
        );
    }

    #[test]
    fn sql_server_brackets_names_and_keeps_offset_fetch_and_option() {
        let statement = "SELECT TOP 10 [Name], Price FROM dbo.Products ORDER BY Price OFFSET 0 ROWS FETCH NEXT 5 ROWS ONLY OPTION (RECOMPILE)";
        let sorted = sorted_statement(statement, SqlGrammar::SqlServer, &names(&["Name", "Price"]), &by(0));
        assert_eq!(
            sorted.as_deref(),
            Some(
                "SELECT TOP 10 [Name], Price FROM dbo.Products\nORDER BY [Name] OFFSET 0 ROWS FETCH NEXT 5 ROWS ONLY OPTION (RECOMPILE)"
            )
        );
    }

    #[test]
    fn a_trailing_line_comment_is_left_out_so_it_cannot_swallow_the_order_by() {
        let sorted = sorted_statement(
            "select * from t -- every row",
            SqlGrammar::Postgres,
            &names(&["x"]),
            &by(0),
        );
        assert_eq!(sorted.as_deref(), Some("select * from t\nORDER BY \"x\""));
        // A comment before a tail clause stays, and the new line keeps it from reaching the ORDER BY.
        let sorted = sorted_statement(
            "select * from t -- recent\nlimit 5",
            SqlGrammar::Postgres,
            &names(&["x"]),
            &by(0),
        );
        assert_eq!(
            sorted.as_deref(),
            Some("select * from t -- recent\nORDER BY \"x\" limit 5")
        );
    }

    #[test]
    fn unions_and_ctes_sort_their_whole_result() {
        let union = "select a from x union all select a from y";
        assert_eq!(
            sorted_statement(union, SqlGrammar::Postgres, &names(&["a"]), &by(0)).as_deref(),
            Some("select a from x union all select a from y\nORDER BY \"a\"")
        );
        let cte = "with recent as (select * from orders order by order_date desc limit 10) select order_id, total from recent";
        assert_eq!(
            sorted_statement(cte, SqlGrammar::Postgres, &names(&["order_id", "total"]), &by_desc(1)).as_deref(),
            Some(
                "with recent as (select * from orders order by order_date desc limit 10) select order_id, total from recent\nORDER BY \"total\" DESC"
            )
        );
    }

    #[test]
    fn statements_that_cannot_safely_run_again_are_refused() {
        let columns = names(&["x"]);
        for statement in [
            "insert into t values (1) returning x",
            "with gone as (delete from t returning x) select x from gone",
            "select x into t2 from t",
            "select x from t for update",
            "SELECT x FROM t FOR JSON PATH",
            "exec sp_who",
            "explain select x from t",
            "show tables",
            "select 1; select 2",
            "select 'unclosed",
            "update t set x = 1",
        ] {
            assert_eq!(
                sorted_statement(statement, SqlGrammar::Postgres, &columns, &by(0)),
                None,
                "{statement}"
            );
        }
        assert_eq!(
            sorted_statement("select x from t", SqlGrammar::Postgres, &columns, &[]),
            None
        );
        assert_eq!(
            sorted_statement("select x from t", SqlGrammar::Postgres, &columns, &by(3)),
            None
        );
    }

    #[test]
    fn quoted_names_and_vietnamese_text_survive() {
        let statement = "select \"Tên khách\", 'Việt; Nam' as \"ghi chú\" from khach";
        let sorted = sorted_statement(
            statement,
            SqlGrammar::Postgres,
            &names(&["Tên khách", "ghi chú"]),
            &by(0),
        );
        assert_eq!(
            sorted.as_deref(),
            Some("select \"Tên khách\", 'Việt; Nam' as \"ghi chú\" from khach\nORDER BY \"Tên khách\"")
        );
    }
}
