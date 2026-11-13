//! Whether a statement may change what the object tree shows, so a UI reads
//! the catalog again only after one that may (`NOTIFY`, `INSERT` or `SET`
//! leave the tree as it was).
//!
//! | Word, outside quotes and comments | Why |
//! |---|---|
//! | `CREATE`, `ALTER`, `DROP` | they make, change or remove objects |
//! | `GRANT`, `REVOKE` | what the login may see changes |
//! | `IMPORT` | PostgreSQL's `IMPORT FOREIGN SCHEMA` makes tables |
//! | `ATTACH`, `DETACH` | SQLite's databases, PostgreSQL's partitions |
//! | `INTO` not after `INSERT` or `MERGE` | `SELECT ... INTO t` makes a table |
//! | `DO`, `CALL`, `EXEC`, `EXECUTE` | a body or procedure Paboable cannot read may do anything |
//!
//! It errs towards yes: a word is enough, wherever it stands, so a function
//! body or a batch that creates something is never missed. A `$$` body is a
//! string to the lexer, which is why `DO` counts by itself.

use super::SqlGrammar;
use super::lex::{Kind, lex};

const CHANGING: &[&str] = &[
    "CREATE", "ALTER", "DROP", "GRANT", "REVOKE", "IMPORT", "ATTACH", "DETACH", "DO", "CALL", "EXEC", "EXECUTE",
];

/// Whether `statement` (one statement, or one SQL Server batch) may change the
/// objects a tree lists.
pub fn may_change_schema(statement: &str, grammar: SqlGrammar) -> bool {
    let tokens = lex(statement, grammar).tokens;
    let mut previous: Option<&str> = None;
    for token in &tokens {
        if token.kind != Kind::Word {
            previous = None;
            continue;
        }
        let word = &statement[token.start..token.end];
        if CHANGING.iter().any(|changing| word.eq_ignore_ascii_case(changing)) {
            return true;
        }
        let inserting = previous
            .is_some_and(|before| before.eq_ignore_ascii_case("INSERT") || before.eq_ignore_ascii_case("MERGE"));
        if word.eq_ignore_ascii_case("INTO") && !inserting {
            return true;
        }
        previous = Some(word);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: SqlGrammar = SqlGrammar::Postgres;

    #[test]
    fn data_and_session_statements_leave_the_tree_as_it_was() {
        for statement in [
            "NOTIFY orders, 'xin chào'",
            "select pg_notify('orders', g::text) from generate_series(1, 5000) g",
            "insert into t (a) values (1)",
            "insert into t select * from s",
            "update t set a = 2 where b = 'create'",
            "delete from t where note = 'drop table x'",
            "set search_path = public",
            "listen orders",
            "-- drop table t\nselect 1",
            "select \"create\" from t",
        ] {
            assert!(!may_change_schema(statement, PG), "{statement}");
        }
        assert!(!may_change_schema(
            "merge into t using s on t.id = s.id when matched then update set a = s.a",
            SqlGrammar::SqlServer
        ));
    }

    #[test]
    fn statements_that_make_change_or_remove_objects_say_so() {
        for statement in [
            "create table t (a int)",
            "CREATE OR REPLACE VIEW v AS SELECT 1",
            "alter table t add column b int",
            "drop table if exists t",
            "grant select on t to reader",
            "revoke all on t from reader",
            "import foreign schema public from server s into local",
            "select a, b into new_table from t",
            "do $$ begin create table x (a int); end $$",
            "call refresh_everything()",
        ] {
            assert!(may_change_schema(statement, PG), "{statement}");
        }
        assert!(may_change_schema(
            "attach database 'other.db' as other",
            SqlGrammar::Sqlite
        ));
        assert!(may_change_schema("exec sp_rename 't', 'u'", SqlGrammar::SqlServer));
        assert!(may_change_schema(
            "insert into log values (1)\nselect 1\ncreate table [x y] (a int)",
            SqlGrammar::SqlServer
        ));
    }
}
