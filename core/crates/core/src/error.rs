use serde::Serialize;

/// A stable code a UI words for the user. The core holds no sentences meant
/// for the user; `message` is the engine's own text, kept for the detail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ErrorCode {
    #[serde(rename = "config.invalid")]
    ConfigInvalid,
    #[serde(rename = "driver.connect")]
    Connect,
    #[serde(rename = "driver.auth")]
    Auth,
    #[serde(rename = "driver.tls")]
    Tls,
    #[serde(rename = "driver.timeout")]
    Timeout,
    #[serde(rename = "driver.cancelled")]
    Cancelled,
    #[serde(rename = "driver.query")]
    Query,
    #[serde(rename = "driver.io")]
    Io,
    #[serde(rename = "driver.lost")]
    Lost,
    #[serde(rename = "driver.unsupported")]
    Unsupported,
    /// An id the caller holds names nothing open any more.
    #[serde(rename = "not_found")]
    NotFound,
    /// An edit named a row by its key and changed another number of rows:
    /// the row is gone, or the key is not unique. Nothing was saved.
    #[serde(rename = "edit.row_gone")]
    RowGone,
    #[serde(rename = "internal")]
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigInvalid => "config.invalid",
            Self::Connect => "driver.connect",
            Self::Auth => "driver.auth",
            Self::Tls => "driver.tls",
            Self::Timeout => "driver.timeout",
            Self::Cancelled => "driver.cancelled",
            Self::Query => "driver.query",
            Self::Io => "driver.io",
            Self::Lost => "driver.lost",
            Self::Unsupported => "driver.unsupported",
            Self::NotFound => "not_found",
            Self::RowGone => "edit.row_gone",
            Self::Internal => "internal",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where in the statement the engine says the error is, as a byte offset
/// into the text the driver was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ErrorPosition {
    pub byte_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct DriverError {
    pub code: ErrorCode,
    pub message: String,
    /// The engine's own error code, such as a SQLSTATE or an MSSQL error number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<ErrorPosition>,
}

impl DriverError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            engine_code: None,
            position: None,
        }
    }

    #[must_use]
    pub fn with_engine_code(mut self, engine_code: impl Into<String>) -> Self {
        self.engine_code = Some(engine_code.into());
        self
    }

    #[must_use]
    pub fn with_position(mut self, byte_offset: usize) -> Self {
        self.position = Some(ErrorPosition { byte_offset });
        self
    }

    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "cancelled")
    }

    pub fn timeout() -> Self {
        Self::new(ErrorCode::Timeout, "deadline passed")
    }

    pub fn unsupported(what: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, what)
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConfigInvalid, message)
    }

    pub fn not_found(what: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, what)
    }

    /// Statement `index` (from 0) of an edit changed `changed` rows, not one;
    /// `None` when the engine does not say how many.
    pub fn row_gone(index: usize, changed: Option<u64>) -> Self {
        let changed = changed.map_or_else(|| "not one".to_owned(), |count| count.to_string());
        Self::new(
            ErrorCode::RowGone,
            format!("statement {} changed {changed} rows", index + 1),
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub const fn code(&self) -> ErrorCode {
        self.code
    }
}
