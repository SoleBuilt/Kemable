//! The statements inside one SQL Server batch, to tell which result came from
//! which statement.
//!
//! SQL Server runs a batch (the text between two `GO` lines, `split.rs`) as
//! one request, as sqlcmd and SSMS send it, and its result sets come back in
//! the order its statements made them. Nothing in the reply says which
//! statement made a set, so this reads the batch: it finds where each
//! statement starts (T-SQL needs no `;` between them) and how many sets each
//! one makes. Only a batch whose every statement makes a known number of sets
//! (none, or one) gets an answer; anything else (`EXEC`, `IF`, `BEGIN ... END`,
//! `OUTPUT`, `MERGE`, `DBCC`, ...) gives `None`, and the caller treats the
//! batch as one statement, as SSMS does.
//!
//! A statement starts at a word that begins T-SQL statements, at the top level
//! of the batch, unless the statement before needs it: the `SELECT` of
//! `INSERT ... SELECT`, of a `UNION`, after `FOR` or `AS`; the main verb after
//! a `WITH` list; the `SET` of an `UPDATE`.

use std::ops::Range;

use super::SqlGrammar;
use super::lex::{Kind, Token, lex_strict};

/// Words that begin a statement in T-SQL.
const STARTERS: &[&str] = &[
    "ALTER",
    "BACKUP",
    "BEGIN",
    "BREAK",
    "BULK",
    "CHECKPOINT",
    "CLOSE",
    "COMMIT",
    "CONTINUE",
    "CREATE",
    "DBCC",
    "DEALLOCATE",
    "DECLARE",
    "DELETE",
    "DENY",
    "DISABLE",
    "DROP",
    "ELSE",
    "ENABLE",
    "END",
    "EXEC",
    "EXECUTE",
    "FETCH",
    "GOTO",
    "GRANT",
    "IF",
    "INSERT",
    "KILL",
    "MERGE",
    "OPEN",
    "PRINT",
    "RAISERROR",
    "READTEXT",
    "RECEIVE",
    "RECONFIGURE",
    "RESTORE",
    "RETURN",
    "REVERT",
    "REVOKE",
    "ROLLBACK",
    "SAVE",
    "SELECT",
    "SEND",
    "SET",
    "SETUSER",
    "SHUTDOWN",
    "THROW",
    "TRUNCATE",
    "UPDATE",
    "UPDATETEXT",
    "USE",
    "WAITFOR",
    "WHILE",
    "WITH",
    "WRITETEXT",
];

/// A starter that belongs to the statement before it when it follows one of
/// these words: `UNION SELECT`, `CURSOR FOR SELECT`, `FOR UPDATE`, `ON DELETE`,
/// `BULK INSERT`, `OFFSET 5 ROWS FETCH NEXT`.
const JOINED: &[(&str, &[&str])] = &[
    ("SELECT", &["UNION", "ALL", "EXCEPT", "INTERSECT", "FOR", "AS"]),
    ("UPDATE", &["FOR", "ON"]),
    ("DELETE", &["ON"]),
    ("INSERT", &["BULK"]),
    ("FETCH", &["ROWS", "ROW"]),
];

/// How many result sets a statement sends back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sets {
    None,
    One,
    Unknown,
}

/// One statement of the batch: its first and last token.
struct Part {
    first: usize,
    last: usize,
}

/// The byte ranges, in `batch`, of the statements that each send back one
/// result set, in order, when every statement's count of sets is known.
pub fn batch_queries(batch: &str) -> Option<Vec<Range<usize>>> {
    let tokens = lex_strict(batch, SqlGrammar::SqlServer)?;
    let mut queries = Vec::new();
    for part in parts(batch, &tokens) {
        let tokens = &tokens[part.first..=part.last];
        match sets(batch, tokens) {
            Sets::None => {}
            Sets::One => {
                let (first, last) = (tokens.first()?, tokens.last()?);
                queries.push(first.start..last.end);
            }
            Sets::Unknown => return None,
        }
    }
    Some(queries)
}

/// How many statements a SQL Server batch holds; `None` when a quote,
/// comment or parenthesis is left open.
pub fn batch_statement_count(batch: &str) -> Option<usize> {
    let tokens = lex_strict(batch, SqlGrammar::SqlServer)?;
    Some(parts(batch, &tokens).len())
}

