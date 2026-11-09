use std::collections::BTreeMap;
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
