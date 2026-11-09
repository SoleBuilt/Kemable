//! Console text for an engine that runs commands, not SQL (Redis): one command
//! per line, its words split the way `redis-cli` splits them
//! (`docs/decisions/0008-key-value-engines.md`).
//!
//! | Line | Becomes |
//! |---|---|
//! | empty, or starting with `#` or `//` | nothing |
//! | `SELECT n` | a directive: the run switches to database `n` and sends nothing |
//! | anything else | one command |
//!
//! Words are separated by spaces. `"..."` takes `\"`, `\\`, `\n`, `\r`, `\t`,
//! `\b`, `\a` and `\xHH`; `'...'` takes only `\'`. A closing quote must be
//! followed by a space or the end of the line, as `redis-cli` requires.

use crate::error::DriverError;
use crate::sql::{StatementKind, StatementSpan};

/// The commands in `text`, one per line, as byte ranges without the line's
/// leading and trailing spaces.
pub fn split_lines(text: &str) -> Vec<StatementSpan> {
    let mut spans = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("//") {
            let lead = content.len() - content.trim_start().len();
            let from = start + lead;
            let kind = if select_database(trimmed).is_some() {
                StatementKind::Directive
            } else {
                StatementKind::Sql
            };
            spans.push(StatementSpan {
                start: from,
                end: from + trimmed.len(),
                kind,
            });
        }
        start += line.len();
    }
    spans
}

/// The database a `SELECT n` line switches to.
pub fn select_database(line: &str) -> Option<String> {
    let words = parse_args(line).ok()?;
    match words.as_slice() {
        [command, index] if command.eq_ignore_ascii_case(b"SELECT") => {
            let index = std::str::from_utf8(index).ok()?;
            index.parse::<u32>().ok().map(|index| index.to_string())
        }
        _ => None,
    }
}

/// The words of one command line, as bytes: `\xHH` can write any byte.
pub fn parse_args(line: &str) -> Result<Vec<Vec<u8>>, DriverError> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut i = 0;
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return Ok(words);
        }
        let mut word = Vec::new();
        let mut in_double = false;
        let mut in_single = false;
        loop {
            let Some(&byte) = bytes.get(i) else {
                if in_double || in_single {
                    return Err(DriverError::config("a quote is not closed"));
                }
                break;
            };
            if in_double {
                if byte == b'\\' {
                    if bytes.get(i + 1) == Some(&b'x')
                        && let Some(value) = hex_pair(bytes, i + 2)
                    {
                        word.push(value);
                        i += 4;
                        continue;
                    }
                    if let Some(&next) = bytes.get(i + 1) {
                        word.push(match next {
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            b'b' => 8,
                            b'a' => 7,
                            other => other,
                        });
                        i += 2;
                        continue;
                    }
                }
                match byte {
                    b'"' => {
                        if bytes.get(i + 1).is_some_and(|next| !next.is_ascii_whitespace()) {
                            return Err(DriverError::config("a closing quote must be followed by a space"));
                        }
                        i += 1;
                        break;
                    }
                    other => {
                        word.push(other);
                        i += 1;
                    }
                }
            } else if in_single {
                match byte {
                    b'\\' if bytes.get(i + 1) == Some(&b'\'') => {
                        word.push(b'\'');
                        i += 2;
                    }
                    b'\'' => {
                        if bytes.get(i + 1).is_some_and(|next| !next.is_ascii_whitespace()) {
                            return Err(DriverError::config("a closing quote must be followed by a space"));
                        }
                        i += 1;
                        break;
                    }
                    other => {
                        word.push(other);
                        i += 1;
                    }
                }
            } else {
                match byte {
                    b if b.is_ascii_whitespace() => break,
                    b'"' => in_double = true,
                    b'\'' => in_single = true,
                    other => word.push(other),
                }
                i += 1;
            }
        }
        words.push(word);
    }
}

/// The byte two hex digits at `at` write.
fn hex_pair(bytes: &[u8], at: usize) -> Option<u8> {
    let digit = |offset: usize| {
        bytes
            .get(at + offset)
            .and_then(|byte| char::from(*byte).to_digit(16))
            .and_then(|digit| u8::try_from(digit).ok())
    };
    Some(digit(0)? * 16 + digit(1)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        parse_args(line)
            .unwrap_or_else(|error| panic!("{line}: {error:?}"))
            .into_iter()
            .map(|word| String::from_utf8_lossy(&word).into_owned())
            .collect()
    }

    #[test]
    fn words_split_on_spaces_and_quotes_keep_them_whole() {
        assert_eq!(
            words("  SET  greeting \"hello world\" "),
            ["SET", "greeting", "hello world"]
        );
        assert_eq!(words("SET k 'it\\'s'"), ["SET", "k", "it's"]);
        assert_eq!(words("SET k \"a\\\"b\\\\c\\n\""), ["SET", "k", "a\"b\\c\n"]);
        assert_eq!(words("SET k \"\""), ["SET", "k", ""]);
        assert_eq!(words("SET k xin\u{a0}chào"), ["SET", "k", "xin\u{a0}chào"]);
    }

    #[test]
    fn a_hex_escape_writes_any_byte() {
        let parsed = parse_args("SET k \"\\x00\\xff\\x4A\"").unwrap_or_default();
        assert_eq!(parsed[2], [0x00, 0xff, 0x4a]);
        // Not a hex escape: the x is kept, as redis-cli does.
        assert_eq!(words("ECHO \"\\xZZ\""), ["ECHO", "xZZ"]);
    }

    #[test]
    fn an_open_quote_or_a_word_glued_to_a_closing_one_is_refused() {
        assert!(parse_args("SET k \"open").is_err());
        assert!(parse_args("SET k 'open").is_err());
        assert!(parse_args("SET k \"a\"b").is_err());
    }

    #[test]
    fn lines_become_commands_and_comments_are_dropped() {
        let text = "# setup\r\nSET a 1\n\n   GET a  \n// done\nSELECT 2\nselect x\n";
        let spans = split_lines(text);
        let shown: Vec<(&str, StatementKind)> = spans.iter().map(|span| (span.text(text), span.kind)).collect();
        assert_eq!(
            shown,
            [
                ("SET a 1", StatementKind::Sql),
                ("GET a", StatementKind::Sql),
                ("SELECT 2", StatementKind::Directive),
                ("select x", StatementKind::Sql),
            ]
        );
    }

    #[test]
    fn select_reads_a_database_number_only() {
        assert_eq!(select_database("select 3").as_deref(), Some("3"));
        assert_eq!(select_database("SELECT \"15\"").as_deref(), Some("15"));
        assert_eq!(select_database("SELECT -1"), None);
        assert_eq!(select_database("SELECT 1 2"), None);
        assert_eq!(select_database("GET 1"), None);
    }
}
