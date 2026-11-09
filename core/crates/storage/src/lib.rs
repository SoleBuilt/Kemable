//! Saved connections.
//!
//! The library is one JSON file, `connections.json`, in the folder the app
//! names. It never holds a secret: a saved password or token goes to the
//! [`SecretVault`] the app passes in (Windows Credential Manager in the
//! Windows app), under `connection/<id>/password`.
//!
//! Every change reads the file, changes it, and writes it back whole through
//! a temporary file and a rename, so a crash leaves the old file or the new
//! one, never half of one. A file that cannot be read is never written over:
//! the change fails instead, so a damaged library is not lost to a save.

pub mod history;
pub mod queries;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use paboable_core::config::{ConnectionConfig, Secret, Target};
use paboable_core::{DriverError, ErrorCode, Result};
use serde::{Deserialize, Serialize};

/// Where secrets live. The app implements it with the platform's store.
pub trait SecretVault: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Secret>>;
    fn set(&self, key: &str, secret: &Secret) -> Result<()>;
    /// Removing a key that is not there succeeds.
    fn delete(&self, key: &str) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedConnection {
    /// Empty for a connection not saved yet; `save` gives it one.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// The connection without its secret.
    pub config: ConnectionConfig,
    /// Whether the secret is kept in the vault. Without it, connecting asks.
    #[serde(default)]
    pub save_password: bool,
    /// When it was last opened, in milliseconds since the Unix epoch.
    #[serde(default)]
    pub last_used: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    connections: Vec<SavedConnection>,
}

const FILE_VERSION: u32 = 1;

pub struct ConnectionLibrary {
    path: PathBuf,
}

pub fn password_key(id: &str) -> String {
    format!("connection/{id}/password")
}

fn io_error(what: &str, path: &Path, error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorCode::Io, format!("{what} {}: {error}", path.display()))
}

/// The secret a config carries: a network password or an HTTP token.
fn secret_of(config: &ConnectionConfig) -> Option<&Secret> {
    match &config.target {
        Target::Network { password, .. } => Some(password),
        Target::Http { token, .. } => Some(token),
        Target::File { .. } => None,
    }
}

fn set_secret(config: &mut ConnectionConfig, secret: Secret) {
    match &mut config.target {
        Target::Network { password, .. } => *password = secret,
        Target::Http { token, .. } => *token = secret,
        Target::File { .. } => {}
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

impl ConnectionLibrary {
    /// A library kept in `folder`. The folder is created on the first save.
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            path: folder.join("connections.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<LibraryFile> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LibraryFile {
                    version: FILE_VERSION,
                    connections: Vec::new(),
                });
            }
            Err(error) => return Err(io_error("cannot read", &self.path, error)),
        };
        let mut file: LibraryFile =
            serde_json::from_slice(&bytes).map_err(|error| io_error("cannot understand", &self.path, error))?;
        // A secret in the file (written by hand, say) is never used or kept.
        for connection in &mut file.connections {
            set_secret(&mut connection.config, Secret::default());
        }
        Ok(file)
    }

    fn write(&self, file: &LibraryFile) -> Result<()> {
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| io_error("cannot create", folder, error))?;
        }
        let text = serde_json::to_vec_pretty(file).map_err(|error| DriverError::internal(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|error| io_error("cannot write", &temporary, error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| io_error("cannot replace", &self.path, error))
    }

    /// Every saved connection, in the order they were added.
    pub fn list(&self) -> Result<Vec<SavedConnection>> {
        Ok(self.read()?.connections)
    }

    /// Adds `connection`, or replaces the saved one with its id. The secret
    /// is `secret` when given, else the one in its config; it goes to the
    /// vault when `save_password` is on and is removed from it otherwise.
    pub fn save(
        &self,
        mut connection: SavedConnection,
        secret: Option<Secret>,
        vault: &dyn SecretVault,
    ) -> Result<SavedConnection> {
        if connection.name.trim().is_empty() {
            return Err(DriverError::config("a saved connection needs a name"));
        }
        let mut file = self.read()?;
        if connection.id.is_empty() {
            connection.id = uuid::Uuid::new_v4().to_string();
        }
        let secret = secret
            .or_else(|| secret_of(&connection.config).cloned())
            .unwrap_or_default();
        let key = password_key(&connection.id);
        if connection.save_password && !secret.is_empty() {
            vault.set(&key, &secret)?;
        } else if !connection.save_password {
            vault.delete(&key)?;
        }
        set_secret(&mut connection.config, Secret::default());

        match file.connections.iter_mut().find(|saved| saved.id == connection.id) {
            Some(saved) => *saved = connection.clone(),
            None => file.connections.push(connection.clone()),
        }
        file.version = FILE_VERSION;
        self.write(&file)?;
        Ok(connection)
    }

    pub fn delete(&self, id: &str, vault: &dyn SecretVault) -> Result<()> {
        let mut file = self.read()?;
        file.connections.retain(|saved| saved.id != id);
        self.write(&file)?;
        vault.delete(&password_key(id))
    }

    /// The saved connection and its config with the secret filled in from the
    /// vault, when one is kept.
    pub fn resolve(&self, id: &str, vault: &dyn SecretVault) -> Result<(SavedConnection, ConnectionConfig)> {
        let saved = self
            .read()?
            .connections
            .into_iter()
            .find(|saved| saved.id == id)
            .ok_or_else(|| DriverError::not_found("saved connection"))?;
        let mut config = saved.config.clone();
        if saved.save_password
            && let Some(secret) = vault.get(&password_key(id))?
        {
            set_secret(&mut config, secret);
        }
        Ok((saved, config))
    }

    /// Records that `id` was opened now.
    pub fn touch(&self, id: &str) -> Result<()> {
        let mut file = self.read()?;
        if let Some(saved) = file.connections.iter_mut().find(|saved| saved.id == id) {
            saved.last_used = Some(now_millis());
            self.write(&file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
