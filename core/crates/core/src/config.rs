use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A secret held in memory. Its `Debug` never prints it, so a logged config
/// does not leak a password.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() {
            "Secret(empty)"
        } else {
            "Secret(***)"
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TlsMode {
    Disable,
    /// TLS when the server offers it, certificate not checked.
    #[default]
    Prefer,
    /// Encrypted, certificate not checked.
    Require,
    /// The certificate chains to a trusted root and names the host.
    VerifyFull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    #[serde(rename_all = "camelCase")]
    File {
        path: PathBuf,
        #[serde(default)]
        read_only: bool,
    },
    #[serde(rename_all = "camelCase")]
    Network {
        host: String,
        port: Option<u16>,
        user: Option<String>,
        #[serde(default)]
        password: Secret,
        database: Option<String>,
        #[serde(default)]
        tls: TlsMode,
    },
    #[serde(rename_all = "camelCase")]
    Http {
        endpoint: String,
        #[serde(default)]
        token: Secret,
    },
}

/// What a driver needs to connect. Saved connections, with their names,
/// groups and keyring references, live in the storage layer and produce this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConfig {
    pub engine: String,
    pub target: Target,
    /// Engine-specific settings, such as `application_name` or `sslrootcert`.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// Reach a network target through this SSH server
    /// (`docs/decisions/0024-ssh-tunnels.md`). The app opens the tunnel and
    /// sets [`Self::dial`]; a driver never opens one itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshTunnel>,
    /// Where a driver opens its TCP link instead of the target's host and
    /// port: an SSH tunnel's local end. The host still names the server
    /// (TLS, SNI, a login that carries it). Set by the app, never kept or sent.
    #[serde(skip)]
    pub dial: Option<SocketAddr>,
}

/// An SSH server a connection goes through: the database's host and port
/// are then as that server sees them (often `localhost`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTunnel {
    pub host: String,
    /// 22 when not given.
    pub port: Option<u16>,
    pub user: String,
    pub auth: SshAuth,
}

impl SshTunnel {
    pub const DEFAULT_PORT: u16 = 22;

    pub fn port(&self) -> u16 {
        self.port.unwrap_or(Self::DEFAULT_PORT)
    }

    /// The password or the key's passphrase: what the vault keeps.
    pub fn secret(&self) -> &Secret {
        match &self.auth {
            SshAuth::Password { password } => password,
            SshAuth::Key { passphrase, .. } => passphrase,
        }
    }

    pub fn set_secret(&mut self, secret: Secret) {
        match &mut self.auth {
            SshAuth::Password { password } => *password = secret,
            SshAuth::Key { passphrase, .. } => *passphrase = secret,
        }
    }
}

/// How the SSH login is proven.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SshAuth {
    Password {
        #[serde(default)]
        password: Secret,
    },
    /// A private key file in OpenSSH or PEM form, its passphrase empty when it has none.
    Key {
        path: PathBuf,
        #[serde(default)]
        passphrase: Secret,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_password() {
        let target = Target::Network {
            host: "db".into(),
            port: None,
            user: Some("me".into()),
            password: Secret::new("hunter2"),
            database: None,
            tls: TlsMode::Require,
        };
        assert!(!format!("{target:?}").contains("hunter2"));
    }
}
