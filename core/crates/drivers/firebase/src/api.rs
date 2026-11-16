//! Firestore's REST API and the Realtime Database's, signed in as a service
//! account (`auth`), through our own rustls config (ring, the Windows trust
//! store), as D1's client is.

use std::sync::Arc;
use std::time::Duration;

use kemable_core::config::{ConnectionConfig, Secret, Target};
use kemable_core::{DriverError, ErrorCode, Result};
use serde_json::{Value as Json, json};

use crate::auth::{Login, ServiceAccount};

/// Firestore's API, before `/projects/…`.
const FIRESTORE: &str = "https://firestore.googleapis.com/v1";
/// How many documents or collection ids one request asks for.
pub(crate) const PAGE: u32 = 300;

pub(crate) struct Firebase {
    http: reqwest::Client,
    login: Login,
    pub(crate) project: String,
    /// `…/projects/<project>/databases/(default)/documents`.
    documents: String,
    /// The Realtime Database's address, `https://<project>-default-rtdb.firebaseio.com` unless set.
    rtdb: String,
}

/// The longest a reply may stay silent before its request fails.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

fn tls_config() -> Result<rustls::ClientConfig> {
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

fn transport_error(error: &reqwest::Error) -> DriverError {
    let code = if error.is_timeout() {
        ErrorCode::Timeout
    } else if error.is_connect() {
        ErrorCode::Connect
    } else {
        ErrorCode::Io
    };
    DriverError::new(code, error.to_string())
}

/// Google's error (`{"error": {"code", "message", "status"}}`) as a code:
/// a refused login, else the request's own error with its status.
fn api_error(status: reqwest::StatusCode, body: &Json) -> DriverError {
    // runQuery answers with a list, its error the first entry's.
    let error = body
        .get("error")
        .or_else(|| body.get(0).and_then(|first| first.get("error")));
    let message = error
        .and_then(|error| error.get("message").or(Some(error)))
        .and_then(|message| message.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("HTTP {status}"));
    let engine = error
        .and_then(|error| error.get("status"))
        .and_then(Json::as_str)
        .map_or_else(|| status.as_u16().to_string(), str::to_owned);
    let code = match status.as_u16() {
        401 | 403 => ErrorCode::Auth,
        _ => ErrorCode::Query,
    };
    DriverError::new(code, message).with_engine_code(engine)
}

fn plain_url(base: &str, what: &str) -> Result<String> {
    let base = base.trim().trim_end_matches('/');
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| base.starts_with(prefix));
    if base.starts_with("https://") || loopback {
        Ok(base.to_owned())
    } else {
        Err(DriverError::config(format!("{what} must be an https:// address")))
    }
}

