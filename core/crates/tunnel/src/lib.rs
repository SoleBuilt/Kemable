//! SSH tunnels (`docs/decisions/0024-ssh-tunnels.md`): a port on `127.0.0.1`
//! whose every link is forwarded through an SSH server to a database's host
//! and port, as `ssh -L` does. A driver connects to the local end
//! (`ConnectionConfig::dial`) and never knows; the tunnel lasts as long as
//! the [`Tunnel`] value, and stops when it is dropped.
//!
//! The server's key is checked before anything is sent: against the user's
//! `~/.ssh/known_hosts`, then against Kemable's own file of hosts the user
//! trusted ([`HostKeys`]). An unknown key fails with `ssh.host_unknown` and
//! its fingerprint, so the UI can ask; a changed one fails with
//! `ssh.host_changed` and is never offered.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kemable_core::config::{SshAuth, SshTunnel};
use kemable_core::{CallOptions, DriverError, ErrorCode};
use russh::Disconnect;
use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// How often a quiet tunnel asks the server whether it is there, so a NAT or
/// a load balancer does not drop it, and how many unanswered asks end it.
const KEEPALIVE: Duration = Duration::from_secs(30);
const KEEPALIVE_MAX: usize = 3;

/// The known hosts files, and the keys seen for hosts not in them yet.
pub struct HostKeys {
    /// Kemable's own file, in the data folder: the hosts the user trusted here.
    own: PathBuf,
    /// The user's OpenSSH file, read only: hosts trusted with `ssh` count too.
    user: Option<PathBuf>,
    /// The key an unknown host showed, by host and port, until it is trusted.
    seen: Mutex<HashMap<(String, u16), PublicKey>>,
}

/// What the known hosts files say of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Known {
    Yes,
    No,
    Changed,
}

impl HostKeys {
    /// Kemable's file at `own`, and the user's `~/.ssh/known_hosts`.
    pub fn new(own: PathBuf) -> Self {
        let user = std::env::home_dir().map(|home| home.join(".ssh").join("known_hosts"));
        Self::with_user_file(own, user)
    }

    pub fn with_user_file(own: PathBuf, user: Option<PathBuf>) -> Self {
        Self {
            own,
            user,
            seen: Mutex::new(HashMap::new()),
        }
    }

    fn check(&self, host: &str, port: u16, key: &PublicKey) -> Known {
        let mut known = Known::No;
        for file in self.user.iter().chain(std::iter::once(&self.own)) {
            if !file.exists() {
                continue;
            }
            match russh::keys::check_known_hosts_path(host, port, key, file) {
                Ok(true) => known = Known::Yes,
                Ok(false) => {}
                Err(russh::keys::Error::KeyChanged { .. }) => return Known::Changed,
                // A file that cannot be read says nothing either way.
                Err(_) => {}
            }
        }
        known
    }

    fn remember(&self, host: &str, port: u16, key: PublicKey) {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert((host.to_owned(), port), key);
    }

    /// Trusts the key `host` showed when it was found unknown: it goes into
    /// Kemable's file, and the next connection goes through.
    pub fn trust(&self, host: &str, port: u16) -> Result<(), DriverError> {
        let key = self
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(host.to_owned(), port))
            .ok_or_else(|| DriverError::not_found("the SSH server's key"))?;
        russh::keys::known_hosts::learn_known_hosts_path(host, port, &key, &self.own)
            .map_err(|error| DriverError::new(ErrorCode::Io, format!("cannot write {}: {error}", self.own.display())))
    }
}

/// A key's fingerprint as OpenSSH prints it: `ED25519 SHA256:…`.
fn fingerprint(key: &PublicKey) -> String {
    format!("{} {}", key.algorithm(), key.fingerprint(HashAlg::Sha256))
}

/// Checks the server's key and keeps what it found for [`open`] to report.
struct Checker {
    host: String,
    port: u16,
    keys: Arc<HostKeys>,
    found: Arc<Mutex<Option<(Known, String)>>>,
}

