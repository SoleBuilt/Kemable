//! Statements after which a run continues in another database.
//!
//! | Statement | Grammar | Meaning |
//! |---|---|---|
//! | `CREATE DATABASE name` | all with databases | `Created`: the run may follow it (`docs/roadmap.md`, UX backlog) |
//! | `USE name` | SQL Server | `Used`: the session already switched; the UI follows |
//! | `DROP DATABASE name` | all with databases | `Dropped`: the app first closes its own sessions on it |
//! | `\c name`, `\connect name` | PostgreSQL directive | `Connect`: the run reopens its session there |
//!
//! Names follow each engine's rules: PostgreSQL folds an unquoted name to
//! lower case and keeps a `"quoted"` one as written; SQL Server keeps the
//! name as written, `[bracketed]` or `"quoted"`. psql takes a meta-command's
//! argument literally, unquoted or `"quoted"`.

use super::SqlGrammar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatabaseChange {
    Created(String),
    Used(String),
    Connect(String),
    /// `DROP DATABASE name`: the app closes its own sessions on it first,
    /// since the server refuses to drop a database anyone is connected to.
    Dropped(String),
}

/// What `statement` (one statement, as `split` returns it) does to the
/// session's database, if anything.
pub fn database_change(statement: &str, grammar: SqlGrammar) -> Option<DatabaseChange> {
    let statement = statement.trim();
    if let Some(rest) = statement.strip_prefix('\\') {
        return connect_directive(rest);
    }
    let mut tokens = Tokens {
        text: statement,
        at: 0,
        grammar,
    };
    let first = tokens.next()?;
    if first.is_word("CREATE") {
        if !tokens.next()?.is_word("DATABASE") {
            return None;
        }
        let mut name = tokens.next()?;
        if name.is_word("IF") {
            // CREATE DATABASE IF NOT EXISTS name (MySQL; harmless elsewhere).
            if !(tokens.next()?.is_word("NOT") && tokens.next()?.is_word("EXISTS")) {
                return None;
            }
            name = tokens.next()?;
        }
        return name.identifier(grammar).map(DatabaseChange::Created);
    }
    if first.is_word("DROP") {
        if !tokens.next()?.is_word("DATABASE") {
            return None;
        }
        let mut name = tokens.next()?;
        if name.is_word("IF") {
            if !tokens.next()?.is_word("EXISTS") {
                return None;
            }
            name = tokens.next()?;
        }
        return name.identifier(grammar).map(DatabaseChange::Dropped);
    }
    if first.is_word("USE") && grammar == SqlGrammar::SqlServer {
        return tokens.next()?.identifier(grammar).map(DatabaseChange::Used);
    }
    None
}

/// `c name` or `connect name` after the backslash. Further arguments (user,
/// host, port) would point the session somewhere else, which a run does not do.
fn connect_directive(rest: &str) -> Option<DatabaseChange> {
    let (command, arguments) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if command != "c" && command != "connect" {
        return None;
    }
    let arguments = arguments.trim();
    let name = if let Some(quoted) = arguments.strip_prefix('"') {
        let mut name = String::new();
        let mut chars = quoted.chars().peekable();
        loop {
            match chars.next()? {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    name.push('"');
                }
                '"' => break,
                ch => name.push(ch),
            }
        }
        if chars.any(|ch| !ch.is_whitespace()) {
            return None;
        }
        name
    } else {
        let mut words = arguments.split_whitespace();
        let name = words.next()?.to_owned();
        if words.next().is_some() {
            return None;
        }
        name
    };
    (!name.is_empty() && name != "-").then_some(DatabaseChange::Connect(name))
}

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    Quoted(String),
    Other,
}

impl Token<'_> {
    fn is_word(&self, keyword: &str) -> bool {
        matches!(self, Token::Word(word) if word.eq_ignore_ascii_case(keyword))
    }

    fn identifier(self, grammar: SqlGrammar) -> Option<String> {
        match self {
            Token::Word(word) if grammar == SqlGrammar::Postgres => Some(word.to_ascii_lowercase()),
            Token::Word(word) => Some(word.to_owned()),
            Token::Quoted(name) if !name.is_empty() => Some(name),
            _ => None,
        }
    }
}

/// The few tokens the statements above need: words, quoted names, and
/// anything else as one opaque token. Comments are skipped.
struct Tokens<'a> {
    text: &'a str,
    at: usize,
    grammar: SqlGrammar,
}

