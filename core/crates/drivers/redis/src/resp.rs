//! RESP2, the protocol Redis speaks by default: a command goes as an array of
//! bulk strings; a reply is a simple string, an error, an integer, a bulk
//! string (or nil) or an array of replies (or nil).
//!
//! The reader caps a bulk string at Redis's own 512 MiB and nesting at 64
//! levels, so a broken or hostile server cannot make it allocate without end.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};

/// Redis's own limit for one string.
const MAX_BULK: i64 = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
/// An array header past this is not a reply this client asked for.
const MAX_ARRAY: i64 = 1 << 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Status(String),
    Error(String),
    Integer(i64),
    Bulk(Vec<u8>),
    Array(Vec<Reply>),
    /// A nil bulk string or nil array.
    Nil,
}

impl Reply {
    /// The reply as text, for the replies that are text or a number.
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Status(text) => Some(text.clone()),
            Self::Bulk(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            Self::Integer(value) => Some(value.to_string()),
            _ => None,
        }
    }

    pub fn integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            Self::Bulk(bytes) => std::str::from_utf8(bytes).ok()?.parse().ok(),
            _ => None,
        }
    }
}

/// A command as the bytes RESP2 sends.
pub fn encode<A: AsRef<[u8]>>(args: &[A]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + args.iter().map(|arg| arg.as_ref().len() + 16).sum::<usize>());
    out.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
    for arg in args {
        let arg = arg.as_ref();
        out.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
        out.extend_from_slice(arg);
        out.extend_from_slice(b"\r\n");
    }
    out
}

#[derive(Debug)]
pub enum ReadError {
    Io(std::io::Error),
    /// The server sent something that is not RESP2.
    Protocol(String),
}

impl From<std::io::Error> for ReadError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Reads one reply.
pub async fn read<R: AsyncBufRead + Unpin + Send>(reader: &mut R) -> Result<Reply, ReadError> {
    read_at(reader, 0).await
}

fn read_at<'a, R: AsyncBufRead + Unpin + Send>(
    reader: &'a mut R,
    depth: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Reply, ReadError>> + Send + 'a>> {
    Box::pin(async move {
        if depth > MAX_DEPTH {
            return Err(ReadError::Protocol("replies nested too deep".into()));
        }
        let line = read_line(reader).await?;
        let (kind, rest) = line
            .split_first()
            .ok_or_else(|| ReadError::Protocol("an empty line".into()))?;
        let text = || String::from_utf8_lossy(rest).into_owned();
        Ok(match kind {
            b'+' => Reply::Status(text()),
            b'-' => Reply::Error(text()),
            b':' => Reply::Integer(number(rest)?),
            b'$' => {
                let length = number(rest)?;
                if length < 0 {
                    return Ok(Reply::Nil);
                }
                if length > MAX_BULK {
                    return Err(ReadError::Protocol(format!("a string of {length} bytes")));
                }
                let length = usize::try_from(length).map_err(|_| ReadError::Protocol("a string too long".into()))?;
                let mut bytes = vec![0; length + 2];
                reader.read_exact(&mut bytes).await?;
                if !bytes.ends_with(b"\r\n") {
                    return Err(ReadError::Protocol("a string not ended by CRLF".into()));
                }
                bytes.truncate(length);
                Reply::Bulk(bytes)
            }
            b'*' => {
                let count = number(rest)?;
                if count < 0 {
                    return Ok(Reply::Nil);
                }
                if count > MAX_ARRAY {
                    return Err(ReadError::Protocol(format!("an array of {count} replies")));
                }
                // Grown as replies arrive, so a huge header alone allocates nothing.
                let mut items = Vec::with_capacity(usize::try_from(count.min(1024)).unwrap_or(0));
                for _ in 0..count {
                    items.push(read_at(reader, depth + 1).await?);
                }
                Reply::Array(items)
            }
            other => {
                return Err(ReadError::Protocol(format!(
                    "a reply starting with {:?}",
                    char::from(*other)
                )));
            }
        })
    })
}

/// One line without its CRLF. A line longer than a status or a header could
/// be is refused.
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, ReadError> {
    let mut line = Vec::new();
    let read = (&mut *reader).take(64 * 1024).read_until(b'\n', &mut line).await?;
    if read == 0 {
        return Err(ReadError::Io(std::io::ErrorKind::UnexpectedEof.into()));
    }
    if !line.ends_with(b"\r\n") {
        return Err(ReadError::Protocol("a line not ended by CRLF".into()));
    }
    line.truncate(line.len() - 2);
    Ok(line)
}

fn number(text: &[u8]) -> Result<i64, ReadError> {
    std::str::from_utf8(text)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| ReadError::Protocol(format!("{:?} is not a number", String::from_utf8_lossy(text))))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn parse(bytes: &[u8]) -> Result<Reply, ReadError> {
        let mut reader = tokio::io::BufReader::new(bytes);
        read(&mut reader).await
    }

    #[test]
    fn a_command_is_an_array_of_bulk_strings() {
        assert_eq!(
            encode(&["SET", "k", "xin chào"]),
            b"*3\r\n$3\r\nSET\r\n$1\r\nk\r\n$9\r\nxin ch\xc3\xa0o\r\n".to_vec()
        );
        assert_eq!(encode::<&str>(&[]), b"*0\r\n".to_vec());
    }

    #[tokio::test]
    async fn every_reply_kind_reads() {
        assert_eq!(parse(b"+OK\r\n").await.ok(), Some(Reply::Status("OK".into())));
        assert_eq!(
            parse(b"-WRONGTYPE no\r\n").await.ok(),
            Some(Reply::Error("WRONGTYPE no".into()))
        );
        assert_eq!(parse(b":-42\r\n").await.ok(), Some(Reply::Integer(-42)));
        assert_eq!(parse(b"$-1\r\n").await.ok(), Some(Reply::Nil));
        assert_eq!(parse(b"$0\r\n\r\n").await.ok(), Some(Reply::Bulk(Vec::new())));
        assert_eq!(
            parse(b"$4\r\na\r\nb\r\n").await.ok(),
            Some(Reply::Bulk(b"a\r\nb".to_vec()))
        );
        assert_eq!(parse(b"*-1\r\n").await.ok(), Some(Reply::Nil));
        assert_eq!(
            parse(b"*2\r\n$1\r\nx\r\n*1\r\n:1\r\n").await.ok(),
            Some(Reply::Array(vec![
                Reply::Bulk(b"x".to_vec()),
                Reply::Array(vec![Reply::Integer(1)])
            ]))
        );
    }

    #[tokio::test]
    async fn broken_replies_are_errors_not_panics() {
        for bytes in [
            &b"?x\r\n"[..],
            b"$5\r\nab\r\n",
            b":abc\r\n",
            b"+OK\n",
            b"$9999999999\r\n",
            b"",
            b"$3\r\nabcXY",
        ] {
            assert!(parse(bytes).await.is_err(), "{:?}", String::from_utf8_lossy(bytes));
        }
        let deep = "*1\r\n".repeat(100) + ":1\r\n";
        assert!(parse(deep.as_bytes()).await.is_err());
    }
}