/// Splits the batch's tokens into statements. A `;` at the top level ends one.
fn parts(text: &str, tokens: &[Token]) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut start: Option<usize> = None;
    // What the statement under way is: its first word, and for a `WITH`, the verb after its list.
    let mut verb = String::new();
    let mut main: Option<String> = None;
    let mut insert_source = false;
    // A module (procedure, function, trigger, view) runs to the end of the batch.
    let mut module = false;
    // Inside CASE ... END, ELSE and END belong to the CASE.
    let mut case_depth = 0u32;
    let word = |token: &Token| text[token.start..token.end].to_ascii_uppercase();

    for (index, token) in tokens.iter().enumerate() {
        if module {
            continue;
        }
        if token.depth == 0 && token.kind == Kind::Punct(';') {
            if let Some(first) = start.take()
                && index > first
            {
                parts.push(Part { first, last: index - 1 });
            }
            continue;
        }
        if token.depth > 0 || token.kind != Kind::Word {
            if start.is_none() {
                start = Some(index);
                verb.clear();
                main = None;
                insert_source = false;
                case_depth = 0;
            }
            continue;
        }
        let upper = word(token);
        if upper == "CASE" {
            case_depth += 1;
        } else if upper == "END" && case_depth > 0 {
            case_depth -= 1;
            continue;
        }
        let starts = start.is_none()
            || (case_depth == 0 && begins(text, tokens, index, &upper, &verb, main.as_deref(), insert_source));
        if !starts {
            if verb == "WITH" && main.is_none() && is_verb(&upper) {
                main = Some(upper.clone());
            }
            if (verb == "INSERT" || main.as_deref() == Some("INSERT"))
                && matches!(upper.as_str(), "SELECT" | "VALUES" | "EXEC" | "EXECUTE" | "DEFAULT")
            {
                insert_source = true;
            }
            continue;
        }
        if let Some(first) = start.take()
            && index > first
        {
            parts.push(Part { first, last: index - 1 });
        }
        start = Some(index);
        main = None;
        insert_source = false;
        case_depth = usize::from(upper == "CASE") as u32;
        if matches!(upper.as_str(), "CREATE" | "ALTER") {
            let rest: Vec<String> = tokens[index + 1..]
                .iter()
                .filter(|token| token.kind == Kind::Word)
                .take(3)
                .map(&word)
                .collect();
            let kind = if rest.first().map(String::as_str) == Some("OR") {
                rest.get(2)
            } else {
                rest.first()
            };
            module = kind
                .is_some_and(|kind| matches!(kind.as_str(), "PROC" | "PROCEDURE" | "FUNCTION" | "TRIGGER" | "VIEW"));
        }
        verb = upper;
    }
    if let Some(first) = start
        && let Some(last) = tokens.len().checked_sub(1)
        && last >= first
    {
        parts.push(Part { first, last });
    }
    parts
}

fn is_verb(upper: &str) -> bool {
    matches!(upper, "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE")
}

/// Whether the word at `index` starts a statement, given the one under way.
fn begins(
    text: &str,
    tokens: &[Token],
    index: usize,
    upper: &str,
    verb: &str,
    main: Option<&str>,
    insert_source: bool,
) -> bool {
    if !STARTERS.contains(&upper) {
        return false;
    }
    // GRANT SELECT, INSERT ON t TO u; MERGE ... THEN UPDATE SET ...
    if matches!(verb, "GRANT" | "REVOKE" | "DENY" | "MERGE") {
        return false;
    }
    let previous = index
        .checked_sub(1)
        .and_then(|before| tokens.get(before))
        .filter(|token| token.depth == 0);
    let joined = JOINED
        .iter()
        .find(|(starter, _)| *starter == upper)
        .is_some_and(|(_, after)| previous.is_some_and(|token| after.iter().any(|word| token.is_word(text, word))));
    if joined {
        return false;
    }
    if verb == "WITH" && main.is_none() && is_verb(upper) {
        return false;
    }
    let inserting = verb == "INSERT" || main == Some("INSERT");
    if inserting && !insert_source && matches!(upper, "SELECT" | "EXEC" | "EXECUTE") {
        return false;
    }
    if upper == "SET" && (matches!(verb, "UPDATE" | "ALTER") || main == Some("UPDATE")) {
        return false;
    }
    // ALTER TABLE t ENABLE TRIGGER, ALTER INDEX i ON t DISABLE.
    if verb == "ALTER" && matches!(upper, "ENABLE" | "DISABLE") {
        return false;
    }
    // A table hint, WITH TIES, WITH ROLLUP: only `WITH name AS (` or `WITH name (` opens a list.
    if upper == "WITH" {
        let next = tokens.get(index + 1);
        let after = tokens.get(index + 2);
        let named = next.is_some_and(|token| matches!(token.kind, Kind::Word | Kind::QuotedName));
        let opens = after.is_some_and(|token| token.is_word(text, "AS") || token.kind == Kind::Punct('('));
        return named && opens;
    }
    true
}

