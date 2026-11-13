//! tiberius errors to `DriverError` codes.

use kemable_core::{DriverError, ErrorCode};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Connect,
    Run,
}

pub(crate) fn map(error: tiberius::error::Error, batch: Option<&str>, phase: Phase) -> DriverError {
    use tiberius::error::Error;
    match error {
        Error::Server(token) => {
            let code = match token.code() {
                // Login failed.
                18456 | 18452 => ErrorCode::Auth,
                // The login cannot open the database it asked for.
                4060 => ErrorCode::Connect,
                // A severity of 20 or more ends the connection.
                _ if token.class() >= 20 => ErrorCode::Lost,
                _ => ErrorCode::Query,
            };
            let mut message = token.message().to_owned();
            if !token.procedure().is_empty() {
                message.push_str(&format!(" (in {})", token.procedure()));
            }
            let error = DriverError::new(code, message).with_engine_code(token.code().to_string());
            match batch.and_then(|text| line_offset(text, token.line())) {
                Some(offset) if token.line() > 0 => error.with_position(offset),
                _ => error,
            }
        }
        Error::Io { kind, message } => {
            let code = match (kind, phase) {
                (std::io::ErrorKind::TimedOut, _) => ErrorCode::Timeout,
                (_, Phase::Connect) => ErrorCode::Connect,
                (_, Phase::Run) => ErrorCode::Lost,
            };
            DriverError::new(code, message)
        }
        Error::Tls(message) => DriverError::new(ErrorCode::Tls, message),
        Error::Routing { host, port } => DriverError::new(
            ErrorCode::Connect,
            format!("the server routed the login to {host}:{port}"),
        ),
        other => DriverError::new(
            if phase == Phase::Connect {
                ErrorCode::Connect
            } else {
                ErrorCode::Query
            },
            other.to_string(),
        ),
    }
}

/// SQL Server reports a line of the batch, from 1. The byte offset of that line's start.
pub(crate) fn line_offset(text: &str, line: u32) -> Option<usize> {
    let line = usize::try_from(line).ok()?.checked_sub(1)?;
    if line == 0 {
        return Some(0);
    }
    text.match_indices('\n').nth(line - 1).map(|(offset, _)| offset + 1)
}
