//! The text of a script file as the editor shows it, and back to the bytes
//! the file had (`docs/decisions/0016-sql-files.md`).
//!
//! A `.sql` file may be UTF-8, with or without a byte order mark, or UTF-16
//! (SSMS's older default), with CRLF or LF line endings. The editor works on
//! text with `\n` line endings; a save writes the encoding and the endings the
//! file was read with, so a file saved from Kemable differs from the one
//! opened only where the user typed.

use serde::{Deserialize, Serialize};

use crate::{DriverError, ErrorCode, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextEncoding {
    Utf8,
    /// UTF-8 that starts with the bytes EF BB BF.
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineEnding {
    Crlf,
    Lf,
}

/// A file's text with `\n` line endings, and how its bytes were written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    pub text: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
}

/// A new file: UTF-8 without a byte order mark, with Windows line endings.
pub const NEW_FILE: (TextEncoding, LineEnding) = (TextEncoding::Utf8, LineEnding::Crlf);

/// Reads a file's bytes as text. A byte order mark names the encoding;
/// without one the bytes must be UTF-8. The line endings are the first ones
/// the text has (CRLF for a text of one line).
pub fn decode(bytes: &[u8]) -> Result<TextFile> {
    let (encoding, text) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (TextEncoding::Utf8Bom, utf8(rest)?)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (TextEncoding::Utf16Le, utf16(rest, u16::from_le_bytes)?)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (TextEncoding::Utf16Be, utf16(rest, u16::from_be_bytes)?)
    } else {
        (TextEncoding::Utf8, utf8(bytes)?)
    };
    let line_ending = match text.find('\n') {
        Some(at) if at > 0 && text.as_bytes()[at - 1] == b'\r' => LineEnding::Crlf,
        Some(_) => LineEnding::Lf,
        None => LineEnding::Crlf,
    };
    // A lone \r (an old Mac line end) becomes a line end too, as the editor reads it.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    Ok(TextFile {
        text,
        encoding,
        line_ending,
    })
}

/// The bytes of `text` (with `\n` or `\r\n` line endings) in `encoding`, with `line_ending`.
pub fn encode(text: &str, encoding: TextEncoding, line_ending: LineEnding) -> Vec<u8> {
    let unified = text.replace("\r\n", "\n");
    let text = match line_ending {
        LineEnding::Crlf => unified.replace('\n', "\r\n"),
        LineEnding::Lf => unified,
    };
    match encoding {
        TextEncoding::Utf8 => text.into_bytes(),
        TextEncoding::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat(),
        TextEncoding::Utf16Le => [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
        TextEncoding::Utf16Be => [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect(),
    }
}

fn not_text(detail: &str) -> DriverError {
    DriverError::new(ErrorCode::NotText, detail.to_owned())
}

fn utf8(bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|error| not_text(&format!("not UTF-8 or UTF-16 text: {error}")))
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(not_text("UTF-16 text with an odd number of bytes"));
    }
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| unit(*pair)).collect();
    String::from_utf16(&units).map_err(|error| not_text(&format!("broken UTF-16 text: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "select N'chữ ký ✓' as [Tên]\nGO\nselect 2";

    #[test]
    fn every_encoding_reads_back_the_bytes_it_was_written_as() {
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Utf8Bom,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
        ] {
            for line_ending in [LineEnding::Crlf, LineEnding::Lf] {
                let bytes = encode(SCRIPT, encoding, line_ending);
                let read = decode(&bytes).unwrap();
                assert_eq!(read.text, SCRIPT, "{encoding:?} {line_ending:?}");
                assert_eq!((read.encoding, read.line_ending), (encoding, line_ending));
                assert_eq!(encode(&read.text, read.encoding, read.line_ending), bytes);
            }
        }
    }

    #[test]
    fn a_bom_names_the_encoding_and_its_bytes_are_kept() {
        let bom = [&[0xEF, 0xBB, 0xBF][..], b"select 1\r\nselect 2\r\n"].concat();
        let read = decode(&bom).unwrap();
        assert_eq!(read.text, "select 1\nselect 2\n");
        assert_eq!(read.encoding, TextEncoding::Utf8Bom);
        assert_eq!(read.line_ending, LineEnding::Crlf);
        // SSMS's UTF-16 LE, as it writes a file.
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("select 1\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let read = decode(&utf16).unwrap();
        assert_eq!(
            (read.text.as_str(), read.encoding),
            ("select 1\n", TextEncoding::Utf16Le)
        );
    }

    #[test]
    fn line_endings_are_the_first_ones_and_a_text_of_one_line_is_crlf() {
        assert_eq!(decode(b"a\nb\r\nc").unwrap().line_ending, LineEnding::Lf);
        assert_eq!(decode(b"a\r\nb\nc").unwrap().line_ending, LineEnding::Crlf);
        assert_eq!(decode(b"select 1").unwrap().line_ending, LineEnding::Crlf);
        // Mixed endings in the editor are one kind; a save writes the file's kind everywhere.
        let read = decode(b"a\r\nb\nc\rd").unwrap();
        assert_eq!(read.text, "a\nb\nc\nd");
        assert_eq!(encode(&read.text, read.encoding, read.line_ending), b"a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn bytes_that_are_not_text_are_refused_not_guessed() {
        // Windows-1252's "é" alone is not UTF-8.
        assert_eq!(decode(b"select 'caf\xe9'").unwrap_err().code, ErrorCode::NotText);
        assert_eq!(decode(&[0xFF, 0xFE, 0x41]).unwrap_err().code, ErrorCode::NotText);
        // A lone UTF-16 surrogate.
        assert_eq!(decode(&[0xFF, 0xFE, 0x00, 0xD8]).unwrap_err().code, ErrorCode::NotText);
        assert_eq!(decode(b"").unwrap().text, "");
    }
}