impl<'a> Tokens<'a> {
    fn next(&mut self) -> Option<Token<'a>> {
        let rest = self.skip_space_and_comments()?;
        let first = rest.chars().next()?;
        if first.is_alphabetic() || first == '_' {
            let len = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                .unwrap_or(rest.len());
            self.at += len;
            return Some(Token::Word(&rest[..len]));
        }
        let close = match first {
            '"' => Some('"'),
            '[' if self.grammar == SqlGrammar::SqlServer => Some(']'),
            '`' if self.grammar == SqlGrammar::Sqlite => Some('`'),
            _ => None,
        };
        if let Some(close) = close {
            let mut name = String::new();
            let mut chars = rest.char_indices().skip(1).peekable();
            while let Some((index, ch)) = chars.next() {
                if ch == close {
                    if chars.peek().map(|&(_, next)| next) == Some(close) {
                        chars.next();
                        name.push(close);
                        continue;
                    }
                    self.at += index + ch.len_utf8();
                    return Some(Token::Quoted(name));
                }
                name.push(ch);
            }
            return None;
        }
        self.at += first.len_utf8();
        Some(Token::Other)
    }

    fn skip_space_and_comments(&mut self) -> Option<&'a str> {
        loop {
            let rest = self.text.get(self.at..)?;
            let trimmed = rest.trim_start();
            self.at += rest.len() - trimmed.len();
            if let Some(after) = trimmed.strip_prefix("--") {
                self.at += 2 + after.find('\n').map_or(after.len(), |n| n + 1);
            } else if let Some(after) = trimmed.strip_prefix("/*") {
                self.at += 2 + after.find("*/").map_or(after.len(), |n| n + 2);
            } else {
                return (!trimmed.is_empty()).then_some(trimmed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(statement: &str, grammar: SqlGrammar) -> Option<DatabaseChange> {
        database_change(statement, grammar)
    }

    #[test]
    fn postgres_folds_unquoted_names_and_keeps_quoted_ones() {
        assert_eq!(
            change("CREATE DATABASE DEMO_SALES", SqlGrammar::Postgres),
            Some(DatabaseChange::Created("demo_sales".into()))
        );
        assert_eq!(
            change("create database \"Demo \"\"Sales\"\"\"", SqlGrammar::Postgres),
            Some(DatabaseChange::Created("Demo \"Sales\"".into()))
        );
        assert_eq!(
            change(
                "-- new shop\n/* x */ create database shop with owner = me",
                SqlGrammar::Postgres
            ),
            Some(DatabaseChange::Created("shop".into()))
        );
    }

    #[test]
    fn sql_server_keeps_names_and_follows_use() {
        assert_eq!(
            change("CREATE DATABASE [Demo Sales]", SqlGrammar::SqlServer),
            Some(DatabaseChange::Created("Demo Sales".into()))
        );
        assert_eq!(
            change("use Sales", SqlGrammar::SqlServer),
            Some(DatabaseChange::Used("Sales".into()))
        );
        assert_eq!(change("use Sales", SqlGrammar::Postgres), None);
    }

    #[test]
    fn if_not_exists_is_read_past() {
        assert_eq!(
            change("CREATE DATABASE IF NOT EXISTS shop", SqlGrammar::Postgres),
            Some(DatabaseChange::Created("shop".into()))
        );
    }

    #[test]
    fn dropping_a_database_names_it() {
        assert_eq!(
            change("DROP DATABASE IF EXISTS Shop WITH (FORCE)", SqlGrammar::Postgres),
            Some(DatabaseChange::Dropped("shop".into()))
        );
        assert_eq!(
            change("drop database [Old Shop]", SqlGrammar::SqlServer),
            Some(DatabaseChange::Dropped("Old Shop".into()))
        );
    }

    #[test]
    fn other_statements_change_nothing() {
        for statement in [
            "create table database (x int)",
            "select 'create database x'",
            "drop table database",
            "",
        ] {
            assert_eq!(change(statement, SqlGrammar::Postgres), None, "{statement}");
        }
    }

    #[test]
    fn psql_connect_directives_take_their_argument_literally() {
        assert_eq!(
            change("\\c demo_sales", SqlGrammar::Postgres),
            Some(DatabaseChange::Connect("demo_sales".into()))
        );
        assert_eq!(
            change("\\connect DEMO", SqlGrammar::Postgres),
            Some(DatabaseChange::Connect("DEMO".into()))
        );
        assert_eq!(
            change("\\c \"My DB\"", SqlGrammar::Postgres),
            Some(DatabaseChange::Connect("My DB".into()))
        );
        assert_eq!(change("\\c db user host", SqlGrammar::Postgres), None);
        assert_eq!(change("\\c -", SqlGrammar::Postgres), None);
        assert_eq!(change("\\dt", SqlGrammar::Postgres), None);
    }
}
