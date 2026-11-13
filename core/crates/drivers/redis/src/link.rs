//! One connection to a Redis server, opened, logged in and switched to its
//! database, and opened again after anything left it in an unknown state.
//!
//! Commands on a link go one exchange at a time. A call that is cancelled or
//! runs past its deadline drops the link: its reply may still be on the way,
//! and closing the socket is also what stops a blocking command (`BLPOP`) on
//! the server. The next call opens a new link.

use std::sync::Arc;

use paboable_core::config::Secret;
use paboable_core::{CallOptions, DriverError, ErrorCode};
use rustls::ClientConfig;
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufStream};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::TlsConnector;

use crate::resp::{self, ReadError, Reply};

pub(crate) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub(crate) type Stream = BufStream<Box<dyn Io>>;

/// Where a link goes and how it logs in.
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Secret,
    pub database: u32,
    pub tls: Option<Arc<ClientConfig>>,
}

pub struct Link {
    endpoint: Endpoint,
    stream: Mutex<Option<Stream>>,
}

impl Link {
    /// Opens the first connection now, so a wrong address or password fails at connect.
    pub async fn open(endpoint: Endpoint, options: &CallOptions) -> Result<Self, DriverError> {
        let stream = options.run(connect(&endpoint)).await?;
        Ok(Self {
            endpoint,
            stream: Mutex::new(Some(stream)),
        })
    }

    pub fn database(&self) -> u32 {
        self.endpoint.database
    }

    /// Sends `commands` together and reads one reply each. A Redis error is a
    /// reply (`Reply::Error`), not a failure: the link stays good.
    pub async fn call(&self, commands: &[Vec<Vec<u8>>], options: &CallOptions) -> Result<Vec<Reply>, DriverError> {
        let mut guard = options.run(async { Ok(self.stream.lock().await) }).await?;
        let endpoint = &self.endpoint;
        let result = options
            .run(async {
                if guard.is_none() {
                    *guard = Some(connect(endpoint).await?);
                }
                let stream = guard.as_mut().ok_or_else(|| DriverError::internal("no link"))?;
                exchange(stream, commands).await
            })
            .await;
        if result.is_err() {
            *guard = None;
        }
        result
    }

    /// One command, its reply.
    pub async fn one(&self, command: Vec<Vec<u8>>, options: &CallOptions) -> Result<Reply, DriverError> {
        let mut replies = self.call(&[command], options).await?;
        replies.pop().ok_or_else(|| DriverError::internal("no reply"))
    }

    pub async fn close(&self) {
        if let Some(mut stream) = self.stream.lock().await.take() {
            let _ = stream.write_all(&resp::encode(&["QUIT"])).await;
            let _ = stream.flush().await;
        }
    }
}

async fn exchange(stream: &mut Stream, commands: &[Vec<Vec<u8>>]) -> Result<Vec<Reply>, DriverError> {
    for command in commands {
        stream.write_all(&resp::encode(command)).await.map_err(lost)?;
    }
    stream.flush().await.map_err(lost)?;
    let mut replies = Vec::with_capacity(commands.len());
    for _ in commands {
        replies.push(resp::read(stream).await.map_err(read_error)?);
    }
    Ok(replies)
}

/// A new connection, logged in and on the endpoint's database.
pub(crate) async fn connect(endpoint: &Endpoint) -> Result<Stream, DriverError> {
    let tcp = TcpStream::connect((endpoint.host.as_str(), endpoint.port))
        .await
        .map_err(|error| DriverError::new(ErrorCode::Connect, error.to_string()))?;
    let _ = tcp.set_nodelay(true);
    let io: Box<dyn Io> = match &endpoint.tls {
        None => Box::new(tcp),
        Some(config) => {
            let name = ServerName::try_from(endpoint.host.clone())
                .map_err(|error| DriverError::config(format!("{} is not a host name: {error}", endpoint.host)))?;
            let tls = TlsConnector::from(Arc::clone(config))
                .connect(name, tcp)
                .await
                .map_err(|error| DriverError::new(ErrorCode::Tls, error.to_string()))?;
            Box::new(tls)
        }
    };
    let mut stream = BufStream::new(io);

    let mut login = Vec::new();
    if !endpoint.password.is_empty() {
        let mut auth = vec![b"AUTH".to_vec()];
        if let Some(user) = endpoint.user.as_deref().filter(|user| !user.is_empty()) {
            auth.push(user.as_bytes().to_vec());
        }
        auth.push(endpoint.password.expose().as_bytes().to_vec());
        login.push(auth);
    }
    if endpoint.database != 0 {
        login.push(vec![b"SELECT".to_vec(), endpoint.database.to_string().into_bytes()]);
    }
    if !login.is_empty() {
        let replies = exchange(&mut stream, &login).await?;
        for (index, reply) in replies.into_iter().enumerate() {
            if let Reply::Error(text) = reply {
                let is_auth = index == 0 && !endpoint.password.is_empty();
                return Err(if is_auth { auth_error(&text) } else { query_error(&text) });
            }
        }
    }
    Ok(stream)
}

pub(crate) fn lost(error: std::io::Error) -> DriverError {
    DriverError::new(ErrorCode::Lost, error.to_string())
}

pub(crate) fn read_error(error: ReadError) -> DriverError {
    match error {
        ReadError::Io(error) => lost(error),
        ReadError::Protocol(text) => DriverError::new(ErrorCode::Io, text),
    }
}

/// A refused login: `WRONGPASS`, or an older server's `ERR invalid password`.
fn auth_error(text: &str) -> DriverError {
    DriverError::new(ErrorCode::Auth, text).with_engine_code(error_word(text))
}

/// An error reply, with its first word (`WRONGTYPE`, `ERR`, `NOAUTH`) as the engine's code.
pub fn query_error(text: &str) -> DriverError {
    let code = error_word(text);
    let kind = if matches!(code.as_str(), "NOAUTH" | "WRONGPASS" | "NOPERM") {
        ErrorCode::Auth
    } else {
        ErrorCode::Query
    };
    DriverError::new(kind, text).with_engine_code(code)
}

fn error_word(text: &str) -> String {
    text.split_whitespace().next().unwrap_or("ERR").to_owned()
}