/// A path's segments for a URL: each percent-encoded, `/` between them.
fn encode_path(path: &str) -> String {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            segment
                .bytes()
                .map(|byte| {
                    if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                        char::from(byte).to_string()
                    } else {
                        format!("%{byte:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

impl Firebase {
    /// Reads a config: the project id as the endpoint (or the key file's),
    /// the service account key as the token, or the path of its JSON file as
    /// the option `key_file` (read at connect, never kept). `emulator` (`host:port`) reaches
    /// the Firestore emulator with no key; `database_url` names the Realtime
    /// Database; `firestore_url` stands in for Google's API (tests).
    pub(crate) fn new(config: &ConnectionConfig) -> Result<Self> {
        let Target::Http { endpoint, token } = &config.target else {
            return Err(DriverError::config(
                "Firebase is reached over HTTPS: a project and a service account key",
            ));
        };
        let emulator = config
            .options
            .get("emulator")
            .map(|host| host.trim())
            .filter(|host| !host.is_empty());
        // The key's text, typed or pasted, else the key file's: read now, kept in memory only.
        let key = match config
            .options
            .get("key_file")
            .map(|path| path.trim())
            .filter(|path| !path.is_empty())
        {
            Some(path) if token.is_empty() => Secret::new(
                std::fs::read_to_string(path)
                    .map_err(|error| DriverError::config(format!("cannot read the key file {path}: {error}")))?,
            ),
            _ => token.clone(),
        };
        let (login, from_key) = match (key.is_empty(), emulator) {
            (true, Some(_)) => (Login::Emulator, None),
            (true, None) => return Err(DriverError::config("a service account key file is needed")),
            (false, _) => {
                let account = ServiceAccount::read(&key)?;
                let project = account.project_id.clone();
                (Login::account(account), project)
            }
        };
        let project = Some(endpoint.trim().to_owned())
            .filter(|project| !project.is_empty())
            .or(from_key)
            .ok_or_else(|| DriverError::config("the project id is needed"))?;
        if !project.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-') {
            return Err(DriverError::config(format!("{project} is not a project id")));
        }
        let api = match (config.options.get("firestore_url"), emulator) {
            (Some(url), _) => plain_url(url, "the Firestore address")?,
            (None, Some(host)) => format!("http://{host}/v1"),
            (None, None) => FIRESTORE.to_owned(),
        };
        let rtdb = match config
            .options
            .get("database_url")
            .map(|url| url.trim())
            .filter(|url| !url.is_empty())
        {
            Some(url) => plain_url(url, "the Realtime Database's address")?,
            None => format!("https://{project}-default-rtdb.firebaseio.com"),
        };
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .connect_timeout(Duration::from_secs(15))
            // A link that dies with a request in flight fails instead of
            // leaving the call waiting; Realtime streams send a keep-alive
            // well inside it.
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!("Kemable/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| DriverError::new(ErrorCode::Tls, error.to_string()))?;
        Ok(Self {
            http,
            login,
            documents: format!("{api}/projects/{project}/databases/(default)/documents"),
            project,
            rtdb,
        })
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Signs in (a token for a service account), so a wrong key fails at connect.
    pub(crate) async fn sign_in(&self) -> Result<()> {
        self.login.bearer(&self.http).await.map(|_| ())
    }

    pub(crate) async fn bearer(&self) -> Result<String> {
        self.login.bearer(&self.http).await
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Json> {
        let response = request
            .header("authorization", self.bearer().await?)
            .send()
            .await
            .map_err(|error| transport_error(&error))?;
        let status = response.status();
        let text = response.text().await.map_err(|error| transport_error(&error))?;
        let body: Json = serde_json::from_str(&text).unwrap_or(Json::String(text));
        if status.is_success() {
            Ok(body)
        } else {
            Err(api_error(status, &body))
        }
    }

    /// The collections under the root, or under a document.
    pub(crate) async fn collection_ids(&self, parent: Option<&str>) -> Result<Vec<String>> {
        let url = match parent {
            Some(path) => format!("{}/{}:listCollectionIds", self.documents, encode_path(path)),
            None => format!("{}:listCollectionIds", self.documents),
        };
        let mut ids = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut body = json!({ "pageSize": PAGE });
            if let Some(token) = &token {
                body["pageToken"] = json!(token);
            }
            let reply = self.send(self.http.post(&url).json(&body)).await?;
            ids.extend(
                reply
                    .get("collectionIds")
                    .and_then(Json::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| id.as_str().map(str::to_owned)),
            );
            token = reply
                .get("nextPageToken")
                .and_then(Json::as_str)
                .map(str::to_owned)
                .filter(|token| !token.is_empty());
            if token.is_none() || ids.len() >= 10_000 {
                return Ok(ids);
            }
        }
    }

    /// One page of a collection's documents, and the token of the next.
    pub(crate) async fn documents(&self, collection: &str, page: Option<&str>) -> Result<(Vec<Json>, Option<String>)> {
        let mut query = vec![("pageSize", PAGE.to_string())];
        if let Some(page) = page {
            query.push(("pageToken", page.to_owned()));
        }
        let url = format!("{}/{}", self.documents, encode_path(collection));
        let reply = self.send(self.http.get(url).query(&query)).await?;
        let documents = reply
            .get("documents")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        let next = reply
            .get("nextPageToken")
            .and_then(Json::as_str)
            .map(str::to_owned)
            .filter(|token| !token.is_empty());
        Ok((documents, next))
    }

    pub(crate) async fn document(&self, path: &str) -> Result<Json> {
        self.send(self.http.get(format!("{}/{}", self.documents, encode_path(path))))
            .await
    }

    /// A structured query on the collection at `collection` (its last segment
    /// is the query's `from`); the documents it matched.
    pub(crate) async fn run_query(&self, collection: &str, query: &Json) -> Result<Vec<Json>> {
        let parent = collection.rsplit_once('/').map(|(parent, _)| parent);
        let url = match parent {
            Some(parent) => format!("{}/{}:runQuery", self.documents, encode_path(parent)),
            None => format!("{}:runQuery", self.documents),
        };
        let reply = self
            .send(self.http.post(url).json(&json!({ "structuredQuery": query })))
            .await?;
        Ok(reply
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("document").cloned())
            .collect())
    }

    /// The Realtime Database's address for `path`, as `<path>.json`.
    pub(crate) fn rtdb_url(&self, path: &str) -> String {
        let path = encode_path(path);
        if path.is_empty() {
            format!("{}/.json", self.rtdb)
        } else {
            format!("{}/{path}.json", self.rtdb)
        }
    }

    /// The token as the Realtime Database takes it, in the query; none for an emulator.
    pub(crate) async fn rtdb_query(&self) -> Result<Vec<(&'static str, String)>> {
        let bearer = self.bearer().await?;
        Ok(match bearer.strip_prefix("Bearer ") {
            Some(token) if token != "owner" => vec![("access_token", token.to_owned())],
            _ => Vec::new(),
        })
    }

    /// The Realtime Database's value at `path`: whole, its children's keys
    /// only (`shallow`), or its first `limit` children by key. A reply whose
    /// text passes `max_bytes` is not read on: `result.too_big`.
    pub(crate) async fn rtdb(
        &self,
        path: &str,
        shallow: bool,
        limit: Option<u64>,
        max_bytes: Option<u64>,
    ) -> Result<Json> {
        let mut query = self.rtdb_query().await?;
        if shallow {
            query.push(("shallow", "true".to_owned()));
        }
        if let Some(limit) = limit {
            query.push(("orderBy", "\"$key\"".to_owned()));
            query.push(("limitToFirst", limit.to_string()));
        }
        let mut response = self
            .http
            .get(self.rtdb_url(path))
            .query(&query)
            .send()
            .await
            .map_err(|error| transport_error(&error))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| transport_error(&error))? {
            bytes.extend_from_slice(&chunk);
            if let Some(max) = max_bytes.filter(|max| bytes.len() as u64 > *max) {
                // Dropping the response closes the request: nothing more is read.
                return Err(DriverError::new(
                    ErrorCode::TooBig,
                    format!("the value at /{path} is over {max} bytes"),
                ));
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let body: Json = serde_json::from_str(&text).unwrap_or(Json::String(text));
        if status.is_success() {
            Ok(body)
        } else {
            // The Realtime Database's own error is `{"error": "Permission denied"}`.
            let message = body
                .get("error")
                .and_then(Json::as_str)
                .map_or_else(|| format!("HTTP {status}"), str::to_owned);
            let code = if matches!(status.as_u16(), 401 | 403) {
                ErrorCode::Auth
            } else {
                ErrorCode::Query
            };
            Err(DriverError::new(code, message).with_engine_code(status.as_u16().to_string()))
        }
    }
}
