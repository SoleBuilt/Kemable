//! Tokens of one statement, for the readers that need more than the splitter
//! gives: the sort (`sorted.rs`), which refuses anything left open, and the
//! completion context (`completion.rs`), which reads unfinished text up to
//! the cursor. The quoting rules are the splitter's (`split.rs`).

use super::SqlGrammar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Word,
    QuotedName,
    Literal,
    Punct(char),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// Parentheses around the token; a `(` and its `)` sit at the outer depth.
    pub depth: u32,
}

impl Token {
    pub fn is_word(&self, text: &str, word: &str) -> bool {
        self.kind == Kind::Word && text[self.start..self.end].eq_ignore_ascii_case(word)
    }

    /// The name a word or quoted name stands for. A quoted name left open
    /// (tolerant reading) is the text after its quote.
    pub fn identifier(&self, text: &str) -> Option<String> {
        let raw = &text[self.start..self.end];
        match self.kind {
            Kind::Word => Some(raw.to_owned()),
            Kind::QuotedName => {
                let open = raw.chars().next()?;
                let close = match open {
                    '[' => ']',
                    other => other,
                };
                let inner = raw.get(open.len_utf8()..)?;
                let inner = inner.strip_suffix(close).unwrap_or(inner);
                Some(inner.replace(&format!("{close}{close}"), &close.to_string()))
            }
            _ => None,
        }
    }
}

/// What the text ends inside of, when it ends before a quote or comment closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Open {
    /// A `--` comment, which the end of the text closes.
    LineComment,
    BlockComment,
    Literal,
    /// A quoted name; its token is the last one.
    Name,
}

pub(super) struct Lexed {
    pub tokens: Vec<Token>,
    pub open: Option<Open>,
    /// Every `)` had its `(` and every `(` its `)`.
    pub balanced: bool,
}

/// The tokens of a statement, comments dropped, or `None` when a quote,
/// block comment or parenthesis is left open, since nothing sure can be said
/// about it. A `--` comment may run to the end.
pub(super) fn lex_strict(text: &str, grammar: SqlGrammar) -> Option<Vec<Token>> {
    let lexed = lex(text, grammar);
    let open = matches!(lexed.open, Some(Open::Literal | Open::Name | Open::BlockComment));
    (lexed.balanced && !open).then_some(lexed.tokens)
}

/// The tokens of a statement, comments dropped, reading on through anything
/// left open: a quote or comment runs to the end, a `)` with no `(` stays at
/// depth 0.
pub(super) fn lex(text: &str, grammar: SqlGrammar) -> Lexed {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut depth = 0u32;
    let mut balanced = true;
    let mut open = None;
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
                match bytes[i..].iter().position(|&b| b == b'\n') {
                    Some(n) => i += n + 1,
                    None => {
                        i = bytes.len();
                        open = Some(Open::LineComment);
                    }
                }
                continue;
            }
            b'/' if next == b'*' => {
                let mut level = 0;
                loop {
                    if i + 1 >= bytes.len() {
                        i = bytes.len();
                        open = Some(Open::BlockComment);
                        break;
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
                i = quoted(bytes, i, b'\'', backslash).unwrap_or_else(|| {
                    open = Some(Open::Literal);
                    bytes.len()
                });
                Kind::Literal
            }
            b'"' | b'[' | b'`'
                if byte == b'"'
                    || (byte == b'[' && grammar == SqlGrammar::SqlServer)
                    || (byte == b'`' && grammar == SqlGrammar::Sqlite) =>
            {
                let close = if byte == b'[' { b']' } else { byte };
                i = quoted(bytes, i, close, false).unwrap_or_else(|| {
                    open = Some(Open::Name);
                    bytes.len()
                });
                Kind::QuotedName
            }
            b'$' if grammar == SqlGrammar::Postgres && (next == b'$' || next.is_ascii_alphabetic() || next == b'_') => {
                let closed = bytes[i + 1..]
                    .iter()
                    .position(|&b| b == b'$')
                    .map(|n| i + 1 + n)
                    .and_then(|tag_end| {
                        let tag = &bytes[i..=tag_end];
                        let body = tag_end + 1;
                        let close = bytes[body..].windows(tag.len()).position(|w| w == tag)?;
                        Some(body + close + tag.len())
                    });
                i = closed.unwrap_or_else(|| {
                    open = Some(Open::Literal);
                    bytes.len()
                });
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
                match depth.checked_sub(1) {
                    Some(outer) => depth = outer,
                    None => balanced = false,
                }
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
    Lexed {
        tokens,
        open,
        balanced: balanced && depth == 0,
    }
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

    #[test]
    fn the_strict_reading_refuses_what_is_left_open() {
        for text in [
            "select 'a",
            "select \"a",
            "select (1",
            "select 1)",
            "select 1 /* note",
            "select /* a\n -- b",
        ] {
            assert!(lex_strict(text, SqlGrammar::Postgres).is_none(), "{text}");
        }
        assert!(lex_strict("select 1 -- note", SqlGrammar::Postgres).is_some());
    }

    #[test]
    fn the_tolerant_reading_says_what_the_text_ends_in() {
        assert_eq!(lex("select 'a", SqlGrammar::Postgres).open, Some(Open::Literal));
        assert_eq!(lex("select [na", SqlGrammar::SqlServer).open, Some(Open::Name));
        assert_eq!(lex("select 1 -- no", SqlGrammar::Sqlite).open, Some(Open::LineComment));
        assert_eq!(lex("select $$ body", SqlGrammar::Postgres).open, Some(Open::Literal));
        let lexed = lex("select count(", SqlGrammar::Postgres);
        assert_eq!(lexed.open, None);
        assert!(!lexed.balanced);
        assert_eq!(lexed.tokens.len(), 3);
    }

    #[test]
    fn an_open_quoted_name_reads_as_what_was_typed() {
        let text = "select \"Ord";
        let lexed = lex(text, SqlGrammar::Postgres);
        let last = lexed.tokens.last().copied();
        assert_eq!(last.and_then(|token| token.identifier(text)).as_deref(), Some("Ord"));
        let text = "select [a]]b]";
        let lexed = lex(text, SqlGrammar::SqlServer);
        let last = lexed.tokens.last().copied();
        assert_eq!(last.and_then(|token| token.identifier(text)).as_deref(), Some("a]b"));
    }
}
