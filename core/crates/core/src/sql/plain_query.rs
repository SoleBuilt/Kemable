//! Whether a statement is one query that changes nothing, so a driver may
//! stop it on the server once the rows a result keeps are read
//! (`docs/decisions/0004-engines.md`, The row cap).
//!
//! A plain query starts with `SELECT`, `VALUES` or `TABLE`, after any `(` and
//! a `WITH`'s named queries, and holds no word that writes, wherever it
//! stands: `INSERT`, `UPDATE` (but `FOR UPDATE` and `FOR NO KEY UPDATE`,
//! which lock), `DELETE`, `MERGE`, `TRUNCATE`, and `INTO`, since
//! `SELECT ... INTO t` makes a table. It is one statement: a `;` with more
//! after it is a second one.
//!
//! It errs towards no: a column named `update` makes a query that is not
//! plain, which costs a driver only the rows it reads to the end. It reads
//! words, not what a function does: a PostgreSQL function the query calls
//! may still write, so PostgreSQL stops a plain query through a cursor and
//! keeps what the rows read did (`core/crates/drivers/postgres`).

use super::SqlGrammar;
use super::lex::{Kind, lex_strict};

const STARTS: &[&str] = &["SELECT", "VALUES", "TABLE", "WITH"];

const WRITES: &[&str] = &["INSERT", "UPDATE", "DELETE", "MERGE", "TRUNCATE", "INTO"];

/// Whether `statement` is one query that only reads (the module's comment
/// says what counts). `false` when a quote, comment or parenthesis is left
/// open.
pub fn is_plain_query(statement: &str, grammar: SqlGrammar) -> bool {
    let Some(tokens) = lex_strict(statement, grammar) else {
        return false;
    };
    let mut words = tokens.iter().filter(|token| token.kind == Kind::Word);
    let Some(first) = words.next() else {
        return false;
    };
    if !STARTS.iter().any(|start| first.is_word(statement, start)) {
        return false;
    }
    let mut previous: Option<&str> = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Kind::Punct(';') if index + 1 < tokens.len() => return false,
            Kind::Word => {
                let word = &statement[token.start..token.end];
                let locks = word.eq_ignore_ascii_case("UPDATE")
                    && previous
                        .is_some_and(|before| before.eq_ignore_ascii_case("FOR") || before.eq_ignore_ascii_case("KEY"));
                if !locks && WRITES.iter().any(|writes| word.eq_ignore_ascii_case(writes)) {
                    return false;
                }
                previous = Some(word);
            }
            _ => previous = None,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: SqlGrammar = SqlGrammar::Postgres;
    const MS: SqlGrammar = SqlGrammar::SqlServer;

    #[test]
    fn queries_that_only_read_are_plain() {
        for statement in [
            "select * from t",
            "SELECT g FROM generate_series(1, 5000) g;",
            "select pg_notify('orders', g::text) from generate_series(1, 300000) g",
            "  -- the orders\n(select 1) union all (select 2)",
            "values (1), (2)",
            "table orders",
            "with recent as (select * from orders where day > now() - interval '1 day') select * from recent",
            "select * from t for update",
            "select * from t for no key update skip locked",
            "select 'insert into t values (1)', \"delete\" from t",
            "select $$update t set a = 1$$",
        ] {
            assert!(is_plain_query(statement, PG), "{statement}");
        }
        for statement in [
            "select top 10 * from dbo.t",
            "with c as (select 1 as a) select a from c",
            "select [update] from t",
        ] {
            assert!(is_plain_query(statement, MS), "{statement}");
        }
    }

    #[test]
    fn statements_that_write_or_are_not_queries_are_not_plain() {
        for statement in [
            "insert into t (a) select a from s returning *",
            "update t set a = 1 returning *",
            "delete from t returning *",
            "with gone as (delete from t returning *) select * from gone",
            "with gone as (select 1) insert into t select * from gone",
            "select * into t2 from t",
            "select 1; select 2",
            "explain analyze delete from t",
            "show search_path",
            "notify orders, 'x'",
            "call refresh_all()",
            "fetch 10 from c",
            "",
            "-- only a comment",
            "select 'open",
            "select (1",
        ] {
            assert!(!is_plain_query(statement, PG), "{statement}");
        }
        for statement in [
            "insert into t output inserted.* select * from s",
            "with c as (select 1 as a) update t set a = c.a from c",
            "merge t using s on t.id = s.id when matched then update set a = s.a;",
            "exec dbo.report",
            "select * from t\nupdate t set a = 1",
        ] {
            assert!(!is_plain_query(statement, MS), "{statement}");
        }
    }
}
