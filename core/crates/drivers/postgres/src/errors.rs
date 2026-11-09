//! tokio-postgres errors to `DriverError` codes.

use std::error::Error as _;

use paboable_core::{DriverError, ErrorCode};
use tokio_postgres::error::{DbError, ErrorPosition};

/// What the call was doing when it failed: a failure to reach the server
/// means "could not connect" while connecting and "connection lost" after.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Connect,
    Run,
}

pub(crate) fn map(error: &tokio_postgres::Error, statement: Option<&str>, phase: Phase) -> DriverError {
    if let Some(db) = error.as_db_error() {
        return from_server(db, statement);
    }
    let message = chain(error);
    if error.is_closed() {
        return DriverError::new(ErrorCode::Lost, message);
    }
    let mut source = error.source();
    while let Some(inner) = source {
        if inner.downcast_ref::<rustls::Error>().is_some() {
            return DriverError::new(ErrorCode::Tls, message);
        }
        if let Some(io) = inner.downcast_ref::<std::io::Error>() {
            if io
                .get_ref()
                .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some())
            {
                return DriverError::new(ErrorCode::Tls, message);
            }
            if io.kind() == std::io::ErrorKind::TimedOut {
                return DriverError::new(ErrorCode::Timeout, message);
            }
        }
        source = inner.source();
    }
    let code = match phase {
        Phase::Connect => ErrorCode::Connect,
        Phase::Run => ErrorCode::Lost,
    };
    DriverError::new(code, message)
}

/// An error the server sent, with its SQLSTATE, and its position turned from
/// a character count into a byte offset into the statement.
fn from_server(db: &DbError, statement: Option<&str>) -> DriverError {
    let sqlstate = db.code().code();
    let code = match sqlstate {
        "57014" => ErrorCode::Cancelled,
        "28P01" | "28000" => ErrorCode::Auth,
        "3D000" | "53300" | "57P03" => ErrorCode::Connect,
        state if state.starts_with("08") => ErrorCode::Lost,
        _ => ErrorCode::Query,
    };
    // psql shows the detail and the hint under the message; keep them.
    let mut message = db.message().to_owned();
    if let Some(detail) = db.detail() {
        message.push_str("\nDETAIL: ");
        message.push_str(detail);
    }
    if let Some(hint) = db.hint() {
        message.push_str("\nHINT: ");
        message.push_str(hint);
    }
    let error = DriverError::new(code, message).with_engine_code(sqlstate);
    match (db.position(), statement) {
        (Some(ErrorPosition::Original(position)), Some(text)) => match byte_offset(text, *position) {
            Some(offset) => error.with_position(offset),
            None => error,
        },
        _ => error,
    }
}

/// PostgreSQL counts a position in characters, from 1.
pub(crate) fn byte_offset(text: &str, position: u32) -> Option<usize> {
    let index = usize::try_from(position).ok()?.checked_sub(1)?;
    text.char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .or((index == text.chars().count()).then_some(text.len()))
}

fn chain(error: &tokio_postgres::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(inner) = source {
        let text = inner.to_string();
        if !message.contains(&text) {
            message.push_str(": ");
            message.push_str(&text);
        }
        source = inner.source();
    }
    message
}
