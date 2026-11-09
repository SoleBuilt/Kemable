//! Cloudflare's REST API for D1: list an account's databases, and run SQL on
//! one through `/raw`, which returns rows as arrays so column order and
//! repeated names survive.
//!
//! Every reply is Cloudflare's envelope, `{ success, errors: [{code, message}], result }`.
//! A refused token is HTTP 401 or 403, or error code 10000; a SQL error is
//! code 7500 with SQLite's message and `at offset N`.

use std::sync::Arc;
use std::time::Duration;

use paboable_core::config::Secret;
use paboable_core::{DriverError, ErrorCode};
use serde::Deserialize;
use serde_json::Value as Json;

pub const CLOUDFLARE_API: &str = "https://api.cloudflare.com/client/v4";

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    errors: Vec<ApiError>,
    #[serde(default)]
    result: Json,
    #[serde(default)]
    result_info: Option<ResultInfo>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct ResultInfo {
    #[serde(default)]
    count: u64,
    #[serde(default)]
    per_page: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseEntry {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct RawResult {
    #[serde(default)]
    pub results: Option<RawRows>,
    #[serde(default)]
    pub meta: Option<Meta>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RawRows {
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Vec<Json>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub changes: Option<u64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub rows_read: Option<u64>,
    #[serde(default)]
    pub rows_written: Option<u64>,
}

pub struct Api {
    client: reqwest::Client,
    /// `<base>/accounts/<account>/d1/database`.
    root: String,
    token: Secret,
}

impl Api {
    /// `base` is Cloudflare's API, or a loopback server in the tests; any
    /// other plain `http://` is refused, since the token travels in each request.
    pub fn new(base: &str, account: &str, token: Secret) -> Result<Self, DriverError> {
        let base = base.trim_end_matches('/');
        let loopback = ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
            .iter()
            .any(|prefix| base.starts_with(prefix));
        if !base.starts_with("https://") && !loopback {
            return Err(DriverError::config("the API address must be https://"));
        }
        let account = account.trim();
        if account.is_empty() || !account.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return Err(DriverError::config(
                "the account ID is 32 letters and digits, from the dashboard",
            ));
        }
        if token.is_empty() {
            return Err(DriverError::config("an API token with D1 permission is needed"));
        }
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .connect_timeout(Duration::from_secs(15))
            .user_agent(concat!("Paboable/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| DriverError::new(ErrorCode::Tls, error.to_string()))?;
        Ok(Self {
            client,
            root: format!("{base}/accounts/{account}/d1/database"),
            token,
        })
    }

    /// One page of the account's databases, `name` an exact filter; whether more pages follow.
    pub async fn databases(&self, name: Option<&str>, page: u32) -> Result<(Vec<DatabaseEntry>, bool), DriverError> {
        let per_page = 1000u32;
        let mut query = vec![("page", page.to_string()), ("per_page", per_page.to_string())];
        if let Some(name) = name {
            query.push(("name", name.to_owned()));
        }
        let envelope = self.send(self.client.get(&self.root).query(&query)).await?;
        let entries: Vec<DatabaseEntry> = serde_json::from_value(envelope.result).map_err(bad_reply)?;
        let more = envelope
            .result_info
            .is_some_and(|info| info.count >= info.per_page && info.per_page > 0);
        Ok((entries, more))
    }

    /// Runs `sql` (one statement here) on the database `id`.
    pub async fn raw(&self, id: &str, sql: &str) -> Result<Vec<RawResult>, DriverError> {
        let body = serde_json::json!({ "sql": sql });
        let url = format!("{}/{id}/raw", self.root);
        let envelope = self.send(self.client.post(url).json(&body)).await?;
        serde_json::from_value(envelope.result).map_err(bad_reply)
    }

    /// Runs `statements` as one batch, which D1 runs as one transaction.
    pub async fn batch(&self, id: &str, statements: &[String]) -> Result<Vec<RawResult>, DriverError> {
        let batch: Vec<Json> = statements.iter().map(|sql| serde_json::json!({ "sql": sql })).collect();
        let url = format!("{}/{id}/raw", self.root);
        let envelope = self
            .send(self.client.post(url).json(&serde_json::json!({ "batch": batch })))
            .await?;
        serde_json::from_value(envelope.result).map_err(bad_reply)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Envelope, DriverError> {
        let response = request
            .bearer_auth(self.token.expose())
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(transport_error)?;
        let envelope: Option<Envelope> = serde_json::from_slice(&bytes).ok();
        match envelope {
            Some(envelope) if envelope.success && status.is_success() => Ok(envelope),
            Some(envelope) => Err(api_error(status, &envelope.errors)),
            None => Err(DriverError::new(
                if matches!(status.as_u16(), 401 | 403) {
                    ErrorCode::Auth
                } else {
                    ErrorCode::Io
                },
                format!(
                    "HTTP {status}: {}",
                    String::from_utf8_lossy(&bytes[..bytes.len().min(300)])
                ),
            )),
        }
    }
}

/// rustls with ring and the Windows trust store, as the other drivers use, and HTTP/1.1.
fn tls_config() -> Result<rustls::ClientConfig, DriverError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls_error = |error: rustls::Error| DriverError::new(ErrorCode::Tls, error.to_string());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone()).map_err(tls_error)?;
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

fn bad_reply(error: serde_json::Error) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("an API reply that does not read: {error}"))
}

fn transport_error(error: reqwest::Error) -> DriverError {
    let code = if error.is_timeout() {
        ErrorCode::Timeout
    } else if error.is_connect() {
        ErrorCode::Connect
    } else {
        ErrorCode::Io
    };
    DriverError::new(code, error.to_string())
}

/// Cloudflare's first error, as a code: a refused token, a missing database, or the SQL's own error.
fn api_error(status: reqwest::StatusCode, errors: &[ApiError]) -> DriverError {
    let first = errors.first();
    let message = first.map_or_else(|| format!("HTTP {status}"), |error| error.message.clone());
    let engine_code = first.map(|error| error.code.to_string()).unwrap_or_default();
    let code = match (status.as_u16(), first.map(|error| error.code)) {
        (401 | 403, _) | (_, Some(10000)) => ErrorCode::Auth,
        (404, _) => ErrorCode::NotFound,
        (429, _) => ErrorCode::Io,
        _ => ErrorCode::Query,
    };
    let mut error = DriverError::new(code, message.clone()).with_engine_code(engine_code);
    if let Some(offset) = sql_offset(&message) {
        error = error.with_position(offset);
    }
    error
}

/// The byte offset SQLite names in `... syntax error at offset 7: SQLITE_ERROR`.
pub fn sql_offset(message: &str) -> Option<usize> {
    let rest = &message[message.find("at offset ")? + "at offset ".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}
