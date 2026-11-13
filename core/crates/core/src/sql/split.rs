//! Splitting a script into the statements, or SQL Server batches, a run sends.
//!
//! | Grammar | Splits at | Kept whole across `;` |
//! |---|---|---|
//! | PostgreSQL | `;` | `'...'`, `E'...'` with backslash escapes, `"..."`, `$tag$...$tag$`, nested `/* */`, a `BEGIN ATOMIC ... END` body of `CREATE FUNCTION` or `PROCEDURE` |
//! | SQL Server | a `GO` line | the whole batch, `[...]` names, nested `/* */` |
//! | SQLite | `;` | `[...]` and backtick names, a `CREATE TRIGGER ... BEGIN ... END` body |
//!
//! In the PostgreSQL grammar, a line that starts with `\` where a statement
//! would start is a psql meta-command (`\c demo`): it runs to the end of its
//! line and comes back as a [`StatementKind::Directive`].
//!
//! ## A blank line
//!
//! In the PostgreSQL and SQLite grammars a blank line also ends a statement,
//! when the next line starts a new one
//! (`docs/decisions/0021-blank-line-ends-a-statement.md`). No client does
//! this; it is for scripts written without `;`. When unsure it does not split.
//!
//! | After the blank line | The statement before it | Splits |
//! |---|---|---|
//! | a word of `STARTERS` (`SELECT`, `INSERT`, `NOTIFY`, ...), or a `\` directive | ends in a name, a value, `)`, `]` or `*` | yes |
//! | anything else (`FROM`, `SET`, `UNION`, a name, `(`) | any | no |
//! | any | inside `(...)` or a `BEGIN ... END` body | no |
//! | any | ends in `,`, an operator, `(`, or a word of `CONTINUES` (`AS`, `UNION`, `FROM`, `DO`, ...) | no |
//! | a query word (`SELECT`, `VALUES`, `WITH`, `INSERT`, `UPDATE`, `DELETE`, `MERGE`) | a `WITH`, `INSERT`, `REPLACE` or `EXPLAIN` that has not reached its own query | no |
//! | `ALTER` or `DROP`, then `COLUMN` or `CONSTRAINT` | an `ALTER` | no |
//! | `BEGIN` | a `CREATE FUNCTION`, `PROCEDURE` or `TRIGGER` | no |
//!
//! A SQL Server batch is sent whole; `batch.rs` reads the T-SQL statements
//! inside it, to tell which one made each result.
//!
//! A statement holding only whitespace and comments is dropped. Ranges are
//! byte offsets into the text and start at the first token, so a comment
//! before a statement is not sent with it.

use serde::Serialize;

use super::SqlGrammar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StatementKind {
    /// Text the engine runs.
    Sql,
    /// A client command the run handles itself, such as psql's `\c name`.
    Directive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StatementSpan {
    pub start: usize,
    pub end: usize,
    pub kind: StatementKind,
}

impl StatementSpan {
    pub fn text<'a>(&self, script: &'a str) -> &'a str {
        script.get(self.start..self.end).unwrap_or_default()
    }
}