/// How many sets a statement sends back.
fn sets(text: &str, tokens: &[Token]) -> Sets {
    let Some(first) = tokens.first() else { return Sets::None };
    let verb = text[first.start..first.end].to_ascii_uppercase();
    let top = |word: &str| tokens.iter().any(|token| token.depth == 0 && token.is_word(text, word));
    let second = tokens
        .get(1)
        .map(|token| text[token.start..token.end].to_ascii_uppercase());
    match verb.as_str() {
        "SELECT" => select_sets(text, tokens),
        "WITH" => {
            let main = tokens
                .iter()
                .filter(|token| token.depth == 0 && token.kind == Kind::Word)
                .map(|token| text[token.start..token.end].to_ascii_uppercase())
                .find(|word| is_verb(word));
            match main.as_deref() {
                Some("SELECT") => {
                    let at = tokens
                        .iter()
                        .position(|token| token.depth == 0 && token.is_word(text, "SELECT"))
                        .unwrap_or(0);
                    select_sets(text, &tokens[at..])
                }
                Some("INSERT" | "UPDATE" | "DELETE") if !top("OUTPUT") => Sets::None,
                _ => Sets::Unknown,
            }
        }
        "INSERT" | "UPDATE" | "DELETE" if !top("OUTPUT") => Sets::None,
        "SET" => match second.as_deref() {
            Some("SHOWPLAN_ALL" | "SHOWPLAN_TEXT" | "SHOWPLAN_XML") => Sets::Unknown,
            Some("STATISTICS")
                if !tokens
                    .get(2)
                    .is_some_and(|token| token.is_word(text, "IO") || token.is_word(text, "TIME")) =>
            {
                Sets::Unknown
            }
            _ => Sets::None,
        },
        "BEGIN" => match second.as_deref() {
            Some("TRAN" | "TRANSACTION" | "DISTRIBUTED") => Sets::None,
            _ => Sets::Unknown,
        },
        "BULK" | "DECLARE" | "USE" | "PRINT" | "RAISERROR" | "THROW" | "WAITFOR" | "COMMIT" | "ROLLBACK" | "SAVE"
        | "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "GRANT" | "REVOKE" | "DENY" | "DEALLOCATE" | "CLOSE" => Sets::None,
        _ => Sets::Unknown,
    }
}