impl client::Handler for Checker {
    type Error = russh::Error;

    async fn check_server_key(&mut self, server: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match server {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            // A certificate is checked by the key it certifies, as OpenSSH does without a CA line.
            PublicKeyOrCertificate::Certificate(certificate) => PublicKey::from(certificate.public_key().clone()),
        };
        let known = self.keys.check(&self.host, self.port, &key);
        *self.found.lock().unwrap_or_else(PoisonError::into_inner) = Some((known, fingerprint(&key)));
        if known == Known::No {
            self.keys.remember(&self.host, self.port, key);
        }
        Ok(known == Known::Yes)
    }
}

/// A local port forwarded through an SSH server. Dropping it stops the
/// listener and closes the SSH session, which ends every link through it.
pub struct Tunnel {
    local: SocketAddr,
    stop: CancellationToken,
    /// What opened it, to open it again after the SSH session ended.
    ssh: SshTunnel,
    to_host: String,
    to_port: u16,
    keys: Arc<HostKeys>,
}

impl Tunnel {
    /// The address a driver connects to: `127.0.0.1` and a port the system chose.
    pub fn local(&self) -> SocketAddr {
        self.local
    }

    /// Whether the SSH session or the listener has ended.
    pub fn is_closed(&self) -> bool {
        self.stop.is_cancelled()
    }

    /// A new tunnel through the same SSH server to the same host and port,
    /// on a new local port: after sleep or a dropped VPN the SSH session
    /// ends, and every link through it with it.
    pub async fn reopen(&self, options: &CallOptions) -> Result<Tunnel, DriverError> {
        open(&self.ssh, &self.to_host, self.to_port, Arc::clone(&self.keys), options).await
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn ssh_error(code: ErrorCode, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(code, error.to_string())
}

/// Opens a tunnel through `ssh` to `to_host`:`to_port`, as the SSH server
/// sees them. Returns once the server is checked, the login accepted and the
/// local port listening; `options` bounds that, not the tunnel's life.
pub async fn open(
    ssh: &SshTunnel,
    to_host: &str,
    to_port: u16,
    keys: Arc<HostKeys>,
    options: &CallOptions,
) -> Result<Tunnel, DriverError> {
    if ssh.host.trim().is_empty() || ssh.user.trim().is_empty() {
        return Err(DriverError::config("an SSH tunnel needs a host and a user"));
    }
    // The key is read first: a wrong path or passphrase is said before any network wait.
    let key = match &ssh.auth {
        SshAuth::Key { path, passphrase } => {
            let passphrase = (!passphrase.is_empty()).then(|| passphrase.expose());
            let key = russh::keys::load_secret_key(path, passphrase).map_err(|error| match error {
                russh::keys::Error::KeyIsEncrypted => ssh_error(ErrorCode::SshAuth, "the key needs its passphrase"),
                russh::keys::Error::IO(error) => {
                    DriverError::config(format!("cannot read the key {}: {error}", path.display()))
                }
                other => ssh_error(
                    ErrorCode::SshAuth,
                    format!("cannot read the key {}: {other}", path.display()),
                ),
            })?;
            Some(Arc::new(key))
        }
        SshAuth::Password { .. } => None,
    };
    let reopen_keys = Arc::clone(&keys);
    let found = Arc::new(Mutex::new(None));
    let checker = Checker {
        host: ssh.host.clone(),
        port: ssh.port(),
        keys,
        found: Arc::clone(&found),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE),
        keepalive_max: KEEPALIVE_MAX,
        nodelay: true,
        ..Default::default()
    });
    let session = options
        .run(async {
            client::connect(config, (ssh.host.as_str(), ssh.port()), checker)
                .await
                .map_err(
                    |error| match found.lock().unwrap_or_else(PoisonError::into_inner).take() {
                        Some((Known::No, fingerprint)) => ssh_error(ErrorCode::SshHostUnknown, fingerprint),
                        Some((Known::Changed, fingerprint)) => ssh_error(ErrorCode::SshHostChanged, fingerprint),
                        _ => ssh_error(ErrorCode::SshConnect, error),
                    },
                )
        })
        .await?;
    let session = options.run(sign_in(session, ssh, key)).await?;
    // One channel now, so a database the server cannot reach is said here,
    // not as a link the driver sees close.
    let probe = options
        .run(async {
            session
                .channel_open_direct_tcpip(to_host, u32::from(to_port), Ipv4Addr::LOCALHOST.to_string(), 0)
                .await
                .map_err(|error| ssh_error(ErrorCode::SshForward, format!("{to_host}:{to_port}: {error}")))
        })
        .await?;
    let _ = probe.close().await;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| ssh_error(ErrorCode::Io, error))?;
    let local = listener.local_addr().map_err(|error| ssh_error(ErrorCode::Io, error))?;
    let stop = CancellationToken::new();
    tokio::spawn(serve(
        listener,
        Arc::new(session),
        to_host.to_owned(),
        to_port,
        stop.clone(),
    ));
    Ok(Tunnel {
        local,
        stop,
        ssh: ssh.clone(),
        to_host: to_host.to_owned(),
        to_port,
        keys: reopen_keys,
    })
}