pub fn split(text: &str, grammar: SqlGrammar) -> Vec<StatementSpan> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let nested_comments = matches!(grammar, SqlGrammar::Postgres | SqlGrammar::SqlServer);
    let mut spans = Vec::new();
    let mut current = Current::default();
    let mut i = 0;
    // Nothing but spaces since the line began: a line that ends so is blank.
    let mut line_empty = true;
    let blank_lines = grammar != SqlGrammar::SqlServer;

    while i < len {
        if grammar == SqlGrammar::SqlServer
            && (i == 0 || bytes[i - 1] == b'\n')
            && let Some(next) = go_line(bytes, i)
        {
            current.finish(&mut spans);
            i = next;
            continue;
        }

        if grammar == SqlGrammar::Postgres
            && bytes[i] == b'\\'
            && (current.start.is_none() || current.ends_at_blank_line())
            && starts_line(bytes, i)
        {
            current.finish(&mut spans);
            let line_end = bytes[i..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(len, |offset| i + offset);
            let end = i + text.get(i..line_end).unwrap_or_default().trim_end().len();
            spans.push(StatementSpan {
                start: i,
                end,
                kind: StatementKind::Directive,
            });
            i = line_end;
            line_empty = false;
            continue;
        }

        let byte = bytes[i];
        if !byte.is_ascii_whitespace() {
            line_empty = false;
        }
        match byte {
            b'-' if peek(bytes, i + 1) == b'-' => {
                i = skip_line(bytes, i);
                // The comment took its line's end with it.
                line_empty = true;
            }
            b'/' if peek(bytes, i + 1) == b'*' => i = skip_block_comment(bytes, i, nested_comments),
            b'\'' => {
                let backslash = grammar == SqlGrammar::Postgres && is_escape_prefix(bytes, i);
                current.token(i, skip_quoted(bytes, i, b'\'', backslash));
                i = current.last_end;
            }
            b'"' => {
                current.token(i, skip_quoted(bytes, i, b'"', false));
                i = current.last_end;
            }
            b'[' if grammar != SqlGrammar::Postgres => {
                current.token(i, skip_quoted(bytes, i, b']', false));
                i = current.last_end;
            }
            b'`' if grammar == SqlGrammar::Sqlite => {
                current.token(i, skip_quoted(bytes, i, b'`', false));
                i = current.last_end;
            }
            b'$' if grammar == SqlGrammar::Postgres => {
                let end = dollar_quote(bytes, i).unwrap_or(i + 1);
                current.token(i, end);
                i = end;
            }
            b';' if grammar != SqlGrammar::SqlServer && current.block_depth == 0 => {
                current.finish(&mut spans);
                i += 1;
            }
            byte if byte.is_ascii_whitespace() => {
                if byte == b'\n' {
                    if line_empty && blank_lines && current.start.is_some() {
                        current.blank_line = true;
                    }
                    line_empty = true;
                }
                i += 1;
            }
            byte if is_ident_start(byte) => {
                let start = i;
                while i < len && is_ident(bytes[i]) {
                    i += 1;
                }
                // Words end on a char boundary: every byte of a multi-byte
                // char is an identifier byte.
                let word = text.get(start..i).unwrap_or_default();
                if current.ends_at_blank_line() && current.starts_statement(word, next_word(text, i), grammar) {
                    current.finish(&mut spans);
                }
                current.token(start, i);
                current.word(word, grammar);
            }
            _ => {
                current.token(i, i + 1);
                current.punct(byte);
                i += 1;
            }
        }
    }
    current.finish(&mut spans);
    spans
}

/// The statement a cursor at byte `offset` runs: the one it is inside, else
/// the last one before it, else the first.
pub fn statement_at(spans: &[StatementSpan], offset: usize) -> Option<usize> {
    if let Some(index) = spans.iter().position(|span| span.start <= offset && offset <= span.end) {
        return Some(index);
    }
    spans
        .iter()
        .rposition(|span| span.end <= offset)
        .or(if spans.is_empty() { None } else { Some(0) })
}

/// Words that begin a statement and nothing else, so one after a blank line
/// is a new statement. A word that also continues one is left out, and a
/// statement that starts with it still needs its `;`: `SET` (`UPDATE t SET`),
/// `START` (`START WITH`), `RESET`, `FETCH` (`FETCH FIRST`), `EXECUTE`
/// (`EXECUTE FUNCTION`), `DO` (`ON CONFLICT DO`), `TABLE`, `END`, `CLUSTER`,
/// `SECURITY`.
const STARTERS: &[&str] = &[
    "SELECT",
    "WITH",
    "INSERT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "VALUES",
    "CREATE",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "GRANT",
    "REVOKE",
    "COMMENT",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "ABORT",
    "SAVEPOINT",
    "RELEASE",
    "SHOW",
    "EXPLAIN",
    "ANALYZE",
    "VACUUM",
    "REINDEX",
    "COPY",
    "LISTEN",
    "UNLISTEN",
    "NOTIFY",
    "CALL",
    "LOCK",
    "REFRESH",
    "DECLARE",
    "MOVE",
    "CLOSE",
    "PREPARE",
    "DEALLOCATE",
    "DISCARD",
    "CHECKPOINT",
    "IMPORT",
    "REASSIGN",
];