/// A `SELECT` sends one set, unless it fills a table (`INTO`) or variables
/// (`SELECT @total = ...`).
fn select_sets(text: &str, tokens: &[Token]) -> Sets {
    if tokens
        .iter()
        .any(|token| token.depth == 0 && token.is_word(text, "INTO"))
    {
        return Sets::None;
    }
    // The first select item, past DISTINCT, ALL and TOP (n) [PERCENT] [WITH TIES].
    let mut at = 1;
    while let Some(token) = tokens.get(at) {
        if token.is_word(text, "DISTINCT") || token.is_word(text, "ALL") || token.is_word(text, "PERCENT") {
            at += 1;
        } else if token.is_word(text, "TOP") {
            at += 1;
            if tokens.get(at).is_some_and(|token| token.kind == Kind::Punct('(')) {
                while tokens
                    .get(at)
                    .is_some_and(|token| token.depth > 0 || token.kind == Kind::Punct('('))
                {
                    at += 1;
                }
                // The closing parenthesis sits at the outer depth.
                at += 1;
            } else {
                at += 1;
            }
        } else if token.is_word(text, "WITH") && tokens.get(at + 1).is_some_and(|next| next.is_word(text, "TIES")) {
            at += 2;
        } else {
            break;
        }
    }
    let variable = tokens
        .get(at)
        .is_some_and(|token| token.kind == Kind::Word && text[token.start..token.end].starts_with('@'));
    let assigns = match (tokens.get(at + 1), tokens.get(at + 2)) {
        (Some(next), _) if next.kind == Kind::Punct('=') => true,
        (Some(next), Some(after)) => {
            matches!(next.kind, Kind::Punct('+' | '-' | '*' | '/' | '%' | '&' | '|' | '^'))
                && after.kind == Kind::Punct('=')
        }
        _ => false,
    };
    if variable && assigns { Sets::None } else { Sets::One }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queries(batch: &str) -> Option<Vec<&str>> {
        batch_queries(batch).map(|ranges| ranges.into_iter().map(|range| &batch[range]).collect())
    }

    #[test]
    fn two_selects_without_a_semicolon_are_two_statements_like_ssms_runs_them() {
        let batch = "select * from dbo.[CRONUS International Ltd_$Customer$ext]\n\nselect * from dbo.[CRONUS International Ltd_$Customer]";
        assert_eq!(
            queries(batch).unwrap(),
            [
                "select * from dbo.[CRONUS International Ltd_$Customer$ext]",
                "select * from dbo.[CRONUS International Ltd_$Customer]"
            ]
        );
        assert_eq!(batch_statement_count(batch), Some(2));
    }

    #[test]
    fn statements_that_send_no_set_are_skipped_and_semicolons_end_statements() {
        let batch = "SET NOCOUNT ON;\nDECLARE @n int = 5\nSELECT @n AS n;\nUPDATE t SET x = 1 WHERE id = @n\nSELECT TOP (10) * FROM t ORDER BY x";
        assert_eq!(
            queries(batch).unwrap(),
            ["SELECT @n AS n", "SELECT TOP (10) * FROM t ORDER BY x"]
        );
    }

    #[test]
    fn a_select_inside_another_statement_stays_with_it() {
        let batch = "INSERT INTO t (a) SELECT a FROM s\n\
                     SELECT a FROM t UNION ALL SELECT a FROM s\n\
                     WITH c AS (SELECT 1 AS n) SELECT n FROM c\n\
                     DECLARE k CURSOR FOR SELECT a FROM t\n\
                     SELECT * FROM t WITH (NOLOCK) WHERE a IN (SELECT a FROM s)";
        assert_eq!(
            queries(batch).unwrap(),
            [
                "SELECT a FROM t UNION ALL SELECT a FROM s",
                "WITH c AS (SELECT 1 AS n) SELECT n FROM c",
                "SELECT * FROM t WITH (NOLOCK) WHERE a IN (SELECT a FROM s)"
            ]
        );
    }

    #[test]
    fn a_select_into_a_table_or_into_variables_sends_no_set() {
        let batch = "SELECT * INTO #copy FROM t\nSELECT @total = SUM(x), @count += 1 FROM t\nSELECT DISTINCT TOP 5 a FROM #copy";
        assert_eq!(queries(batch).unwrap(), ["SELECT DISTINCT TOP 5 a FROM #copy"]);
    }

    #[test]
    fn words_that_only_look_like_a_new_statement_stay_with_theirs() {
        let batch = "SET ANSI_NULLS ON\nSELECT 1 AS one\n\
                     SELECT a FROM t ORDER BY a OFFSET 5 ROWS FETCH NEXT 5 ROWS ONLY\n\
                     SET IDENTITY_INSERT t ON\nINSERT INTO t (id) VALUES (1)\n\
                     BULK INSERT t FROM 'data.csv'\n\
                     ALTER TABLE t ENABLE TRIGGER all_rows\n\
                     SELECT 2 AS two";
        assert_eq!(
            queries(batch).unwrap(),
            [
                "SELECT 1 AS one",
                "SELECT a FROM t ORDER BY a OFFSET 5 ROWS FETCH NEXT 5 ROWS ONLY",
                "SELECT 2 AS two"
            ]
        );
        assert_eq!(batch_statement_count(batch), Some(8));
    }

    #[test]
    fn a_case_end_does_not_end_the_statement() {
        let batch = "SELECT CASE WHEN a = 1 THEN 'one' ELSE 'other' END AS word FROM t\nSELECT 2";
        assert_eq!(
            queries(batch).unwrap(),
            [
                "SELECT CASE WHEN a = 1 THEN 'one' ELSE 'other' END AS word FROM t",
                "SELECT 2"
            ]
        );
    }

    #[test]
    fn a_batch_whose_sets_cannot_be_known_gives_no_answer() {
        for batch in [
            "EXEC sp_who\nSELECT 1",
            "IF 1 = 1 SELECT 1 ELSE SELECT 2",
            "BEGIN SELECT 1 END",
            "DELETE FROM t OUTPUT deleted.* WHERE a = 1",
            "MERGE t USING s ON t.a = s.a WHEN MATCHED THEN UPDATE SET b = 1;",
            "DBCC CHECKDB",
            "SET SHOWPLAN_XML ON",
            "SELECT 'not closed",
        ] {
            assert_eq!(batch_queries(batch), None, "{batch}");
        }
    }

    #[test]
    fn a_procedure_runs_to_the_end_of_the_batch() {
        let batch = "CREATE OR ALTER PROCEDURE p AS\nSELECT 1\nSELECT 2";
        assert_eq!(batch_statement_count(batch), Some(1));
        assert_eq!(queries(batch).unwrap(), Vec::<&str>::new());
    }

    #[test]
    fn transactions_and_update_set_are_read_as_one_statement_each() {
        let batch = "BEGIN TRAN\nUPDATE t SET a = 1\nSELECT a FROM t\nCOMMIT";
        assert_eq!(batch_statement_count(batch), Some(4));
        assert_eq!(queries(batch).unwrap(), ["SELECT a FROM t"]);
    }

    #[test]
    fn multi_byte_names_never_split_a_statement() {
        let batch = "SELECT N'chữ ký' AS [Tên khách]\nSELECT [Số tiền] FROM [Đơn hàng]";
        assert_eq!(
            queries(batch).unwrap(),
            ["SELECT N'chữ ký' AS [Tên khách]", "SELECT [Số tiền] FROM [Đơn hàng]"]
        );
    }
}