async fn sign_in(
    mut session: Handle<Checker>,
    ssh: &SshTunnel,
    key: Option<Arc<russh::keys::PrivateKey>>,
) -> Result<Handle<Checker>, DriverError> {
    let refused = |error: russh::Error| ssh_error(ErrorCode::SshAuth, error);
    let result = match (&ssh.auth, key) {
        (SshAuth::Password { password }, _) => session
            .authenticate_password(ssh.user.as_str(), password.expose())
            .await
            .map_err(refused)?,
        (SshAuth::Key { .. }, Some(key)) => {
            let hash = session.best_supported_rsa_hash().await.map_err(refused)?.flatten();
            session
                .authenticate_publickey(ssh.user.as_str(), PrivateKeyWithHashAlg::new(key, hash))
                .await
                .map_err(refused)?
        }
        (SshAuth::Key { .. }, None) => return Err(DriverError::internal("the key was not read")),
    };
    if result.success() {
        Ok(session)
    } else {
        Err(ssh_error(ErrorCode::SshAuth, format!("{} was not let in", ssh.user)))
    }
}

/// Takes links on the local port until `stop` fires or the SSH session ends,
/// forwarding each through its own channel.
async fn serve(
    listener: TcpListener,
    session: Arc<Handle<Checker>>,
    to_host: String,
    to_port: u16,
    stop: CancellationToken,
) {
    let mut alive = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            () = stop.cancelled() => break,
            _ = alive.tick() => {
                if session.is_closed() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((socket, from)) = accepted else { continue };
                let session = Arc::clone(&session);
                let to_host = to_host.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        () = stop.cancelled() => {}
                        () = forward(socket, from, &session, &to_host, to_port) => {}
                    }
                });
            }
        }
    }
    stop.cancel();
    let _ = session.disconnect(Disconnect::ByApplication, "", "en").await;
}

/// One link: a channel to the database's host and port, and the bytes
/// copied both ways until either side closes. A channel the server refuses
/// closes the local link, which the driver reports as it would any refusal.
async fn forward(mut socket: TcpStream, from: SocketAddr, session: &Handle<Checker>, to_host: &str, to_port: u16) {
    let _ = socket.set_nodelay(true);
    let Ok(channel) = session
        .channel_open_direct_tcpip(
            to_host,
            u32::from(to_port),
            from.ip().to_string(),
            u32::from(from.port()),
        )
        .await
    else {
        return;
    };
    let mut stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
}

#[cfg(test)]
mod tests;