/// Statement words of SQLite only: in PostgreSQL `ATTACH` and `DETACH` are
/// actions of `ALTER TABLE`, and `replace` is a function.
const SQLITE_STARTERS: &[&str] = &["PRAGMA", "ATTACH", "DETACH", "REPLACE"];

/// Words a statement cannot end with: what follows one, blank line or not,
/// belongs to the same statement.
const CONTINUES: &[&str] = &[
    "AS",
    "UNION",
    "ALL",
    "DISTINCT",
    "EXCEPT",
    "INTERSECT",
    "AND",
    "OR",
    "NOT",
    "ON",
    "IN",
    "EXISTS",
    "ANY",
    "SOME",
    "FROM",
    "WHERE",
    "JOIN",
    "BY",
    "HAVING",
    "THEN",
    "ELSE",
    "WHEN",
    "CASE",
    "DO",
    "INSTEAD",
    "ALSO",
    "FOR",
    "TO",
    "EXPLAIN",
    "WITH",
    "RECURSIVE",
    "LATERAL",
    "USING",
    "SET",
    "INTO",
    "SELECT",
    "LIKE",
    "ILIKE",
    "BETWEEN",
    "IS",
    "OF",
    "AFTER",
    "BEFORE",
    "KEY",
    "GRANT",
    "REVOKE",
    "ADD",
    "RETURNS",
    "ONLY",
    "LIMIT",
    "OFFSET",
];

/// The words a statement's own query starts with: after a `WITH` list, an
/// `INSERT`'s target or an `EXPLAIN`'s options, one of these is still part of it.
const QUERY_WORDS: &[&str] = &["SELECT", "VALUES", "WITH", "INSERT", "UPDATE", "DELETE", "MERGE"];

fn listed(list: &[&str], word: &str) -> bool {
    list.iter().any(|entry| word.eq_ignore_ascii_case(entry))
}

/// The word that follows `from`, past spaces and line ends; empty when
/// something else comes first.
fn next_word(text: &str, from: usize) -> &str {
    let rest = text.get(from..).unwrap_or_default().trim_start();
    let end = rest.bytes().position(|byte| !is_ident(byte)).unwrap_or(rest.len());
    rest.get(..end).unwrap_or_default()
}

/// What the last token of a statement was, for the blank-line rule.
#[derive(Default)]
enum Last {
    #[default]
    Nothing,
    /// A word, upper case.
    Word(String),
    Punct(u8),
    /// A quoted name, a string or a dollar-quoted body.
    Quoted,
}

#[derive(Default)]
struct Current {
    start: Option<usize>,
    last_end: usize,
    /// The first words, upper case, which say whether a block body follows.
    words: Vec<String>,
    previous_word: String,
    block_mode: bool,
    block_depth: u32,
    /// Parentheses open now.
    paren_depth: u32,
    /// A blank line came after the last token.
    blank_line: bool,
    last: Last,
    /// A query word came after the first word, outside parentheses: a
    /// `WITH`, `INSERT` or `EXPLAIN` has reached its own query.
    query_started: bool,
}

impl Current {
    fn token(&mut self, start: usize, end: usize) {
        if self.start.is_none() {
            self.start = Some(start);
        }
        self.last_end = end;
        self.blank_line = false;
        self.last = Last::Quoted;
    }

    fn punct(&mut self, byte: u8) {
        match byte {
            b'(' => self.paren_depth += 1,
            b')' => self.paren_depth = self.paren_depth.saturating_sub(1),
            _ => {}
        }
        self.last = Last::Punct(byte);
    }

    /// A blank line came after a statement that could end there: nothing is
    /// open, and its last token asks for nothing more.
    fn ends_at_blank_line(&self) -> bool {
        if !self.blank_line || self.start.is_none() || self.block_depth > 0 || self.paren_depth > 0 {
            return false;
        }
        match &self.last {
            Last::Nothing => false,
            Last::Quoted => true,
            Last::Word(word) => !listed(CONTINUES, word),
            // A digit ends a number; `)`, `]`, `*` and `?` end an expression.
            // Any other character is an operator or a comma with more to come.
            Last::Punct(byte) => byte.is_ascii_alphanumeric() || matches!(byte, b')' | b']' | b'*' | b'?'),
        }
    }

    /// Whether `word`, the first after a blank line, begins a new statement
    /// (the table in the module docs). `following` is the word after it.
    fn starts_statement(&self, word: &str, following: &str, grammar: SqlGrammar) -> bool {
        let starter = listed(STARTERS, word) || (grammar == SqlGrammar::Sqlite && listed(SQLITE_STARTERS, word));
        if !starter {
            return false;
        }
        let first = self.words.first().map_or("", String::as_str);
        let awaits_query = matches!(first, "WITH" | "INSERT" | "REPLACE" | "EXPLAIN") && !self.query_started;
        if awaits_query && listed(QUERY_WORDS, word) {
            return false;
        }
        let action = word.eq_ignore_ascii_case("ALTER") || word.eq_ignore_ascii_case("DROP");
        if first == "ALTER" && action && listed(&["COLUMN", "CONSTRAINT"], following) {
            return false;
        }
        !(self.block_mode && word.eq_ignore_ascii_case("BEGIN"))
    }

    fn word(&mut self, word: &str, grammar: SqlGrammar) {
        let upper = word.to_ascii_uppercase();
        // The first word is the statement's own; a query word after it, outside
        // parentheses, is where its query starts.
        if !self.words.is_empty() && self.paren_depth == 0 && upper != "WITH" && listed(QUERY_WORDS, &upper) {
            self.query_started = true;
        }
        self.last = Last::Word(upper.clone());
        if self.words.len() < 5 {
            self.words.push(upper.clone());
            self.block_mode = has_block_body(&self.words, grammar);
        }
        if self.block_mode {
            match (grammar, upper.as_str()) {
                (SqlGrammar::Sqlite, "BEGIN" | "CASE") => self.block_depth += 1,
                (SqlGrammar::Postgres, "ATOMIC") if self.previous_word == "BEGIN" => self.block_depth += 1,
                (SqlGrammar::Postgres, "CASE") if self.block_depth > 0 => self.block_depth += 1,
                (SqlGrammar::Sqlite | SqlGrammar::Postgres, "END") => {
                    self.block_depth = self.block_depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        self.previous_word = upper;
    }

    fn finish(&mut self, spans: &mut Vec<StatementSpan>) {
        if let Some(start) = self.start {
            spans.push(StatementSpan {
                start,
                end: self.last_end,
                kind: StatementKind::Sql,
            });
        }
        *self = Self::default();
    }
}

fn has_block_body(words: &[String], grammar: SqlGrammar) -> bool {
    let word = |index: usize| words.get(index).map_or("", String::as_str);
    if word(0) != "CREATE" {
        return false;
    }
    match grammar {
        SqlGrammar::Sqlite => word(1) == "TRIGGER" || (matches!(word(1), "TEMP" | "TEMPORARY") && word(2) == "TRIGGER"),
        SqlGrammar::Postgres => words.iter().any(|w| w == "FUNCTION" || w == "PROCEDURE"),
        SqlGrammar::SqlServer => false,
    }
}

/// Whether only spaces and tabs come before `index` on its line.
fn starts_line(bytes: &[u8], index: usize) -> bool {
    bytes[..index]
        .iter()
        .rev()
        .take_while(|&&b| b != b'\n')
        .all(|&b| matches!(b, b' ' | b'\t' | b'\r'))
}

fn peek(bytes: &[u8], index: usize) -> u8 {
    bytes.get(index).copied().unwrap_or(0)
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn skip_line(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |offset| start + offset + 1)
}

fn skip_block_comment(bytes: &[u8], start: usize, nested: bool) -> usize {
    let mut depth = 0u32;
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'/' && peek(bytes, i + 1) == b'*' {
            if depth == 0 || nested {
                depth += 1;
            }
            i += 2;
        } else if bytes[i] == b'*' && peek(bytes, i + 1) == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// The end of a quoted run that opened at `start`, past its closing quote.
/// A doubled closing quote stays inside. An unclosed run ends the text.
fn skip_quoted(bytes: &[u8], start: usize, close: u8, backslash: bool) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        let byte = bytes[i];
        if backslash && byte == b'\\' {
            i += 2;
        } else if byte == close {
            if peek(bytes, i + 1) == close {
                i += 2;
            } else {
                return i + 1;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// Whether the `'` at `quote` opens a PostgreSQL `E'...'` string.
fn is_escape_prefix(bytes: &[u8], quote: usize) -> bool {
    quote >= 1 && matches!(bytes[quote - 1], b'E' | b'e') && (quote < 2 || !is_ident(bytes[quote - 2]))
}

/// The end of a `$tag$...$tag$` string that opens at `start`, or `None` when
/// the `$` opens no quote (a `$1` parameter, or a `$` inside a name).
fn dollar_quote(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && is_ident(bytes[start - 1]) {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80) {
        if i == start + 1 && bytes[i].is_ascii_digit() {
            return None;
        }
        i += 1;
    }
    if peek(bytes, i) != b'$' {
        return None;
    }
    let tag = &bytes[start..=i];
    let body = i + 1;
    let close = bytes[body..].windows(tag.len()).position(|window| window == tag);
    Some(close.map_or(bytes.len(), |offset| body + offset + tag.len()))
}

/// When the line at `start` is a `GO` batch separator, the start of the next
/// line. `GO` may carry a repeat count and a trailing comment; the count is
/// read as one run, since repeating a batch is a scripting tool's feature.
fn go_line(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    if !(matches!(peek(bytes, i), b'G' | b'g') && matches!(peek(bytes, i + 1), b'O' | b'o')) {
        return None;
    }
    i += 2;
    if i < bytes.len() && is_ident(bytes[i]) {
        return None;
    }
    while i < bytes.len() && bytes[i] != b'\n' {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'0'..=b'9' => i += 1,
            b'-' if peek(bytes, i + 1) == b'-' => return Some(skip_line(bytes, i)),
            _ => return None,
        }
    }
    Some((i + 1).min(bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(script: &str, grammar: SqlGrammar) -> Vec<&str> {
        split(script, grammar).iter().map(|span| span.text(script)).collect()
    }

    #[test]
    fn splits_at_semicolons_and_drops_empty_statements() {
        assert_eq!(
            texts("select 1; select 2;", SqlGrammar::Postgres),
            ["select 1", "select 2"]
        );
        assert!(texts(" ;; ; -- only a comment", SqlGrammar::Postgres).is_empty());
    }

    #[test]
    fn quotes_and_comments_keep_their_semicolons() {
        assert_eq!(
            texts("select ';'; select 'it''s;' ; select \"a;b\"", SqlGrammar::Sqlite),
            ["select ';'", "select 'it''s;'", "select \"a;b\""]
        );
        assert_eq!(
            texts("-- a;b\nselect 1; /* ; */ select 2", SqlGrammar::Postgres),
            ["select 1", "select 2"]
        );
    }

    #[test]
    fn postgres_dollar_quotes_hold_a_function_body() {
        let script = "create function f() returns int as $$ select 1; $$ language sql; select 2";
        assert_eq!(texts(script, SqlGrammar::Postgres).len(), 2);
        let script = "do $fn$ begin perform 1; end $fn$; select 2";
        assert_eq!(
            texts(script, SqlGrammar::Postgres),
            ["do $fn$ begin perform 1; end $fn$", "select 2"]
        );
    }

    #[test]
    fn postgres_parameters_are_not_dollar_quotes() {
        assert_eq!(
            texts("select $1; select 2", SqlGrammar::Postgres),
            ["select $1", "select 2"]
        );
        assert_eq!(
            texts("select a$b; select 2", SqlGrammar::Postgres),
            ["select a$b", "select 2"]
        );
    }

    #[test]
    fn postgres_escape_strings_take_backslashes() {
        assert_eq!(
            texts(r"select E'a\';b'; select 2", SqlGrammar::Postgres),
            [r"select E'a\';b'", "select 2"]
        );
        // A plain string does not, with standard_conforming_strings on.
        assert_eq!(
            texts(r"select 'a\'; select 2", SqlGrammar::Postgres),
            [r"select 'a\'", "select 2"]
        );
    }

    #[test]
    fn postgres_block_comments_nest() {
        assert_eq!(texts("/* a /* b */ ; */ select 1", SqlGrammar::Postgres), ["select 1"]);
    }

    #[test]
    fn postgres_begin_atomic_bodies_stay_whole() {
        let script = "create function f() returns int language sql begin atomic select 1; select case when true then 2 end; end; select 3";
        let statements = texts(script, SqlGrammar::Postgres);
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[1], "select 3");
    }

    #[test]
    fn postgres_transactions_still_split() {
        assert_eq!(
            texts("begin; select 1; end;", SqlGrammar::Postgres),
            ["begin", "select 1", "end"]
        );
    }

    #[test]
    fn sql_server_splits_only_at_go_lines() {
        let script = "select 1; select 2\nGO\nselect 3\n  go 2 -- twice\nselect 4";
        assert_eq!(
            texts(script, SqlGrammar::SqlServer),
            ["select 1; select 2", "select 3", "select 4"]
        );
    }

    #[test]
    fn sql_server_go_inside_a_string_or_name_is_text() {
        assert_eq!(
            texts("select 'a\nGO\nb'\nGO", SqlGrammar::SqlServer),
            ["select 'a\nGO\nb'"]
        );
        assert_eq!(
            texts("select [x;\ny]\ngoto_label:", SqlGrammar::SqlServer),
            ["select [x;\ny]\ngoto_label:"]
        );
        assert_eq!(texts("select [a]]b]\nGO", SqlGrammar::SqlServer), ["select [a]]b]"]);
    }

    #[test]
    fn sqlite_trigger_bodies_stay_whole() {
        let script = "create trigger t after insert on a begin update b set x = case when 1 then 2 end; end; select 1";
        let statements = texts(script, SqlGrammar::Sqlite);
        assert_eq!(statements.len(), 2);
        assert!(statements[0].ends_with("end"));
    }

    #[test]
    fn multi_byte_text_keeps_byte_ranges_on_char_boundaries() {
        assert_eq!(
            texts("select 'Việt;Nam'; select đ", SqlGrammar::Sqlite),
            ["select 'Việt;Nam'", "select đ"]
        );
    }

    #[test]
    fn psql_meta_commands_are_directives_that_end_at_their_line() {
        let script = "create database demo;\n\\c demo  \r\ncreate table t (x int);\n  \\connect other\nselect 1";
        let spans = split(script, SqlGrammar::Postgres);
        let parts: Vec<_> = spans.iter().map(|span| (span.kind, span.text(script))).collect();
        assert_eq!(
            parts,
            [
                (StatementKind::Sql, "create database demo"),
                (StatementKind::Directive, "\\c demo"),
                (StatementKind::Sql, "create table t (x int)"),
                (StatementKind::Directive, "\\connect other"),
                (StatementKind::Sql, "select 1"),
            ]
        );
    }

    #[test]
    fn a_backslash_inside_a_statement_or_another_grammar_is_text() {
        assert_eq!(
            texts("select 1\n\\c demo;", SqlGrammar::Postgres),
            ["select 1\n\\c demo"]
        );
        assert_eq!(texts("\\c demo;", SqlGrammar::Sqlite), ["\\c demo"]);
    }

    #[test]
    fn statement_at_picks_the_one_under_or_before_the_cursor() {
        let script = "select 1;\nselect 2;\n\n";
        let spans = split(script, SqlGrammar::Postgres);
        assert_eq!(statement_at(&spans, 0), Some(0));
        assert_eq!(statement_at(&spans, 12), Some(1));
        assert_eq!(statement_at(&spans, script.len()), Some(1));
        assert_eq!(statement_at(&[], 0), None);
    }

    // --- A blank line (docs/decisions/0021-blank-line-ends-a-statement.md) ----

    const PG: SqlGrammar = SqlGrammar::Postgres;

    #[test]
    fn a_blank_line_before_a_statement_word_ends_the_statement() {
        // The script the decision came from, and the cursor on its second line.
        let script = "select 1\n\nselect * from public.bang_thu";
        assert_eq!(texts(script, PG), ["select 1", "select * from public.bang_thu"]);
        let spans = split(script, PG);
        assert_eq!(statement_at(&spans, script.len() - 3), Some(1));
        assert_eq!(statement_at(&spans, 3), Some(0));

        assert_eq!(
            texts(
                "NOTIFY orders, 'a'\n\nNOTIFY orders, 'b'\n   \t\r\ninsert into t values (1)\n\n\nupdate t set a = 2\n\ndelete from t",
                PG
            ),
            [
                "NOTIFY orders, 'a'",
                "NOTIFY orders, 'b'",
                "insert into t values (1)",
                "update t set a = 2",
                "delete from t"
            ]
        );
        // A statement may end in a name, a string, a number, `)`, `]`, `*` or a quoted name.
        assert_eq!(
            texts(
                "select * from t\n\nselect count(*)\n\nselect a[1]\n\nselect *\n\nselect \"Tên\"\n\ncreate table t (a int)\n\nselect 'đ'",
                PG
            )
            .len(),
            7
        );
        assert_eq!(
            texts(
                "create table t (a int)\n\nPRAGMA table_info(t)\n\nreplace into t values (1)",
                SqlGrammar::Sqlite
            )
            .len(),
            3
        );
    }

    #[test]
    fn a_line_end_alone_or_a_comment_line_is_not_a_blank_line() {
        assert_eq!(texts("select 1\nselect 2", PG), ["select 1\nselect 2"]);
        assert_eq!(
            texts("select 1\n-- note\nselect 2", PG),
            ["select 1\n-- note\nselect 2"]
        );
        // A blank line before or after a comment is one, and the comment is not sent.
        assert_eq!(texts("select 1\n\n-- note\nselect 2", PG), ["select 1", "select 2"]);
        assert_eq!(texts("select 1 -- note\n\nselect 2", PG), ["select 1", "select 2"]);
        assert_eq!(texts("select 1\n-- note\n\nselect 2", PG), ["select 1", "select 2"]);
        // Blank lines before the first statement and after the last change nothing.
        assert_eq!(texts("\n\nselect 1\n\n", PG), ["select 1"]);
    }

    #[test]
    fn a_blank_line_inside_a_statement_does_not_cut_it() {
        for script in [
            // The next line does not begin a statement.
            "select a\n\nfrom t\n\nwhere a > 1\n\norder by a",
            "update t\n\nset a = 1\n\nwhere b = 2",
            "select 1\n\nunion all\n\nselect 2",
            "select a,\n\nb from t",
            "create sequence s\n\nstart with 1",
            "select * from t offset 5\n\nfetch first 5 rows only",
            "create trigger g after insert on t for each row\n\nexecute function f()",
            "insert into t (a) values (1) on conflict (a)\n\ndo nothing",
            // The statement before is waiting for more.
            "create view v as\n\nselect 1",
            "select 1 union\n\nselect 2",
            "select * from t where a =\n\n1",
            "select * from t where exists\n\n(select 1)",
            "insert into t (a) values (1) on conflict (a) do\n\nupdate set a = 2",
            "select * from t for\n\nupdate",
            "explain\n\nselect 1",
            "grant\n\nselect on t to reader",
            // Inside parentheses, a string, a dollar body, a comment.
            "select * from (\n\nselect 1\n\n) s",
            "select 'a\n\nselect 2'",
            "do $$ begin\n\nupdate t set a = 1;\n\nend $$",
            // A statement that has not reached its own query yet.
            "with x as (select 1)\n\nselect * from x",
            "with x as (select 1),\n\ny as (select 2)\n\ninsert into t select * from x",
            "insert into t (a, b)\n\nvalues (1, 2)",
            "insert into t\n\nselect * from s",
            "explain (analyze, buffers)\n\nselect 1",
            "explain analyze\n\nupdate t set a = 1",
            // An ALTER's next action, a function's body.
            "alter table t\n\nalter column a type int,\n\ndrop column b,\n\ndrop constraint c",
            "create function f() returns int language sql\n\nbegin atomic select 1; end",
        ] {
            assert_eq!(texts(script, PG), [script], "{script}");
        }
        // A blank line inside a block comment is the comment's.
        assert_eq!(texts("select 1 /*\n\nselect 2 */", PG), ["select 1"]);
        let trigger = "create trigger g after insert on t\n\nbegin\n\nupdate t set a = 1;\n\ndelete from u;\n\nend";
        assert_eq!(texts(trigger, SqlGrammar::Sqlite), [trigger]);
        assert_eq!(
            texts("insert or\n\nreplace into t values (1)", SqlGrammar::Sqlite).len(),
            1
        );
    }

    #[test]
    fn a_statement_that_reached_its_query_ends_at_the_blank_line() {
        assert_eq!(
            texts("with x as (select 1) select * from x\n\nselect 2", PG),
            ["with x as (select 1) select * from x", "select 2"]
        );
        assert_eq!(
            texts(
                "insert into t select * from s\n\nselect 1\n\ninsert into t values (1)\n\ninsert into t values (2)",
                PG
            )
            .len(),
            4
        );
        assert_eq!(texts("explain select 1\n\nselect 2", PG).len(), 2);
        assert_eq!(
            texts("alter table t add a int\n\nalter table t add b int\n\ndrop table u", PG).len(),
            3
        );
        // A word left out of the list still needs its semicolon.
        assert_eq!(texts("select 1\n\nset search_path = public", PG).len(), 1);
        assert_eq!(texts("select 1;\n\nset search_path = public", PG).len(), 2);
    }

    #[test]
    fn a_directive_after_a_blank_line_is_its_own_and_sql_server_keeps_its_batch() {
        let parts: Vec<_> = split("create database demo\n\n\\c demo\nselect 1", PG)
            .iter()
            .map(|span| span.kind)
            .collect();
        assert_eq!(
            parts,
            [StatementKind::Sql, StatementKind::Directive, StatementKind::Sql]
        );
        // Without a blank line the backslash is still text, as psql reads it.
        assert_eq!(texts("select 1\n\\c demo", PG).len(), 1);
        let batch = "declare @x int = 1\n\nselect @x\n\nselect 2";
        assert_eq!(texts(batch, SqlGrammar::SqlServer), [batch]);
    }
}
